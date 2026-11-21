# Checks that need a browser or a phone

These drive a real browser, or read a real iPhone, against the site. `cargo
test` does not run them and nothing else does either. Run them after a change
to the QR scanner, to what is kept in the browser's storage, to how a page
takes up what another page or tab has stored, or to the Player Data page.

Everything here is run from this folder:

```bash
cd tests/devices
```

## Browser

Needs Deno and Chrome. The checks open a browser of their own, without a
window, with nothing of yours in it.

```bash
deno task browser                              # the built site (_site), served here
deno task browser https://www.kipukas.cards    # the live site
deno run -A browser/tabs.ts                    # one check by itself; a site may follow
```

Build the site first (`deno task build` in the repository's root) when
checking the built one: it is `_site` that is served.

| File | What it checks |
|---|---|
| `browser/back.ts` | A page left with its scanner open and come back to with Back: the scanner is closed and holds no camera, and opens again with what was chosen on the page in between |
| `browser/switch.ts` | The switch-camera button, with pretend cameras: an iPhone's seven, a phone's two, a computer's that do not say which way they face, a single one |
| `browser/tabs.ts` | A choice made in one tab (the privacy notice, the camera, the light) is taken up by another that is already open, when the browser reports it and when the tab comes back into view |
| `browser/sync.ts` | A game moved from one browser to another through the Player Data page: the code read by the site's own scanner, typed, and opened as an address; what the receiver is asked and told; a code that is used, wrong, or called off |
| `browser/seat.ts` | A game moved while it is being played against someone, with a third browser as the opponent: the player's place in the room goes with it, the device it came from is out of the room, and a device that is in a room leaves it when it takes a game |
| `browser/site.ts` | What they share: the built site served locally, the browser, pretend cameras, the scanner's buttons and what it shows |
| `browser/all.ts` | Runs the five in turn |

Things to know:

- `CHROME` names another Chrome than the one in `/Applications`.
  `BROWSER=firefox` drives Firefox instead (`FIREFOX` names it).
- Chrome brings a page back from its back/forward cache on the live site, and
  loads it anew when the site is served locally. `back.ts` says which
  happened. Only the first is the case it was written for, so run it against
  the live site as well.
- The pretend cameras stand in for `getUserMedia` and `enumerateDevices`. They
  show what the site asks of a browser and does with the answer, not what a
  real browser answers. For that, see the phone below. A picture can be held
  up to them (`holdUp`), which is how `sync.ts` has the site's reader read a
  QR code.
- `sync.ts` and `seat.ts` need a relay. Against the built site each runs one here
  (`wrangler dev` in `signaling-server/`, with storage it throws away) and
  points both browsers at it. Against the live site the browsers use the
  live relay: two rooms' worth of traffic, each ended when its game has
  moved.

The one library is puppeteer-core (Apache-2.0), fetched by Deno into its own
cache.

## Phone

Reads Safari on an iPhone through its Web Inspector, over USB. It needs:

- the iPhone plugged in, unlocked, and trusting this Mac;
- Web Inspector turned on: Settings, Apps, Safari, Advanced;
- `uvx` (from [uv](https://docs.astral.sh/uv/)). Nothing is installed: `uvx`
  fetches pymobiledevice3 (GPL-3.0) into its own cache and runs it from
  there. Python 3.12 is named because the newest Python is too new for it.

```bash
# What a page has stored, and what Safari says of its cameras
uvx --python 3.12 --from pymobiledevice3 python phone/evaluate.py phone/snapshot.js

# Record what the scanner does, for ten minutes, while the phone is used
mkdir -p /tmp/kpk-commands
uvx --python 3.12 --from pymobiledevice3 python phone/record.py 600 /tmp/kpk-scanner.jsonl /tmp/kpk-commands
```

| File | What it does |
|---|---|
| `phone/evaluate.py` | Runs a script in a page of the site that is open on the phone and prints what it returns. The script is the body of an async function |
| `phone/snapshot.js` | A script for it: the scanner's stored choices, the camera permission, the cameras Safari names, whether an update is waiting |
| `phone/record.py` | Puts `instrument.js` in every page of the site open on the phone, again after each page change, and writes down what it records |
| `phone/instrument.js` | What is recorded: every camera asked for and given, the cameras listed, what the page tells the engine of the scanner and what the engine draws, what is stored, taps on the scanner's buttons, the page being left and brought back from the cache |
| `phone/inspector.py` | What the two share |

While `record.py` runs, a file ending in `.js` left in its commands folder is
run in the page as one expression and then removed. That presses the page's
buttons from the Mac:

```bash
echo "(document.querySelector('[data-qr-switch]').click(), 'pressed switch')" > /tmp/kpk-commands/01.js
```

Things learned the hard way:

- One inspector message of more than a few kilobytes is held back by the
  phone, and the session answers nothing from then on. `inspector.py` sends
  anything large in pieces (`put_in`). Keep command files small.
- The inspector does not wait for a promise. `evaluate.py` has the page put
  its answer in a global and asks for that.
- One session can hold a page. A script that was killed can leave one
  behind: `pkill -9 -f phone/record.py` before starting another.
- A press made from the Mac does start the camera, but the permission prompt
  still needs a tap on the phone.
- A page keeps Safari's old copy of the site until its update is accepted
  ("Update Now"). `snapshot.js` says whether one is waiting.
- The home-screen app has its own storage and its own copy of the site. It
  is listed as an app of its own while it is open.

What they leave on the phone: nothing but what the script you run does. The
instrument is gone when the page is loaded again. It carries its notes from
page to page in `sessionStorage` under `__kpk_diag_carry`, which goes with
the tab. Camera ids are shortened in what is recorded. Write logs outside the
repository.
