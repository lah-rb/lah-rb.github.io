//! One connection to the relay: the state machine.
//!
//! A link wants at most one socket at a time and at most one retry. It knows
//! nothing about rooms or the game: its owner says where to connect and what
//! to send, and is told what arrives. Nor does it hold a socket or a clock.
//! It asks for them ([`Ask`]) and is told what they do (`socket_*`,
//! `timer_fired`), which is what lets the tests below drive it with a fake
//! socket and a clock moved by hand. In the browser the engine's worker
//! holds the real ones (`crate::wire`).
//!
//! States:
//!
//! - idle: not wanted, no socket
//! - connecting: socket asked for, not open yet
//! - open: socket open
//! - waiting: socket lost, a retry is due
//!
//! Every socket gets a generation number, which is also its id. What is
//! reported of any socket other than the current one is ignored, so a late
//! `close` from an abandoned socket can never tear down or reschedule a
//! newer one.

/// How often the relay is pinged while the socket is open.
pub const PING_INTERVAL_MS: u32 = 20_000;
/// How long a pong may take before the socket counts as dead.
pub const PONG_TIMEOUT_MS: u32 = 10_000;
/// How long a socket may take to open.
pub const CONNECT_TIMEOUT_MS: u32 = 10_000;
/// The wait before the first retry; it doubles with each loss in a row.
pub const BASE_RETRY_MS: u32 = 1_000;
/// The longest wait before a retry, so retries stay frequent.
pub const MAX_RETRY_MS: u32 = 15_000;

/// Always exactly this text: the relay answers it without waking the room
/// (an automatic reply matched on the exact bytes).
pub const PING: &str = r#"{"type":"ping"}"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Connecting,
    Open,
    Waiting,
}

/// The waits a link can have running. One of each at most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Timer {
    Retry,
    Connect,
    Ping,
    Pong,
}

/// What a link asks of whoever holds its socket and its clock.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    /// Open a socket to `url`, to be known as `socket` from here on.
    Open { socket: u32, url: String },
    /// Close that socket. Nothing more need be reported of it.
    Close { socket: u32 },
    /// Send `text` on that socket, if it is open.
    Send { socket: u32, text: String },
    /// Report `timer` in `ms` milliseconds, in place of any earlier wait of
    /// the same name.
    Set { timer: Timer, ms: u32 },
    /// Forget that wait.
    Clear { timer: Timer },
}

/// What a link tells its owner.
#[derive(Debug, Clone, PartialEq)]
pub enum Told {
    /// The state changed.
    State { next: State, prev: State },
    /// A socket opened.
    Opened,
    /// A message from the relay, parsed. Pongs are not passed on.
    Message(serde_json::Value),
}

/// What a link asks its owner when it needs to know.
pub struct Owner {
    /// The relay address, asked for on every attempt. The owner puts what it
    /// wants (create, join, rejoin) in the address.
    pub url: Box<dyn Fn() -> String>,
    /// Asked when a socket is lost. `false` ends the retrying.
    pub should_retry: Box<dyn Fn() -> bool>,
    /// A number in `0.0..1.0`, to spread retries out.
    pub random: Box<dyn Fn() -> f64>,
}

pub struct Link {
    owner: Owner,
    state: State,
    wanted: bool,
    /// The current socket's id, while there is one.
    socket: Option<u32>,
    /// Raised for every socket and again when that socket is lost.
    generation: u32,
    /// Losses in a row since the owner last called `settled`.
    attempts: u32,
    /// The relay says which protocol it speaks (`v`) in its first reply.
    relay_version: u64,

    // The waits now running. Those that belong to a socket remember which.
    retry: bool,
    connect: Option<u32>,
    ping: Option<u32>,
    pong: Option<u32>,

    asks: Vec<Ask>,
    told: Vec<Told>,
}

impl Link {
    pub fn new(owner: Owner) -> Self {
        Link {
            owner,
            state: State::Idle,
            wanted: false,
            socket: None,
            generation: 0,
            attempts: 0,
            relay_version: 1,
            retry: false,
            connect: None,
            ping: None,
            pong: None,
            asks: Vec::new(),
            told: Vec::new(),
        }
    }

    // ── What the owner does ────────────────────────────────────────

    /// Want a connection. Connects now, or at once if a retry was due.
    pub fn start(&mut self) {
        self.wanted = true;
        self.open();
    }

    /// Stop wanting a connection: close the socket and call off any retry.
    pub fn stop(&mut self) {
        self.wanted = false;
        self.cancel(Timer::Retry);
        if self.socket.is_some() {
            self.drop_socket(self.generation);
        } else {
            self.set_state(State::Idle);
        }
    }

    /// Something suggests the network is back (the page is in view again,
    /// the browser says it is online). Retry now instead of waiting out the
    /// backoff, and check that a socket that seems open still is: `alive`
    /// says whether whoever holds it finds it open.
    pub fn kick(&mut self, alive: bool) {
        if !self.wanted {
            return;
        }
        match self.state {
            State::Waiting => {
                self.attempts = 0;
                self.open();
            }
            State::Open => {
                if self.socket.is_none() || !alive {
                    self.drop_socket(self.generation);
                } else {
                    self.ping(self.generation);
                }
            }
            _ => {}
        }
    }

    /// Close the current socket and reconnect after the usual backoff. For
    /// when the relay answered but refused us for now.
    pub fn bounce(&mut self) {
        if self.socket.is_some() {
            self.drop_socket(self.generation);
        }
    }

    /// The relay accepted us (room created or joined). Resets the backoff,
    /// so a socket that opens and is refused over and over still backs off.
    pub fn settled(&mut self) {
        self.attempts = 0;
    }

    /// Send a message. Returns false if the socket is not open.
    pub fn send(&mut self, text: &str) -> bool {
        match self.socket {
            Some(socket) if self.state == State::Open => {
                self.asks.push(Ask::Send { socket, text: text.to_string() });
                true
            }
            _ => false,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// The current socket's id, while there is one.
    pub fn socket(&self) -> Option<u32> {
        self.socket
    }

    pub fn relay_version(&self) -> u64 {
        self.relay_version
    }

    /// What the link has asked for since this was last called.
    pub fn take_asks(&mut self) -> Vec<Ask> {
        std::mem::take(&mut self.asks)
    }

    /// What the link has to tell since this was last called.
    pub fn take_told(&mut self) -> Vec<Told> {
        std::mem::take(&mut self.told)
    }

    // ── What the socket and the clock report ───────────────────────

    pub fn socket_opened(&mut self, socket: u32) {
        if socket != self.generation {
            return;
        }
        self.cancel(Timer::Connect);
        self.relay_version = 1;
        self.set_state(State::Open);
        self.ping = Some(socket);
        self.asks.push(Ask::Set { timer: Timer::Ping, ms: PING_INTERVAL_MS });
        self.told.push(Told::Opened);
    }

    pub fn socket_message(&mut self, socket: u32, text: &str) {
        if socket != self.generation {
            return;
        }
        let Ok(message) = serde_json::from_str::<serde_json::Value>(text) else {
            return;
        };
        if !message.is_object() {
            return;
        }
        if let Some(version) = message["v"].as_u64() {
            self.relay_version = version;
        }
        if message["type"] == "pong" {
            self.cancel(Timer::Pong);
            return;
        }
        self.told.push(Told::Message(message));
    }

    /// A failed socket reports an error and then that it closed. Both end in
    /// `lost`, which acts once per socket, so one failure makes one retry.
    pub fn socket_error(&mut self, socket: u32) {
        self.drop_socket(socket);
    }

    pub fn socket_closed(&mut self, socket: u32) {
        self.lost(socket);
    }

    pub fn timer_fired(&mut self, timer: Timer) {
        match timer {
            Timer::Retry => {
                if std::mem::take(&mut self.retry) {
                    self.open();
                }
            }
            Timer::Connect => {
                if let Some(socket) = self.connect.take() {
                    if socket == self.generation && self.state == State::Connecting {
                        self.drop_socket(socket);
                    }
                }
            }
            Timer::Ping => {
                if let Some(socket) = self.ping.take() {
                    if socket == self.generation {
                        self.ping(socket);
                        self.ping = Some(socket);
                        self.asks.push(Ask::Set { timer: Timer::Ping, ms: PING_INTERVAL_MS });
                    }
                }
            }
            Timer::Pong => {
                // No pong from the relay: the socket is dead.
                if let Some(socket) = self.pong.take() {
                    self.drop_socket(socket);
                }
            }
        }
    }

    // ── Inside ─────────────────────────────────────────────────────

    fn set_state(&mut self, next: State) {
        if self.state == next {
            return;
        }
        let prev = self.state;
        self.state = next;
        self.told.push(Told::State { next, prev });
    }

    /// Call off a wait, if it is running.
    fn cancel(&mut self, timer: Timer) {
        let running = match timer {
            Timer::Retry => std::mem::take(&mut self.retry),
            Timer::Connect => self.connect.take().is_some(),
            Timer::Ping => self.ping.take().is_some(),
            Timer::Pong => self.pong.take().is_some(),
        };
        if running {
            self.asks.push(Ask::Clear { timer });
        }
    }

    /// Exponential backoff with jitter, capped so retries stay frequent.
    fn retry_delay(&self) -> u32 {
        let base = (BASE_RETRY_MS << self.attempts.min(10)).min(MAX_RETRY_MS) as f64;
        (base + (self.owner.random)() * base * 0.3).round() as u32
    }

    fn open(&mut self) {
        if !self.wanted || self.state == State::Connecting || self.state == State::Open {
            return;
        }
        self.cancel(Timer::Retry);

        self.generation += 1;
        let socket = self.generation;
        self.socket = Some(socket);
        self.asks.push(Ask::Open { socket, url: (self.owner.url)() });
        self.set_state(State::Connecting);
        self.connect = Some(socket);
        self.asks.push(Ask::Set { timer: Timer::Connect, ms: CONNECT_TIMEOUT_MS });
    }

    /// The socket `socket` is gone: closed, failed, or abandoned.
    fn lost(&mut self, socket: u32) {
        if socket != self.generation {
            return; // of an older socket
        }
        self.generation += 1; // anything else this socket reports is stale too
        self.cancel(Timer::Connect);
        self.cancel(Timer::Ping);
        self.cancel(Timer::Pong);
        self.socket = None;

        if !self.wanted || !(self.owner.should_retry)() {
            self.wanted = false;
            self.set_state(State::Idle);
            return;
        }
        self.set_state(State::Waiting);
        let ms = self.retry_delay();
        self.attempts += 1;
        self.retry = true;
        self.asks.push(Ask::Set { timer: Timer::Retry, ms });
    }

    /// Abandon the current socket ourselves (timeout, dead socket, bounce,
    /// stop).
    fn drop_socket(&mut self, socket: u32) {
        if socket != self.generation {
            return;
        }
        self.lost(socket); // first, so that what the closing reports is already stale
        self.asks.push(Ask::Close { socket });
    }

    fn ping(&mut self, socket: u32) {
        if socket != self.generation {
            return;
        }
        self.send(PING);
        // Only once the relay has said it speaks the protocol that answers
        // pings is a missing pong taken for a dead socket.
        if self.relay_version >= 2 && self.pong.is_none() {
            self.pong = Some(socket);
            self.asks.push(Ask::Set { timer: Timer::Pong, ms: PONG_TIMEOUT_MS });
        }
    }
}

/// A fake socket holder and a clock moved by hand, for tests of a link and
/// of what is built on one.
#[cfg(test)]
pub mod rig {
    use super::*;
    use std::collections::BTreeMap;

    /// A socket the link asked for.
    #[derive(Debug, Clone)]
    pub struct Socket {
        pub id: u32,
        pub url: String,
        /// Whether whoever holds it finds it open.
        pub open: bool,
        pub closed: bool,
        pub sent: Vec<String>,
    }

    /// Holds what a link asks for: its sockets, and its waits with the time
    /// each is due.
    #[derive(Default)]
    pub struct Rig {
        pub now: u64,
        pub sockets: Vec<Socket>,
        pub timers: BTreeMap<Timer, u64>,
    }

    impl Rig {
        /// Do what the link has asked for.
        pub fn carry_out(&mut self, link: &mut Link) {
            for ask in link.take_asks() {
                match ask {
                    Ask::Open { socket, url } => self.sockets.push(Socket {
                        id: socket,
                        url,
                        open: false,
                        closed: false,
                        sent: Vec::new(),
                    }),
                    // Like a real socket, closing does not report `close` at once.
                    Ask::Close { socket } => {
                        if let Some(s) = self.sockets.iter_mut().find(|s| s.id == socket) {
                            s.open = false;
                            s.closed = true;
                        }
                    }
                    Ask::Send { socket, text } => {
                        if let Some(s) = self.sockets.iter_mut().find(|s| s.id == socket) {
                            s.sent.push(text);
                        }
                    }
                    Ask::Set { timer, ms } => {
                        self.timers.insert(timer, self.now + u64::from(ms));
                    }
                    Ask::Clear { timer } => {
                        self.timers.remove(&timer);
                    }
                }
            }
        }

        /// The next wait due by `end`, earliest first.
        pub fn next_due(&mut self, end: u64) -> Option<Timer> {
            let (&timer, &at) = self.timers.iter().filter(|(_, at)| **at <= end).min_by_key(|(_, at)| **at)?;
            self.timers.remove(&timer);
            self.now = at;
            Some(timer)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::rig::Rig;
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    /// A link with a fake socket holder and a clock moved by hand.
    struct Bench {
        link: Link,
        rig: Rig,
        states: Vec<State>,
        messages: Vec<serde_json::Value>,
        opens: u32,
        keep: Rc<Cell<bool>>,
    }

    fn setup() -> Bench {
        let keep = Rc::new(Cell::new(true));
        let asked = keep.clone();
        Bench {
            link: Link::new(Owner {
                url: Box::new(|| "ws://fake/ws".to_string()),
                should_retry: Box::new(move || asked.get()),
                random: Box::new(|| 0.0), // no jitter
            }),
            rig: Rig::default(),
            states: Vec::new(),
            messages: Vec::new(),
            opens: 0,
            keep,
        }
    }

    impl Bench {
        /// After anything is done to the link: do what it asked for and note
        /// what it told.
        fn settle(&mut self) {
            self.rig.carry_out(&mut self.link);
            for told in self.link.take_told() {
                match told {
                    Told::State { next, .. } => self.states.push(next),
                    Told::Opened => self.opens += 1,
                    Told::Message(message) => self.messages.push(message),
                }
            }
        }

        fn start(&mut self) {
            self.link.start();
            self.settle();
        }
        fn stop(&mut self) {
            self.link.stop();
            self.settle();
        }
        fn kick(&mut self) {
            let alive = self.rig.sockets.last().is_some_and(|s| s.open);
            self.link.kick(alive);
            self.settle();
        }
        fn bounce(&mut self) {
            self.link.bounce();
            self.settle();
        }
        fn send(&mut self, text: &str) -> bool {
            let sent = self.link.send(text);
            self.settle();
            sent
        }

        // What the socket numbered `index` (in the order asked for) does.
        fn succeed(&mut self, index: usize) {
            self.rig.sockets[index].open = true;
            self.link.socket_opened(self.rig.sockets[index].id);
            self.settle();
        }
        /// A failed connection reports `error` and then `close`.
        fn fail(&mut self, index: usize) {
            self.rig.sockets[index].open = false;
            let id = self.rig.sockets[index].id;
            self.link.socket_error(id);
            self.link.socket_closed(id);
            self.settle();
        }
        fn server_close(&mut self, index: usize) {
            self.rig.sockets[index].open = false;
            self.link.socket_closed(self.rig.sockets[index].id);
            self.settle();
        }
        fn receive(&mut self, index: usize, message: serde_json::Value) {
            self.link.socket_message(self.rig.sockets[index].id, &message.to_string());
            self.settle();
        }

        /// Run every wait due within the next `ms`, in time order.
        fn advance(&mut self, ms: u64) {
            let end = self.rig.now + ms;
            while let Some(timer) = self.rig.next_due(end) {
                self.link.timer_fired(timer);
                self.settle();
            }
            self.rig.now = end;
        }

        fn sockets(&self) -> usize {
            self.rig.sockets.len()
        }
        fn pending(&self) -> usize {
            self.rig.timers.len()
        }
        fn pings(&self, index: usize) -> usize {
            self.rig.sockets[index].sent.iter().filter(|text| *text == PING).count()
        }
    }

    #[test]
    fn opens_one_socket_and_reports_open_once() {
        let mut t = setup();
        t.start();
        assert_eq!(t.link.state(), State::Connecting);
        t.succeed(0);
        assert_eq!(t.link.state(), State::Open);
        assert_eq!(t.opens, 1);
        assert_eq!(t.sockets(), 1);
        assert_eq!(t.rig.sockets[0].url, "ws://fake/ws");
        assert_eq!(t.states, [State::Connecting, State::Open]);
    }

    #[test]
    fn start_while_connecting_or_open_does_not_create_a_second_socket() {
        let mut t = setup();
        t.start();
        t.start();
        t.succeed(0);
        t.start();
        assert_eq!(t.sockets(), 1);
    }

    #[test]
    fn a_failed_open_error_then_close_schedules_exactly_one_retry() {
        let mut t = setup();
        t.start();
        t.fail(0);
        assert_eq!(t.link.state(), State::Waiting);
        assert_eq!(t.pending(), 1, "pending timers");
        t.advance(1000);
        assert_eq!(t.sockets(), 2, "sockets after first retry");
        // Nothing else is queued behind it: only the new socket's connect timeout.
        assert_eq!(t.pending(), 1, "pending timers after retry");
    }

    #[test]
    fn a_late_close_from_an_abandoned_socket_is_ignored() {
        let mut t = setup();
        t.start();
        t.succeed(0);
        t.server_close(0);
        t.advance(1000);
        t.succeed(1);
        assert_eq!(t.link.state(), State::Open);

        // The old socket reports close again, late.
        let first = t.rig.sockets[0].id;
        t.link.socket_closed(first);
        t.link.socket_error(first);
        t.settle();
        assert_eq!(t.link.state(), State::Open);
        assert_eq!(t.sockets(), 2);
        assert!(t.send(r#"{"type":"relay"}"#), "send should still reach the current socket");
        assert_eq!(t.rig.sockets[1].sent.len(), 1);
        assert!(t.rig.sockets[0].sent.is_empty());
    }

    #[test]
    fn retries_never_stop_and_the_delay_is_capped() {
        let mut t = setup();
        t.start();
        for i in 0..25usize {
            t.fail(i);
            assert_eq!(t.link.state(), State::Waiting, "attempt {}", i);
            let delay = (1000u64 << i.min(10)).min(15_000); // jitter is off in tests
            t.advance(delay - 1);
            assert_eq!(t.sockets(), i + 1, "no retry before {}ms (attempt {})", delay, i);
            t.advance(1);
            assert_eq!(t.sockets(), i + 2, "retry at {}ms (attempt {})", delay, i);
        }
    }

    #[test]
    fn backoff_grows_1s_2s_4s() {
        let mut t = setup();
        t.start();
        t.fail(0);
        t.advance(999);
        assert_eq!(t.sockets(), 1);
        t.advance(1);
        assert_eq!(t.sockets(), 2);
        t.fail(1);
        t.advance(1999);
        assert_eq!(t.sockets(), 2);
        t.advance(1);
        assert_eq!(t.sockets(), 3);
        t.fail(2);
        t.advance(3999);
        assert_eq!(t.sockets(), 3);
        t.advance(1);
        assert_eq!(t.sockets(), 4);
    }

    #[test]
    fn jitter_adds_up_to_three_tenths_of_the_wait() {
        let mut t = setup();
        t.link.owner.random = Box::new(|| 0.5);
        t.start();
        t.fail(0);
        t.advance(1149);
        assert_eq!(t.sockets(), 1);
        t.advance(1);
        assert_eq!(t.sockets(), 2);
    }

    #[test]
    fn gives_up_when_should_retry_says_no() {
        let mut t = setup();
        t.start();
        t.fail(0);
        assert_eq!(t.link.state(), State::Waiting);
        t.advance(1000);
        t.keep.set(false);
        t.fail(1);
        assert_eq!(t.link.state(), State::Idle);
        assert_eq!(t.pending(), 0);
    }

    #[test]
    fn kick_reconnects_at_once_from_waiting() {
        let mut t = setup();
        t.start();
        t.fail(0);
        t.advance(1000);
        t.fail(1); // now waiting about 2s
        t.kick();
        assert_eq!(t.sockets(), 3);
        assert_eq!(t.link.state(), State::Connecting);
        assert_eq!(t.pending(), 1, "only the connect timeout is pending");
    }

    #[test]
    fn kick_drops_a_socket_that_claims_to_be_open_but_is_not() {
        let mut t = setup();
        t.start();
        t.succeed(0);
        t.rig.sockets[0].open = false; // died without reporting close (phone slept)
        t.kick();
        assert_eq!(t.link.state(), State::Waiting);
    }

    #[test]
    fn kick_pings_a_socket_that_is_open_and_does_nothing_when_not_wanted() {
        let mut t = setup();
        t.kick();
        assert_eq!(t.sockets(), 0);
        t.start();
        t.succeed(0);
        t.kick();
        assert_eq!(t.link.state(), State::Open);
        assert_eq!(t.pings(0), 1);
    }

    #[test]
    fn stop_closes_the_socket_cancels_retries_and_stays_idle() {
        let mut t = setup();
        t.start();
        t.succeed(0);
        t.stop();
        assert_eq!(t.link.state(), State::Idle);
        assert!(t.rig.sockets[0].closed);
        assert_eq!(t.pending(), 0);
        let first = t.rig.sockets[0].id;
        t.link.socket_closed(first);
        t.settle();
        t.advance(60_000);
        assert_eq!(t.sockets(), 1);
    }

    #[test]
    fn stop_while_waiting_calls_off_the_retry() {
        let mut t = setup();
        t.start();
        t.fail(0);
        t.stop();
        assert_eq!(t.link.state(), State::Idle);
        assert_eq!(t.pending(), 0);
        t.advance(60_000);
        assert_eq!(t.sockets(), 1);
    }

    #[test]
    fn a_connect_that_never_opens_times_out_and_retries() {
        let mut t = setup();
        t.start();
        t.advance(10_000);
        assert_eq!(t.link.state(), State::Waiting);
        assert!(t.rig.sockets[0].closed, "the socket that never opened is closed");
        t.advance(1000);
        assert_eq!(t.sockets(), 2);
    }

    #[test]
    fn pings_are_always_the_same_message_so_the_relay_can_answer_without_waking() {
        let mut t = setup();
        t.start();
        t.succeed(0);
        t.advance(20_000);
        t.advance(20_000);
        assert_eq!(t.rig.sockets[0].sent, [r#"{"type":"ping"}"#, r#"{"type":"ping"}"#]);
    }

    #[test]
    fn before_the_relay_says_which_protocol_it_speaks_a_missing_pong_is_tolerated() {
        let mut t = setup();
        t.start();
        t.succeed(0);
        t.receive(0, serde_json::json!({ "type": "room_joined", "code": "ABCD" })); // no `v`
        t.advance(20_000);
        assert_eq!(t.pings(0), 1);
        t.advance(100_000);
        assert_eq!(t.link.state(), State::Open);
        assert_eq!(t.pings(0), 6);
    }

    #[test]
    fn once_the_relay_has_announced_v2_a_missing_pong_drops_the_socket_and_a_pong_keeps_it() {
        let mut t = setup();
        t.start();
        t.succeed(0);
        t.receive(0, serde_json::json!({ "type": "room_joined", "code": "ABCD", "v": 2 }));
        assert_eq!(t.link.relay_version(), 2);

        t.advance(20_000);
        t.receive(0, serde_json::json!({ "type": "pong" }));
        t.advance(15_000);
        assert_eq!(t.link.state(), State::Open, "pong arrived");

        t.advance(5_000); // second ping at 40s, no pong
        t.advance(10_000);
        assert_eq!(t.link.state(), State::Waiting, "pong missing");
        assert_eq!(t.messages.len(), 1, "pong is not passed to the owner");
    }

    #[test]
    fn bounce_backs_off_until_the_owner_reports_settled() {
        let mut t = setup();
        t.start();
        t.succeed(0);
        t.bounce(); // refused: retry in 1s
        t.advance(1000);
        t.succeed(1);
        t.bounce(); // refused again: retry in 2s, not 1s
        t.advance(1000);
        assert_eq!(t.sockets(), 2);
        t.advance(1000);
        assert_eq!(t.sockets(), 3);
        t.succeed(2);
        t.link.settled();
        t.bounce(); // back to 1s
        t.advance(1000);
        assert_eq!(t.sockets(), 4);
    }

    #[test]
    fn send_reports_false_when_the_socket_is_not_open() {
        let mut t = setup();
        assert!(!t.send(r#"{"type":"relay"}"#));
        t.start();
        assert!(!t.send(r#"{"type":"relay"}"#));
        t.succeed(0);
        assert!(t.send(r#"{"type":"relay"}"#));
    }

    #[test]
    fn what_is_not_a_message_from_the_relay_is_not_passed_on() {
        let mut t = setup();
        t.start();
        t.succeed(0);
        let id = t.rig.sockets[0].id;
        for text in ["not json", "42", "null", r#""text""#] {
            t.link.socket_message(id, text);
        }
        t.settle();
        assert!(t.messages.is_empty());
    }
}
