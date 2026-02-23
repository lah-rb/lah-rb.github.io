# Kipukas — Contributing Guide

> Practices, architecture, proven patterns, and workflow for the Kipukas card game platform.

---

## Table of Contents

1. [Practices, Principles & Philosophies](#practices-principles--philosophies)
2. [Full Stack Architecture](#full-stack-architecture)
3. [Proven Patterns](#proven-patterns)
4. [Technology Stack & Licenses](#technology-stack--licenses)
5. [Development Workflow](#development-workflow)
6. [Phase History](#phase-history)
7. [Desired Next Features](#desired-next-features)

---

## Practices, Principles & Philosophies

### Offline-First / PWA-First

The site works without internet after the first load. Workbox **injectManifest** mode gives full control over the service worker lifecycle. Updates use a user-controlled flow — a toast notification appears when new content is deployed, and the user chooses when to apply it. No surprise reloads.

### Decentralized Architecture

Game logic runs **100% client-side** in WebAssembly. There is no backend server processing game state. The only hosted component is a stateless WebSocket signaling relay (~130 lines) that brokers WebRTC connections between players. After the peer-to-peer connection is established, the signaling server can go offline and the game continues.

### HTMX Over SPA Frameworks

Instead of React, Vue, or Svelte, the project uses **HTMX** to add dynamic behavior to server-rendered HTML. The "server" happens to be a Rust WASM module running in a Web Worker inside the browser — but HTMX doesn't know or care. This fits naturally with Jekyll's static HTML model: just add `hx-*` attributes to existing markup.

### Incremental Migration (Alpine.js + HTMX Coexistence)

Alpine.js and HTMX coexist throughout the codebase. The guiding principle:

| Layer | Technology | Examples |
|-------|-----------|----------|
| **UI chrome** (visual-only) | Alpine.js | Modal open/close, hamburger menu, visibility toggles, animations |
| **Data & logic** | HTMX + WASM | Card filtering, damage tracking, type matchups, combat resolution |

A feature migrates from Alpine to HTMX when it involves data processing, complex state machines, or cross-player synchronization. A feature stays in Alpine when it's purely visual with no data dependencies.

### Type Safety via Rust

Game logic has been ported from JavaScript to Rust, compiled to WASM. The Rust type system catches bugs at compile time that JavaScript hides. The crate currently has **114 unit tests** covering route handlers, game logic, matchup tables, combat outcomes, and edge cases.

### Build-Time Code Generation

Card metadata is extracted from Jekyll `_posts/*.html` YAML front matter at build time by a Deno script (`scripts/build-card-catalog.ts`). This generates a Rust source file (`kipukas-server/src/cards_generated.rs`) containing a static array of `Card` structs compiled directly into the WASM binary. No runtime data fetching, no JSON loading, no IndexedDB — just compiled-in data.

### Two-Scope State Model

All data is tracked in two distinct scopes:

| Scope | Storage | Synced via WebRTC? | Examples |
|-------|---------|-------------------|----------|
| **Local User** | WASM `GameState` + localStorage | No | Damage tracking, turn alarms, card browsing |
| **Global (Room)** | WASM `RoomState` + WebRTC data channel | Yes | Fists combat submissions, combat results, outcome damage |

A feature defaults to local user state unless it explicitly requires cross-player visibility. Single-player behavior is completely unaffected by multiplayer code.

### Minimal Infrastructure

- **Hosting:** GitHub Pages (free, static)
- **Game logic:** In-browser WASM (zero server cost)
- **Multiplayer networking:** Peer-to-peer WebRTC
- **Signaling:** Deno Deploy free tier (stateless, <150 lines)
- **kipukas-turn:** Cloudflare free tier backup TURN server (public Cloudflare STUN first)
- **No database, no authentication, no paid services**

### Formatting & Linting

`deno fmt` and `deno lint` enforce consistent style on scripts and JavaScript assets. Run `deno task check` to verify both in a single command. Configuration lives in `deno.json` under `fmt` and `lint` keys.

---

## Full Stack Architecture

### Request Flow (Production)

```
User clicks a button with hx-get="/api/cards?page=0"
        │
        ▼
   HTMX makes a standard fetch()
        │
        ▼
   Service Worker intercepts /api/* requests
        │
        ▼
   SW sends message to page via client.postMessage()
        │
        ▼
   kipukas-api.js (page bridge) relays to Web Worker via MessageChannel
        │
        ▼
   kipukas-worker.js runs WASM: handle_request("GET", "/api/cards", "page=0")
        │
        ▼
   Rust router (matchit) dispatches to handler → returns HTML string
        │
        ▼
   Response travels back: Worker → MessageChannel → SW → fetch Response
        │
        ▼
   HTMX swaps the HTML fragment into the DOM
```

### Request Flow (Development / First Load)

When the service worker isn't active yet, a fallback path kicks in:

```
HTMX fires htmx:beforeRequest event
        │
        ▼
   kipukas-api.js intercepts, routes directly to Web Worker
        │
        ▼
   Worker runs WASM, returns HTML
        │
        ▼
   kipukas-api.js swaps HTML into the target element
```

**Why the dual path matters:** The dev fallback via `htmx:beforeRequest` is essential, not optional. Without it, nothing works on first page load (before the SW installs) or during `jekyll serve` development.

### Multiplayer Architecture

```
Player A's Browser                     Player B's Browser
┌─────────────────────-─┐              ┌──────────────────────-┐
│  HTMX ←→ SW ←→ WASM   │              │  HTMX ←→ SW ←→ WASM   │
│  (local game server)  │              │  (local game server)  │
│          │            │              │            │          │
│   WebRTC Data Channel  ←──────────→  WebRTC Data Channel     │
└──────────┼────────────┘              └────────────┼──────────┘
           │                                        │
           └──────── Signaling Server ──────────────┘
                    (WebSocket, stateless)
```

The signaling server handles **only** connection brokering: room creation, SDP offer/answer relay, ICE candidate exchange, and player presence. It never touches game state or logic.

### Key Files

| File | Role |
|------|------|
| `kipukas-server/src/lib.rs` | WASM entry point + route registration |
| `kipukas-server/src/routes/*.rs` | Route handlers (type matchup, QR, cards, game, room) |
| `kipukas-server/src/game/*.rs` | Game state, damage tracking, turns, room/combat state |
| `kipukas-server/src/cards_generated.rs` | Auto-generated card catalog (do not edit) |
| `assets/js/kipukas-api.js` | Page bridge — SW relay + dev fallback + state persistence |
| `assets/js/kipukas-worker.js` | Web Worker — loads WASM + ZXing, handles requests |
| `assets/js/kipukas-multiplayer.js` | WebRTC peer connection + data channel protocol |
| `assets/js/qr-camera.js` | Camera + ZXing QR scan loop |
| `sw-src.js` | Service worker source (Workbox injectManifest) |
| `signaling-server/main.ts` | WebSocket signaling relay for WebRTC |
| `scripts/build-card-catalog.ts` | Extracts card YAML → Rust source |
| `scripts/bundle-alpine.ts` | Bundles Alpine.js + plugins via esbuild |
| `.tmuxinator.yml` | Multi-pane dev environment config |

---

## Proven Patterns

These are patterns that have been tested and work well. Understanding **why** they work prevents future regressions.

### Pattern 1: HTMX + WASM Bridge (the core loop)

**The pattern:** HTMX makes standard `fetch()` calls. The SW intercepts `/api/*` requests, relays them through the page bridge to the Web Worker, which runs WASM and returns an HTML string. HTMX swaps the fragment into the DOM.

**Why it works:** HTMX is transport-agnostic — it just makes HTTP requests and swaps HTML. It doesn't know (or care) that the "server" is WASM running in the same browser. This means every HTMX feature (triggers, swaps, targets, polling) works unmodified with our architecture.

**Key constraint:** HTMX attributes in WASM-returned HTML (`hx-get`, `hx-post`) fire real network fetches that go through the SW relay path. For dynamic content, prefer `onclick` + `htmx.ajax()` calls — these go through the same direct JS path regardless of SW state.

### Pattern 2: Sentinel Div for Hidden State (Final Blows)

**The pattern:** WASM renders a hidden sentinel div (`<div class="keal-all-checked hidden">`) when all keal means checkboxes are checked. Alpine's `x-effect` on the parent watches for this sentinel after each HTMX swap and toggles a CSS class (`.show-final-blows`) that makes the `.final-blows-section` visible.

**Why it works:** The Final Blows section is **always in the DOM** (rendered by WASM regardless of state). Visibility is controlled purely by CSS classes toggled by Alpine. This avoids browser reflow/repaint issues with conditional `innerHTML` swaps — the DOM structure never changes, only CSS classes toggle. The sentinel acts as a bridge between WASM state and Alpine reactivity: WASM decides the state, Alpine handles the visual transition.

**Why alternatives failed:** Conditionally including/excluding the Final Blows HTML from the WASM response caused cross-browser rendering bugs — some browsers wouldn't repaint after the innerHTML swap if the DOM structure changed too dramatically. The sentinel + always-present-DOM pattern is rock-solid.

### Pattern 3: Alpine × HTMX Coexistence

**The pattern:** Alpine manages UI chrome (modals, toggles, visibility). HTMX manages data (fetching, computing, displaying). They communicate via:

1. **Alpine → HTMX:** `htmx.ajax()` calls from Alpine event handlers (e.g., `@click="htmx.ajax('POST', ...)"`)
2. **HTMX → Alpine:** `x-effect` watching for DOM changes after HTMX swaps (sentinel pattern)
3. **Cross-component:** Custom DOM events (e.g., `document.dispatchEvent(new CustomEvent('close-multiplayer'))` listened by `@close-multiplayer.window="showMultiplayer = false"`)

**The bridge function:** `kipukasRefreshCards()` reads Alpine's reactive `filter` and `searchQuery` state, builds a URL, and calls `htmx.ajax()`. This bridges Alpine UI state to HTMX data fetching without coupling them.

**When to use Alpine:** show/hide toggles, CSS class switching, animations, modal open/close — anything purely visual with no data dependencies.

**When to use HTMX + WASM:** data computation, state management, anything that touches game logic or needs cross-player sync.

### Pattern 4: x-effect for Modal Refresh

**The pattern:** The multiplayer modal uses `x-effect` to re-fetch both `#room-status` and `#fists-container` from WASM every time the modal opens:

```html
x-effect="if (showMultiplayer) $nextTick(() => {
  htmx.ajax('GET', '/api/room/status', {target:'#room-status', swap:'innerHTML'});
  var fc = document.getElementById('fists-container');
  if (fc) htmx.ajax('GET', fc.getAttribute('hx-get'), {target:'#fists-container', swap:'innerHTML'});
})"
```

**Why it works:** `hx-trigger="load"` only fires on initial page load. When a user modifies state (e.g., marks damage) and reopens the modal, the stale HTML would be shown. The `x-effect` ensures fresh data every time the modal becomes visible.

**Why not just `hx-trigger="revealed"`?** The `revealed` trigger fires when an element enters the viewport via scrolling, not when it becomes visible via Alpine's `x-show`. Custom events or `x-effect` are the correct Alpine → HTMX bridge for modal visibility.

### Pattern 5: WASM State → DOM Sync (refreshKealTracker)

**The pattern:** After WASM auto-marks damage (e.g., from combat outcome), the keal damage tracker checkboxes on the card page are stale. A JavaScript helper finds the tracker element and re-fetches:

```javascript
function refreshKealTracker() {
  const tracker = document.querySelector('[id^="keal-damage-"]');
  if (tracker) {
    const slug = tracker.id.replace('keal-damage-', '');
    htmx.ajax('GET', '/api/game/damage?card=' + slug,
      { target: '#' + tracker.id, swap: 'innerHTML' });
  }
}
```

**Why it works:** The WASM state is authoritative. When state changes programmatically (not from a user click), the DOM must be explicitly refreshed. A small `setTimeout(refreshKealTracker, 150)` delay ensures the WASM worker has finished processing before the refresh request arrives.

**Why inline scripts failed:** Embedding `<script>htmx.ajax(...)</script>` in WASM responses is fragile — `execScripts()` runs the script, but timing with the WASM worker is unpredictable. Explicit JS calls from the callback chain are more reliable.

### Pattern 6: postToWasmWithCallback (Direct Worker Messaging)

**The pattern:** For multiplayer interactions that need immediate response handling (not just DOM swapping), bypass HTMX and talk directly to the Web Worker:

```javascript
function postToWasmWithCallback(method, path, body, callback) {
  const channel = new MessageChannel();
  channel.port1.onmessage = (msg) => callback(msg.data.html);
  globalThis.kipukasWorker.postMessage(
    { method, pathname: path, search: '', body },
    [channel.port2],
  );
}
```

**Why it works:** HTMX swaps are great for simple GET/POST → innerHTML patterns. But multiplayer needs to: (1) POST to WASM, (2) read the response, (3) update multiple DOM targets, (4) send data to a WebRTC peer, (5) trigger side effects. The callback pattern gives full control over the response.

**Fire-and-forget variant:** `postToWasm()` (no callback) is used for state updates where we don't need the response (e.g., `POST /api/room/create`).

### Pattern 7: Inline Script Re-execution (execScripts)

**The pattern:** After `innerHTML` swap, `<script>` tags in the new HTML are inert (browser security). Clone and replace them:

```javascript
function execScripts(el) {
  el.querySelectorAll('script').forEach((old) => {
    const s = document.createElement('script');
    s.textContent = old.textContent;
    old.parentNode.replaceChild(s, old);
  });
}
```

**Why it's needed:** Both HTMX swap and direct `innerHTML` assignment produce inert scripts. This is used by the QR scanner, multiplayer module, and dev fallback. The pattern is simple but essential — without it, WASM-returned HTML that includes `<script>` (e.g., for WebRTC data channel sends) silently fails.

### Pattern 8: thread_local! + RefCell for WASM State

**The pattern:** WASM state uses `thread_local!` + `RefCell` for safe mutable globals:

```rust
thread_local! {
    static STATE: RefCell<GameState> = RefCell::new(GameState::default());
}
pub fn with_state<F, R>(f: F) -> R where F: FnOnce(&GameState) -> R {
    STATE.with(|s| f(&s.borrow()))
}
pub fn with_state_mut<F, R>(f: F) -> R where F: FnOnce(&mut GameState) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}
```

**Why it works:** The WASM module runs in a single Web Worker thread. `thread_local!` provides safe global state without `unsafe`. The `RefCell` borrow checker prevents concurrent access at runtime, though in practice the single-threaded worker never triggers it. Room state and game state use **separate** `thread_local!` stores — room is global (synced), game is local (private).

### Pattern 9: WebRTC Data Channel Protocol

**The pattern:** Peers exchange JSON messages over the WebRTC data channel. Each message has a `type` field:

| Message Type | Direction | Payload | Purpose |
|-------------|-----------|---------|---------|
| `fists_submission` | Both → peer | `{ data: FistsSubmission }` | Sync combat choice |
| `fists_reset` | Both → peer | (none) | Reset for next round |
| `fists_outcome` | Both → peer | `{ attacker_won: bool }` | Sync "Did you win?" result |

**Why JSON over binary:** With 56 cards and simple turn-based interactions, message frequency is ~1-2 per combat round. JSON is human-readable for debugging and trivially parsed. Binary would add complexity for negligible performance gain.

**Outcome sync pattern:** When a player answers "Did you win?", the JS derives `attacker_won` from the local role + answer, sends it to the peer, and both sides independently process the outcome via `POST /api/room/fists/outcome`. The defender's WASM auto-marks damage on their local card. Each side sees a role-appropriate message.

### Pattern 10: Session Persistence via sessionStorage

**The pattern:** Room connection info is saved to `sessionStorage` on create/join and restored on page load for auto-reconnect:

```javascript
function saveSession() {
  sessionStorage.setItem(SESSION_KEY, JSON.stringify({ code, name, creator }));
}
// On page load:
async function autoReconnect() {
  const session = loadSession();
  if (!session) return;
  // Rejoin signaling server, re-establish WebRTC
}
```

**Why sessionStorage (not localStorage):** Room connections are ephemeral — they should survive page navigation within a session but not persist across browser restarts. `sessionStorage` provides exactly this lifecycle. Game state (damage, turns) uses `localStorage` for cross-session persistence.

---

## Technology Stack & Licenses

### Frontend

| Technology | Version | License | Purpose |
|---|---|---|---|
| [Jekyll](https://jekyllrb.com/) | ~4.3.4 | MIT | Static site generator |
| [Tailwind CSS](https://tailwindcss.com/) | v4 | MIT | Utility-first CSS framework |
| [Alpine.js](https://alpinejs.dev/) | 3.14.9 | MIT | Lightweight UI reactivity (modals, toggles, filters) |
| [HTMX](https://htmx.org/) | 2.0.4 | BSD 2-Clause | HTML-over-the-wire data fetching |
| [Workbox](https://developer.chrome.com/docs/workbox) | 7.3.0 | MIT | Service worker tooling / PWA caching |

**Alpine.js plugins bundled:** persist, intersect, focus, anchor, collapse

### WASM Layer

| Technology | Version | License | Purpose |
|---|---|---|---|
| [Rust](https://www.rust-lang.org/) | Edition 2024 | MIT / Apache-2.0 | Game logic, routing, type safety |
| [wasm-bindgen](https://rustwasm.github.io/wasm-bindgen/) | 0.2 | MIT / Apache-2.0 | Rust ↔ JavaScript interop |
| [matchit](https://crates.io/crates/matchit) | 0.8 | MIT | Radix-tree URL router (same engine as Axum) |
| [serde](https://serde.rs/) + serde_json | 1.x | MIT / Apache-2.0 | State serialization (localStorage + WebRTC) |

### Server & Runtime

| Technology | License | Purpose |
|---|---|---|
| [Deno](https://deno.land/) | MIT | Task runner, build scripts, signaling server runtime |
| [Ruby](https://www.ruby-lang.org/) + Bundler | BSD 2-Clause / MIT | Jekyll runtime |

### Build Tooling

| Tool | License | Purpose |
|---|---|---|
| [esbuild](https://esbuild.github.io/) | MIT | Bundle Alpine.js + plugins into single minified file |
| [wasm-pack](https://rustwasm.github.io/wasm-pack/) | MIT / Apache-2.0 | Compile Rust crate → WASM package |
| [Workbox CLI](https://developer.chrome.com/docs/workbox/modules/workbox-cli) | MIT | Generate precache manifest into service worker |
| [@tailwindcss/cli](https://tailwindcss.com/) | MIT | Compile Tailwind CSS |
| [tmuxinator](https://github.com/tmuxinator/tmuxinator) | MIT | Multi-pane terminal dev environment |

### Jekyll Plugins

| Plugin | License | Purpose |
|---|---|---|
| jekyll-paginate | MIT | Pagination |
| jekyll-seo-tag | MIT | SEO meta tags |
| jekyll-sitemap | MIT | Sitemap generation |
| jekyll-redirect-from | MIT | URL redirects |
| jekyll-feed | MIT | Atom feed |

---

## Development Workflow

### Prerequisites

| Tool | Install |
|------|---------|
| **Ruby** (3.x) + Bundler | `gem install bundler` |
| **Deno** | [deno.land](https://deno.land/) |
| **Rust** + wasm-pack | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` then `cargo install wasm-pack` |
| **tmuxinator** (optional) | `gem install tmuxinator` |

### Initial Setup

```bash
# Install Ruby dependencies (Jekyll + plugins)
bundle install

# Run a full build to generate all artifacts
deno task build
```

### Daily Development

**Option A: tmuxinator (recommended)**

```bash
tmuxinator start kpksdev
```

This opens a tiled tmux layout with four panes:
1. `jekyll serve --host=0.0.0.0 --livereload --watch` — Local dev server with live reload
2. `jekyll build --watch` — Continuous Jekyll rebuild on file changes
3. `deno task dev:css` — Tailwind CSS watch mode
4. Opens Firefox at `http://localhost:4000`

**Option B: Manual**

```bash
# Terminal 1: Jekyll dev server
jekyll serve --host=0.0.0.0 --livereload --watch

# Terminal 2: Tailwind CSS watch
deno task dev:css

# Terminal 3 (if working on multiplayer): Signaling server
cd signaling-server && deno task dev
```

### Full Build Pipeline

The complete build runs in this order (executed by `deno task build`):

```
1. build:card-catalog  → Extract _posts YAML → Rust source (cards_generated.rs)
2. build:wasm          → wasm-pack compile Rust → assets/js-wasm/kipukas-server-pkg/
3. build:htmx          → Vendor HTMX → assets/js/htmx.min.js
4. build:css           → Tailwind compile + minify → assets/css/output.css
5. build:alpine        → esbuild bundle Alpine + plugins → assets/js/alpine.bundle.min.js
6. build:rules         → Build rules book subproject → game_rules/
7. jekyll build        → Generate _site/ from all sources
8. build:sw            → Workbox injectManifest → sw.js with precache manifest
```

Individual build tasks can be run separately:

```bash
deno task build:wasm       # Rebuild WASM only (includes card catalog)
deno task build:css        # Rebuild Tailwind CSS only
deno task build:alpine     # Rebuild Alpine.js bundle only
deno task build:sw         # Rebuild service worker only (after jekyll build)
```

### Testing

**Rust unit tests:**

```bash
cd kipukas-server && cargo test
```

Currently 114 tests covering: route dispatch, type matchup tables, QR URL validation, card filtering/pagination, damage tracking, turn management, room state, combat resolution, and outcome processing.

**Browser integration checks** (DevTools console):

| Message | Confirms |
|---------|----------|
| `[kipukas-worker] WASM server initialized` | Rust WASM loaded in Web Worker |
| `[kipukas-worker] ZXing WASM initialized` | QR decode capability ready |
| `[kipukas-api] No SW controller, routing directly:` | Dev fallback active (expected during `jekyll serve`) |
| `[qr-camera] Camera started, scanning at 2 fps` | Camera + scan loop running |
| `[multiplayer] Signaling connected` | WebSocket to signaling server open |
| `[multiplayer] Peer connected via WebRTC!` | Data channel established |

**Multiplayer testing:**
- Two browser tabs on same machine (WebRTC works locally)
- Two devices on same network
- Two devices on different networks (requires TURN — Cloudflare credentials fetched dynamically)

### Formatting & Linting

```bash
# Check formatting + lint (CI-friendly, no changes)
deno task check

# Auto-format
deno task fmt

# Lint only
deno task lint
```

Scope: `scripts/` and `assets/js/` (excluding vendored/generated files).

### Key Conventions

**Adding a new card:**
1. Create `_posts/YYYY-MM-DD-card_name.html` with YAML front matter
2. Run `deno task build:wasm` — the build script auto-generates the Rust card catalog

**Adding a new `/api/*` route:**
1. Create or extend a route handler in `kipukas-server/src/routes/`
2. Register the route in `kipukas-server/src/lib.rs` (the `matchit` router)
3. Add unit tests in the same file
4. Rebuild WASM: `deno task build:wasm`

**Alpine vs HTMX decision:**
- Use **Alpine** for: show/hide toggles, CSS class switching, animations, modal open/close
- Use **HTMX + WASM** for: data computation, state management, anything that touches game logic

**Jekyll exclusions:**
Non-Jekyll directories must be listed in `_config.yml` under `exclude:` to prevent Jekyll from processing them (especially `kipukas-server/target/` which contains thousands of Rust build files).

**Generated files (do not edit manually):**
- `kipukas-server/src/cards_generated.rs` — regenerated by `deno task build:card-catalog`
- `assets/js/alpine.bundle.min.js` — regenerated by `deno task build:alpine`
- `assets/js/htmx.min.js` — vendored by `deno task build:htmx`
- `sw.js` / `sw.js.map` — regenerated by `deno task build:sw`
- `assets/css/output.css` — regenerated by `deno task build:css`

---

## Phase History

A condensed record of architectural decisions and key lessons from each development phase. For the full narrative, see git history.

### Phase 1: Foundation (✅)

**Built:** Rust WASM crate with `matchit` router, type matchup engine ported from JS, SW + Web Worker sidecar bridge, HTMX integration.

**Key decisions:** Option C architecture (SW + Worker sidecar). Module Web Worker for ES imports. `matchit` router for Axum portability. Dual-path execution (SW relay + dev fallback).

**Lessons:** Jekyll processes everything — exclude `target/` and `node_modules/`. SW isn't available on first load — dev fallback is essential. HTMX + Alpine coexist cleanly. Module Workers need `{ type: 'module' }`.

### Phase 2: QR Scanner Migration (✅)

**Built:** Camera + ZXing WASM QR decoder in the shared Web Worker. HTMX-driven state machine replacing Alpine state (`showScanner`, `showFlash`, `videoReady`, `noCamera`, `showQRModal`).

**Key decisions:** Keep ZXing in the same Web Worker (loaded via `eval()` trick for classic scripts in module workers). All scanner state transitions driven by WASM-returned HTML fragments.

**Lessons:** `importScripts()` blocked in module workers — use `fetch()` + `eval()`. ZXing needs `locateFile` when loaded via eval. wasm-pack generates `.gitignore` with `*` — auto-delete it in build. HTMX attributes in dynamic HTML bypass WASM pipeline — use `onclick` + `htmx.ajax()`. `innerHTML` doesn't execute `<script>` tags — need `execScripts()`.

### Phase 3a: Card Grid Infinite Scroll (✅)

**Built:** Build-time card catalog generation (Deno → Rust source). Paginated, filtered card route. HTMX sentinel-based infinite scroll. Native `srcset` replacing Alpine responsive logic.

**Key decisions:** Card metadata compiled into WASM binary (~5KB for 56 cards). Sentinel div with `hx-trigger="revealed"` for native infinite scroll. `kipukasRefreshCards()` as Alpine → HTMX bridge.

**Alpine state removed:** 56 `inView*` variables, ~170 reactive bindings, per-card `x-data` for responsive images. DOM elements on load: ~110+ → ~24.

### Phase 3b: Game State Migration (✅)

**Built:** `GameState` with per-card damage tracking, turn/alarm system, state persistence (localStorage). `thread_local! + RefCell` for safe WASM globals. POST method support. Alpine `$persist` migration script.

**Key decisions:** `serde_json` added (+111KB WASM) as strategic investment for both localStorage and future WebRTC sync. Custom `Default` needed for non-zero defaults (`show_alarms: true`). State persistence via `beforeunload` + restore on load.

**Alpine state removed:** `clearDamage: $persist(...)`, per-card `$persist({...})` damage state, `alarms: $persist([])`, `showAlarms: $persist(true)`. Moved `showKealModal` to local scope.

### Phase 4: WebRTC Multiplayer + Fists Combat

**Built:** Signaling server (Deno, ~130 lines). WebRTC peer connection with ICE (STUN + Cloudflare TURN). Data channel protocol. Room state module (`RoomState` separate from `GameState`). Fists combat tool: role selection, keal means picker, archetype matchup computation, die modifier display. "Did you win?" outcome flow with auto-damage marking. Session persistence for cross-page navigation.

**Key decisions:** Local User vs Global (Room) state separation. Signaling server is stateless — only brokers connections. Mutual trust sufficient for friend-vs-friend card game. `sessionStorage` for room session (ephemeral), `localStorage` for game state (persistent). TURN credentials fetched dynamically from signaling server (proxies Cloudflare API).

**Lessons:** Sentinel + always-present-DOM pattern is cross-browser reliable for conditional UI (final blows). `x-effect` is the correct trigger for HTMX refresh on Alpine modal open. Explicit JS `refreshKealTracker()` is more reliable than inline scripts for cross-component DOM sync. Custom DOM events (`close-multiplayer`) bridge WASM-rendered HTML to Alpine state.

**WASM binary size progression:** 69KB (Phase 1) → 72KB (3a) → 183KB (3b, +serde) → ~185KB (Phase 4).

---

## Desired Next Features

Features are grouped by priority. Items marked *post-launch* require the game to be publicly available first.

### Near-Term

#### 1. Shared Turn Timer
Sync the diel cycle alarm system via WebRTC so both players see the same turn countdown. Multiple timers should be supported and visable for both players on both devices. If one player advances a turn the other players countdown SHOULD be advanced as well (mutual culpability + true to game intention). First candidate for expanding room scope beyond fists combat. Requires the room/fists separation (feature #1) to be clean.

#### 2. QR Room Join
Embed the room code in a QR code so scanning joins the room directly. This connects two existing features (QR scanner + multiplayer) with minimal new code. The flow: Player A creates a room → room code appears as both text and a QR. Player B scans the QR → auto-joins the room. The QR URL format could be `kpks.us/join?code=ABCD#room=myroom` with a redirect that passes the code to the multiplayer module.
### Medium-Term

#### 3. Replace ZXing with Rust QR Decoder
Eliminate the ~2MB third-party ZXing WASM dependency by compiling a Rust QR decoder into `kipukas-server`. **Caveat:** This has been explored. `rxing` (Rust port of ZXing) produces a ~6MB WASM binary — too large. `rqrr` is small but struggles with Kipukas' anti-cheat camouflaged QR codes, which require robust error correction and perspective distortion handling. This feature is blocked until either `rxing` becomes smaller/more WASM-friendly or `rqrr` improves its decoding of difficult QR patterns. When feature discussions come up ask to check on state of the libs (robustness to detection is the primary concern).

#### 4. WebSocket Relay Migration (Replace WebRTC)
**Replaces:** The WebRTC peer-to-peer data channel with a WebSocket message relay through the signaling server.

**The Problem with WebRTC:**

Current multiplayer uses WebRTC for peer-to-peer game state sync after the signaling server brokers the initial connection. This architecture is elegant in theory but carries significant operational burden:

| Complexity | Impact |
|------------|--------|
| ICE negotiation | 3-5 round trips before gameplay starts |
| STUN servers | 2 external dependencies (Cloudflare + Google) |
| TURN servers | Cloudflare API integration, credential proxying, ~50 lines of server code |
| SDP offer/answer | Fragile timing, complex error handling |
| Connection resilience | Mobile browser sleep often kills WebRTC; reconnection requires full re-signaling |

For a turn-based card game with ~1-2 messages per combat round, this is massive over-engineering. The signaling server is already a hard dependency for room creation. The theoretical benefit ("server can go offline after connection") rarely materializes in practice due to mobile browser lifecycle issues.

**The WebSocket Relay Solution:**

Replace the WebRTC data channel with direct message relay through the signaling server's WebSocket connection:

```
Current:  Player A ←WebRTC→ Player B  (with STUN/TURN/ICE overhead)
Proposed: Player A ←WebSocket→ Signaling Server ←WebSocket→ Player B
```

**Why This Tradeoff Is Right for Kipukas:**

| Factor | WebRTC (Current) | WebSocket Relay (Proposed) |
|--------|------------------|---------------------------|
| **Connection setup** | 8+ round trips (ICE + SDP) | 2 round trips (already connected) |
| **Firewall traversal** | Needs TURN for symmetric NATs | Works everywhere (HTTPS) |
| **External dependencies** | Signaling + 2 STUN + 1 TURN | Signaling server only |
| **Code complexity** | ~250 lines (peer connection, ICE, SDP handling) | ~50 lines |
| **Mobile reliability** | Poor (sleep kills connections) | Excellent (auto-reconnect) |
| **Latency** | P2P (lower) | Server hop (negligible for turn-based) |
| **Server load** | None after connection | ~100 bytes/minute per room |

**Server Load Analysis:**

A typical fists combat round generates ~4 messages (submission × 2, outcome × 2) at ~200 bytes each. With 1000 concurrent games:
- Messages per second: 1000 × 4 / 60 = ~67 msg/s
- Bandwidth: 67 × 200 × 2 (relay) = ~27KB/s
- Deno Deploy free tier: 100K req/day, 1GB egress — sufficient for ~100K combat rounds/day

The signaling server remains stateless. It forwards messages without parsing them. Game logic stays 100% client-side in WASM.

**HTMX WebSocket Extension Integration:**

The migration enables use of `htmx-ext-ws` for declarative multiplayer UI:

```html
<div hx-ext="ws" ws-connect="wss://signal.kipukas.deno.net/ws">
  <div id="fists-container" hx-swap-oob="true">
    <!-- WASM-generated combat UI -->
  </div>
  <form ws-send>
    <input name="fists_submission" type="hidden">
  </form>
</div>
```

The extension handles:
- Auto-reconnection with exponential backoff
- Message queuing during disconnection
- HTML fragment swapping via Out-of-Band swaps
- Event hooks for WASM integration (`htmx:wsBeforeMessage`)

**Implementation Phases:**

**Phase 1: Server simplification**
- Add `relay` message type to forward game messages between room peers
- Remove `sdp_offer`, `sdp_answer`, `ice_candidate` handlers
- Remove `/turn-credentials` endpoint
- Remove Cloudflare TURN API integration

**Phase 2: Client refactor**
- Vendor `htmx-ext-ws` (similar to `htmx.min.js`)
- Replace WebRTC peer connection with WebSocket message relay
- Route game messages (fists submissions, outcomes) through signaling WS
- Delete `setupPeerConnection`, `handleSdpOffer`, `handleSdpAnswer`, `handleIceCandidate`, `cleanupPeer`

**Phase 3: HTMX integration**
- Use `ws-connect` for declarative WebSocket management
- Use `ws-send` for form submissions
- Intercept `htmx:wsBeforeMessage` to route through WASM for HTML generation

**Expected Code Reduction:**

| File | Before | After | Delta |
|------|--------|-------|-------|
| `signaling-server/main.ts` | ~180 lines | ~100 lines | -80 lines |
| `kipukas-multiplayer.js` | ~320 lines | ~80 lines | -240 lines |
| **Total** | ~500 lines | ~180 lines | **-320 lines** |

**Why Not Keep WebRTC?**

WebRTC's benefits (P2P, server bypass, lower latency) matter for:
- Video/audio streaming (bandwidth)
- Real-time games (FPS, RTS)
- Privacy-critical applications

Kipukas is none of these. It's a turn-based card game between friends. The reliability gains and code reduction outweigh the theoretical benefits of P2P. The only better solution arcitecturally is peer to peer websockets connecting the individual client WASM binaries directly after assistance establishing the connection using the signaling server. Due to browser restrictions, this is currently impossible. Double check before implementing that this is still the case.

**Why Not Use Puter.js or Similar?**
Basically, puter.js is just a proxy with fairly misleading marketing. rusttls-wasm is cool though.

#### 5. Decentralized Identity & Authentication (Yrs Foundation)
**Prerequisite for:** Deck Builder (feature #6), Affinity/Loyalty tracking (feature #8), and cross-device sync.

Implement a serverless identity system using **y-crdt (yrs)** CRDT library and local keypairs. This provides the foundation for persistent player state without requiring a backend database or traditional authentication servers.

**Architecture Overview:**

```
┌─────────────────────────────────────────────────────────────────┐
│  Identity Layer (Local-First)                                   │
├─────────────────────────────────────────────────────────────────┤
│  • Ed25519 keypair generated in WASM (ed25519-dalek)            │
│  • Private key encrypted in localStorage                        │
│  • Public key = "Account ID" for cross-device recognition       │
│  • yrs Document for CRDT-based state (decks, counters, history) │
└─────────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
        ┌─────────┐     ┌──────────┐     ┌──────────┐
        │ Storage │     │   Sync   │     │  Backup  │
        │localStorage   │ WebRTC   │     │ Optional │
        │IndexedDB      │ yrs sync │     │ Passkeys │
        └─────────┘     └──────────┘     └──────────┘
```

**Implementation Phases:**

**Phase 5a: yrs Integration**
- Add `yrs` crate to `kipukas-server` (~400-800KB WASM with optimizations)
- Create `IdentityState` module alongside existing `GameState` and `RoomState`
- Implement yrs document with `YMap`/`YArray` types for structured data
- Add `/api/identity/*` routes for keypair generation and document access

**Phase 5b: Local Keypair Identity**
- Generate Ed25519 keypair on first app launch
- Store private key in localStorage (AES-encrypted with device-derived key)
- Display public key as user "ID" (shortened hash for readability)
- Add identity export/import (QR code for key backup)

**Phase 5c: yrs-Based State Containers**
Replace simple `serde_json` state with yrs documents for:
- **Decks**: `YMap<deck_name, YArray<card_slug>>`
- **Combat History**: `YArray<CombatRecord>`
- **Counters**: `YMap<card_slug, loyalty_count>`, `YMap<archetype, affinity_count>`
- Persistence via yrs update events → localStorage/IndexedDB

**Phase 5d: Cross-Device Sync**
- yrs sync protocol over WebRTC data channel (reuses existing P2P infrastructure)
- Device pairing via QR code exchange of public keys
- Automatic conflict resolution via CRDT merge semantics

**Why This Approach:**

| Requirement | Solution | Benefit |
|-------------|----------|---------|
| No backend server | Local keypair + yrs | Zero infrastructure cost |
| Cross-device identity | Public key recognition | Portable without passwords |
| Offline-first | yrs CRDT documents | Works without network |
| Conflict resolution | Automatic CRDT merge | No "last write wins" data loss |
| Future cloud backup | Passkeys encrypt backup key | Optional, no lock-in |

**Technical Considerations:**
- **WASM Size**: yrs adds ~300-400KB (vs. 828KB for Automerge WASM). Total WASM: ~500-600KB.
- **Storage**: yrs binary format is compact; localStorage 5MB limit sufficient for card game data.
- **Security**: Private key never leaves device unencrypted; cross-device sync uses authenticated encryption.

#### 6. Deck Builder / Hand Management
**Requires:** Decentralized Identity & Authentication (feature #5) for persistent deck storage.

Allow players to compose multiple named decks (e.g., "Main Deck", "Dragon Rush") and cycle through cards during a match without page navigation.

**Key Features:**
- **Deck Composer**: Add/remove cards from the catalog grid; visual deck list sidebar
- **Deck Switcher**: Select active deck from toolbar; persists across sessions
- **Hand Cycling**: During fists combat, quickly switch between cards in the active deck
- **Deck Limits**: Enforce deck size constraints (e.g., 30 cards + 1 personal effect)

**Technical Implementation:**
- New WASM routes: `/api/deck/list`, `/api/deck/create`, `/api/deck/update`, `/api/deck/delete`, `/api/deck/active`
- UI Components: deck sidebar in card grid, deck selector in toolbar, card "add to deck" buttons
- State stored in yrs `YMap` keyed by deck name; active deck reference in separate yrs root type

#### 7. Combat History Log
Persist combat results in yrs document so players can review past rounds across sessions. Each outcome (attacker, defender, keal means used, modifier, who won) stored as a `CombatRecord` in a `YArray`.

**UI**: Scrollable log modal accessible from toolbar, filterable by date range or opponent (if identity known).

**Technical**: Append-only `YArray` in yrs document; automatic synchronization if cross-device sync enabled.

#### 8. Affinity & Loyalty Tracking *(post-Yrs)*
**Requires:** Decentralized Identity & Authentication (feature #6) with yrs document infrastructure.

Implement long-term gameplay progression as described in the game rules: affinity with archetypes and loyalty with individual soul cards.

**Core Mechanics:**
- **Affinity**: Increases when declaring archetypes at match start
- **Loyalty**: Increases per play of a specific soul card (once per day)
- **Taming Threshold**: When loyalty + affinity + play bonuses exceed a card's tameability, the card becomes "tamed" (unlocking special abilities)

**Implementation:**
- `YMap<archetype, counter>` for affinity tracking
- `YMap<card_slug, LoyaltyRecord>` with fields: `plays_today`, `total_plays`, `incubation_bonus`
- Daily reset via local timestamp comparison (no server required)
- Tameability section added to all species cards

**Progression Visibility**: Profile modal showing affinity levels (visual bars) and loyalty milestones (badges/frames on cards).

### Long-Term

#### 9. Infinite Scroll with Content-Visibility
Replace the sentinel-chain pagination on the index page with a true rolling infinite scrolling system including position tracking and DOM replacements. Card count need to be around 150 to consider the feature.

#### 10. Card Trading
Propose an NFT brokered trade of cards marked in deck. Requires the game to be publicly available with a real player base to validate the mechanic. Also, requires the store website to be online (kipukas.com).

#### 11. Spectator Mode
Allow a third peer to observe a match via a read-only data channel. Architecturally simple (receive-only data channel, no submissions) but requires rooms to support >2 peers and the signaling server to handle multi-peer SDP negotiation. Low priority until competitive, streaming, or particularily compelling (active, visual, and exciting) use cases emerge.

#### 12. Provide Kippa Tools
Expand Kippa's understanding of the game by allowing it to assist users in using site features, gathering specific card data, and resolving issues.
