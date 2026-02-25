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

Game logic runs **100% client-side** in WebAssembly. There is no backend server processing game state. The only hosted component is a stateless WebSocket relay server (~120 lines) that forwards game messages between peers. The server never inspects message payloads — it simply relays them to the other player in the room.

### HTMX Over SPA Frameworks

Instead of React, Vue, or Svelte, the project uses **HTMX** to add dynamic behavior to server-rendered HTML. The "server" happens to be a Rust WASM module running in a Web Worker inside the browser — but HTMX doesn't know or care. This fits naturally with Jekyll's static HTML model: just add `hx-*` attributes to existing markup.

### Alpine.js + HTMX: Highly flexible and browser friendly

Alpine.js and HTMX coexist throughout the codebase. The guiding principle:

| Layer | Technology | Examples |
|-------|-----------|----------|
| **UI chrome** (visual-only) | Alpine.js | Modal open/close, hamburger menu, visibility toggles, animations |
| **Data & logic** | HTMX + WASM | Card filtering, damage tracking, type matchups, combat resolution |

A feature migrates from Alpine to HTMX when it involves data processing, complex state machines, or cross-player synchronization. A feature stays in Alpine when it's purely visual and can rely on side effect data reflected from WASM state.

### Type Safety via Rust

Game logic has been ported from JavaScript to Rust, compiled to WASM. The Rust type system catches bugs at compile time that JavaScript hides. The crate currently has **over 100 unit tests** covering route handlers, game logic, matchup tables, combat outcomes, and edge cases.

### Build-Time Code Generation

Card metadata is extracted from Jekyll `_posts/*.html` YAML front matter at build time by a Deno script (`scripts/build-card-catalog.ts`). This generates a Rust source file (`kipukas-server/src/cards_generated.rs`) containing a static array of `Card` structs compiled directly into the WASM binary. No runtime data fetching, no JSON loading, no IndexedDB — just compiled-in data.

### Two-Scope State Model

All data is tracked in two distinct scopes:

| Scope | Storage | Synced via WebSocket relay? | Examples |
|-------|---------|----------------------------|----------|
| **Local User** | WASM `GameState` + localStorage | No | Damage tracking, turn alarms, card browsing |
| **Global (Room)** | WASM `RoomState` + WebSocket relay | Yes | Fists combat submissions, combat results, outcome damage |

A feature defaults to local user state unless it explicitly requires cross-player visibility. Single-player behavior is completely unaffected by multiplayer code.

### Minimal Infrastructure

- **Hosting:** GitHub Pages (free, static)
- **Game logic:** In-browser WASM (zero server cost)
- **Multiplayer networking:** WebSocket relay through signaling server
- **Signaling/relay:** Deno Deploy free tier (stateless, ~120 lines)
- **No database, no authentication, no paid services**

### Formatting & Linting

`deno fmt` and `deno lint` enforce consistent style on scripts and JavaScript assets. Run `deno task check` to verify both in a single command. Configuration lives in `deno.json` under `fmt` and `lint` keys. --PLEASE RUN deno fmt and deno lint/ deno lint --fix FOR CODE QUALITY-- --ATTEMPT TO FIX LINTING ERRORS AS THEY ARE FOUND--

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
│     WebSocket ────────┼──────────────┼────── WebSocket       │
└──────────┼────────────┘              └────────────┼──────────┘
           │                                        │
           └──── Relay Server (stateless) ──────────┘
                 (forwards game messages)
```

The relay server handles room management (create/join/rejoin) and message forwarding. It never inspects game message payloads — game logic stays 100% client-side in WASM. Auto-reconnect with exponential backoff handles mobile browser sleep and network transitions.

### Key Files

| File | Role |
|------|------|
| `kipukas-server/src/lib.rs` | WASM entry point + route registration |
| `kipukas-server/src/routes/*.rs` | Route handlers (type matchup, QR, cards, game, room) |
| `kipukas-server/src/game/*.rs` | Game state, damage tracking, turns, room/combat state |
| `kipukas-server/src/cards_generated.rs` | Auto-generated card catalog (do not edit) |
| `assets/js/kipukas-api.js` | Page bridge — SW relay + dev fallback + state persistence |
| `assets/js/kipukas-worker.js` | Web Worker — loads WASM + ZXing, handles requests |
| `assets/js/kipukas-multiplayer.js` | WebSocket relay multiplayer manager + game message protocol |
| `assets/js/qr-camera.js` | Camera + ZXing QR scan loop |
| `sw-src.js` | Service worker source (Workbox injectManifest) |
| `signaling-server/main.ts` | WebSocket relay server — room management + message forwarding |
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

**Why it works:** HTMX swaps are great for simple GET/POST → innerHTML patterns. But multiplayer needs to: (1) POST to WASM, (2) read the response, (3) update multiple DOM targets, (4) send data to the peer via WebSocket relay, (5) trigger side effects. The callback pattern gives full control over the response.

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

**Why it's needed:** Both HTMX swap and direct `innerHTML` assignment produce inert scripts. This is used by the QR scanner, multiplayer module, and dev fallback. The pattern is simple but essential — without it, WASM-returned HTML that includes `<script>` (e.g., for multiplayer relay sends) silently fails.

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

### Pattern 9: WebSocket Relay Protocol

**The pattern:** Peers exchange JSON messages via the signaling server's WebSocket relay. Each game message is wrapped in `{ type: "relay", data: { type: "...", ... } }` for transport. The server forwards `relay` messages to the other peer without inspection. The inner `data.type` field determines how the client processes the message:

| Message Type | Direction | Payload | Purpose |
|-------------|-----------|---------|---------|
| `fists_submission` | Both → peer | `{ data: FistsSubmission }` | Sync combat choice |
| `fists_reset` | Both → peer | (none) | Reset for next round |
| `fists_outcome` | Both → peer | `{ attacker_won: bool }` | Sync "Did you win?" result |
| `final_blows_submission` | Both → peer | `{ data: FinalBlowsSubmission }` | Sync Final Blows choice |
| `yrs_sv` | Both → peer | `{ sv: base64 }` | Yrs CRDT state vector (sync handshake step 1) |
| `yrs_sv_reply` | Both → peer | `{ sv: base64 }` | Yrs CRDT state vector reply (sync handshake step 2) |
| `yrs_update` | Both → peer | `{ update: base64 }` | Yrs CRDT binary update (mutation broadcast) |

**Why JSON over binary:** With 56 cards and simple turn-based interactions, message frequency is ~1-2 per combat round. JSON is human-readable for debugging and trivially parsed. Binary would add complexity for negligible performance gain.

**Outcome sync pattern:** When a player answers "Did you win?", the JS derives `attacker_won` from the local role + answer, sends it to the peer via `sendToPeer()`, and both sides independently process the outcome via `POST /api/room/fists/outcome`. The defender's WASM auto-marks damage on their local card. Each side sees a role-appropriate message.

**Connection lifecycle:** The WebSocket connection to the signaling server handles both room management (create/join/rejoin) and game message relay. Auto-reconnect with exponential backoff (up to 8 attempts) handles mobile browser sleep, network transitions, and temporary server issues. A 5-minute grace period on the server preserves the room slot during page navigation, mobile sleep, and slow reconnections.

**Cross-page auto-reconnect:** `kipukas-multiplayer.js` is normally lazy-loaded when the user clicks the multiplayer button. To support seamless page navigation, `kipukas-api.js` checks `sessionStorage` for a saved room session on every page load. If found, it eagerly imports the multiplayer module, which triggers `autoReconnect()` → WebSocket connects → `rejoin` sent → both peers receive `peer_joined` → fists tool appears automatically.

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
  // Reconnect WebSocket and rejoin signaling server room
}
```

**Why sessionStorage (not localStorage):** Room connections are ephemeral — they should survive page navigation within a session but not persist across browser restarts. `sessionStorage` provides exactly this lifecycle. Game state (damage, turns) uses `localStorage` for cross-session persistence.

### Pattern 11: Self-Contained Tool Component (Alpine × HTMX × Tailwind × localStorage)

**The pattern:** Each interactive tool lives in a single `_includes/*.html` file that combines four layers: Alpine.js `x-data` for UI-only state, `@click`/`$watch`/`x-effect` for behavior, HTMX `hx-get` for WASM data fetching, Tailwind utilities for all styling, and the existing JSON → localStorage pipeline for persistence. The component is fully self-contained — no external CSS, no separate JS file, no global state leakage.

**Reference implementation:** `_includes/multiplayer_fists_tool.html` (also see `turn_tracker.html`, `local_fists_tool.html`).

**Layer breakdown:**

| Layer | Technology | Role in the component |
|-------|-----------|----------------------|
| **UI state** | Alpine `x-data` | Local booleans like `showFistsMenu`, `roomConnected` — purely visual, never persisted |
| **Behavior** | Alpine `@click`, `$watch`, `x-effect` | `@click` toggles visibility; `$watch` handles side effects; `x-effect` bridges to HTMX |
| **Data fetching** | HTMX `hx-get` + `hx-trigger` | Declarative WASM endpoint on the container div; `x-effect` re-fetches via `htmx.ajax()` on reopen |
| **Styling** | Tailwind utilities | Layout, theming, spacing, responsiveness — all inline classes, zero custom CSS |
| **Persistence** | JSON → localStorage (via kipukas-api.js) | WASM state auto-persists on `beforeunload`; component reads fresh state from WASM on each open |

**How the layers connect (annotated from `multiplayer_fists_tool.html`):**

```html
<!-- 1. Alpine x-data: declare ALL visual state locally -->
<div x-data="{ showFistsMenu: false, roomConnected: false }"
     class="relative place-content-center"

     <!-- 2. Window events: receive cross-component signals (from kipukas-multiplayer.js) -->
     @room-connected.window="roomConnected = true"
     @room-disconnected.window="roomConnected = false"

     <!-- 3. $watch: side effects when state changes -->
     x-init="
       $watch('roomConnected', value => { if (!value) showFistsMenu = false });
       $watch('showFistsMenu', value => { if (!value && window.kipukasMultiplayer) kipukasMultiplayer.resetFists(); });
     ">

    <!-- 4. @click: toggle visibility (Alpine handles show/hide) -->
    <button x-show="roomConnected" x-cloak>
        <svg @click="showFistsMenu = !showFistsMenu" ...>...</svg>
    </button>

    <!-- 5. Modal overlay: Tailwind utilities for layout + theming -->
    <div x-show="showFistsMenu" x-cloak
        class="fixed inset-0 flex items-center justify-center z-50"
        x-transition.duration.350ms

        <!-- 6. x-effect: bridge Alpine → HTMX. Re-fetch WASM data every time modal opens -->
        x-effect="if (showFistsMenu) $nextTick(() => {
            if (typeof htmx !== 'undefined') {
                var fc = document.getElementById('fists-container');
                if (fc) htmx.ajax('GET', fc.getAttribute('hx-get') || '/api/room/fists',
                    {target:'#fists-container', swap:'innerHTML'});
            }
        })">

        <!-- Backdrop: click-to-close -->
        <div class="absolute inset-0 bg-slate-300 opacity-75" @click="showFistsMenu = false"></div>

        <!-- Modal content: Tailwind for card-like appearance -->
        <div class="bg-amber-50 z-50 rounded-lg shadow-xl w-full max-w-sm max-h-[85vh] overflow-y-auto relative">

            <!-- 7. HTMX container: declares WASM endpoint, loads on page init -->
            <div id="fists-container"
              hx-get="/api/room/fists"
              hx-trigger="load"
              hx-swap="innerHTML">
            </div>

            <!-- 8. Close button: Alpine @click, Tailwind styling -->
            <button @click="showFistsMenu = false"
              class="w-full bg-slate-400 hover:bg-slate-500 text-amber-50 font-bold py-2 px-4 rounded text-sm">
              Close
            </button>
        </div>
    </div>
</div>
```

**Why it works:**

- **No global state pollution.** All UI state is scoped to the `x-data` block. External signals arrive via custom window events (`@room-connected.window`), not shared Alpine stores or global variables.
- **Fresh data on every open.** `hx-trigger="load"` handles the first page load. `x-effect` handles every subsequent open by calling `htmx.ajax()` — this avoids stale DOM from a previous session (see Pattern 4).
- **WASM is the source of truth.** The component never reads or writes localStorage directly. It asks WASM for current state via HTMX, and WASM's state is auto-persisted to localStorage as JSON by the `kipukas-api.js` bridge on `beforeunload`. On page load the bridge restores JSON → WASM before any component renders.
- **Single-file portability.** Because styling is Tailwind utilities, behavior is Alpine attributes, and data fetching is HTMX attributes, the entire component is one `_includes/*.html` partial with zero dependencies beyond the global Alpine/HTMX/Tailwind setup.

**Template for new tools (e.g., shared turn timer):**

```html
<!-- _includes/my_new_tool.html -->
<div x-data="{ showTool: false }"
     class="relative">

    <!-- Trigger button -->
    <button @click="showTool = !showTool" class="...tailwind classes...">
        <!-- icon SVG or text -->
    </button>

    <!-- Modal -->
    <div x-show="showTool" x-cloak
        class="fixed inset-0 flex items-center justify-center z-50"
        x-transition.duration.350ms
        x-effect="if (showTool) $nextTick(() => {
            if (typeof htmx !== 'undefined') {
                htmx.ajax('GET', '/api/your/endpoint',
                    {target:'#tool-container', swap:'innerHTML'});
            }
        })">

        <div class="absolute inset-0 bg-slate-300 opacity-75" @click="showTool = false"></div>

        <div class="bg-amber-50 z-50 rounded-lg shadow-xl w-full max-w-sm max-h-[85vh] overflow-y-auto relative">
            <!-- HTMX container — WASM renders the content -->
            <div id="tool-container"
              hx-get="/api/your/endpoint"
              hx-trigger="load"
              hx-swap="innerHTML">
            </div>

            <button @click="showTool = false"
              class="w-full bg-slate-400 hover:bg-slate-500 text-amber-50 font-bold py-2 px-4 rounded text-sm">
              Close
            </button>
        </div>
    </div>
</div>
```

Then add the corresponding WASM route in `kipukas-server/src/routes/`, register it in `lib.rs`, and return an HTML fragment. The component handles the rest.

**Key constraints:**
- **Alpine owns visibility, HTMX owns data.** Don't fetch data in Alpine (`fetch()` calls in `x-init`). Don't toggle visibility from HTMX responses. Keep the boundary clean.
- **Always include the `x-effect` re-fetch.** Without it, reopening the modal shows stale HTML from the previous HTMX swap (see Pattern 4 for why `hx-trigger="load"` alone is insufficient).
- **Use `$nextTick` in `x-effect`.** The DOM must be visible before `htmx.ajax()` fires, otherwise the target element may not be findable.
- **Prefer window events for cross-component signals.** If another module needs to tell your tool something (e.g., "room connected"), dispatch a `CustomEvent` on `window` and listen with `@my-event.window` in the `x-data` div. This avoids coupling between includes.

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
| [serde](https://serde.rs/) + serde_json | 1.x | MIT / Apache-2.0 | State serialization (localStorage + WebSocket relay) |
| [yrs](https://crates.io/crates/yrs) | 0.25 | MIT | Yjs CRDT port — conflict-free replicated data types for multiplayer sync |
| [base64](https://crates.io/crates/base64) | 0.22 | MIT / Apache-2.0 | Binary ↔ base64 encoding for yrs update transport |

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
| `[multiplayer] Peer connected via WebSocket relay!` | Game message relay established |

**Multiplayer testing:**
- Two browser tabs on same machine
- Two devices on same network
- Two devices on different networks (works everywhere — WebSocket relay traverses all firewalls/NATs)

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

**Key decisions:** `serde_json` added (+111KB WASM) as strategic investment for both localStorage and multiplayer sync. Custom `Default` needed for non-zero defaults (`show_alarms: true`). State persistence via `beforeunload` + restore on load.

**Alpine state removed:** `clearDamage: $persist(...)`, per-card `$persist({...})` damage state, `alarms: $persist([])`, `showAlarms: $persist(true)`. Moved `showKealModal` to local scope.

### Phase 4a: WebRTC Multiplayer + Fists Combat (✅)

**Built:** Signaling server (Deno, ~130 lines). WebRTC peer connection with ICE (STUN + Cloudflare TURN). Data channel protocol. Room state module (`RoomState` separate from `GameState`). Fists combat tool: role selection, keal means picker, archetype matchup computation, die modifier display. "Did you win?" outcome flow with auto-damage marking. Session persistence for cross-page navigation.

**Key decisions:** Local User vs Global (Room) state separation. Signaling server is stateless — only brokers connections. Mutual trust sufficient for friend-vs-friend card game. `sessionStorage` for room session (ephemeral), `localStorage` for game state (persistent). TURN credentials fetched dynamically from signaling server (proxies Cloudflare API).

**Lessons:** Sentinel + always-present-DOM pattern is cross-browser reliable for conditional UI (final blows). `x-effect` is the correct trigger for HTMX refresh on Alpine modal open. Explicit JS `refreshKealTracker()` is more reliable than inline scripts for cross-component DOM sync. Custom DOM events (`close-multiplayer`) bridge WASM-rendered HTML to Alpine state.

### Phase 4b: WebSocket Relay Migration (✅)

**Built:** Replaced WebRTC peer-to-peer data channel with WebSocket message relay through the signaling server. Eliminated all STUN/TURN/ICE/SDP complexity.

**Key decisions:** WebSocket relay is the right tradeoff for a turn-based card game. The signaling server already was a hard dependency for room creation — making it also relay game messages removes the entire WebRTC stack while improving reliability. Auto-reconnect with exponential backoff (8 attempts, 5-minute server-side grace period) handles page navigation, mobile browser sleep, and network transitions. `kipukas-api.js` eagerly loads the multiplayer module on any page when a `sessionStorage` room session exists, enabling seamless cross-page reconnection. The `sendToPeer()` pattern routes game messages through WASM before sending, which doesn't fit the `htmx-ext-ws` model of direct HTML-over-WebSocket — so the manual WebSocket approach is the correct choice for this architecture.

**Removed:** `RTCPeerConnection`, `RTCDataChannel`, ICE candidate handling, SDP offer/answer exchange, STUN server configuration, Cloudflare TURN API integration + credential proxying, `/turn-credentials` endpoint, `setupPeerConnection`, `handleSdpOffer`, `handleSdpAnswer`, `handleIceCandidate`, `cleanupPeer`.

**Lessons:** WebRTC is overkill for turn-based games with ~1-2 messages per combat round. The operational burden (STUN/TURN external deps, ICE negotiation fragility, mobile browser sleep killing connections) far outweighed the theoretical P2P benefits. WebSocket relay works everywhere, reconnects automatically, and reduced multiplayer code from ~500 lines to ~330 lines across server and client. The only architecturally superior solution — peer-to-peer WebSocket connections between browser WASM instances — remains impossible due to browser security restrictions (browsers cannot listen on sockets). Verify this constraint before future architecture changes.

**WASM binary size progression:** 69KB (Phase 1) → 72KB (3a) → 183KB (3b, +serde) → ~185KB (Phase 4a) → ~185KB (Phase 4b, no WASM changes).

### Phase 5: Yrs CRDT Integration + Shared Turn Timer (✅)

**Built:** `yrs` (Yjs Rust port) CRDT library integrated into the WASM crate. Multiplayer turn timer sync replaced with yrs-backed state that converges automatically via binary update exchange. New `crdt.rs` module with `thread_local!` yrs `Doc` alongside existing `GameState` and `RoomState`. Six new `/api/room/yrs/*` routes for CRDT sync operations (state vector exchange, diff computation, update application, alarm mutations). Multiplayer JS updated with 3-message yrs sync handshake (`yrs_sv` → `yrs_sv_reply` → `yrs_update`) triggered on peer connect/reconnect.

**Key decisions:** yrs chosen over Automerge (300-400KB vs 828KB WASM). Turn timer used as proof-of-concept — the yrs Doc structure (`"alarms"` ArrayRef of MapRef entries) is extensible for future features (decks, combat history, identity). Multiplayer `render_alarm_list(true)` reads from the yrs Doc; local `render_alarm_list(false)` continues reading from `GameState`. Base64 encoding for yrs binary updates over the JSON WebSocket relay — keeps the existing relay protocol intact without requiring binary WebSocket frames. CRDT Doc initialized on room create/join and reset on disconnect, matching the room lifecycle.

**Removed:** Old turn timer message-based sync (`turn_add`, `turn_tick`, `turn_remove` relay messages). These are replaced by `yrs_update` messages carrying the full CRDT binary diff, which handles concurrent edits, reconnection convergence, and deduplication automatically. Dead functions `merge_alarms()` and `export_alarms_json()` from `turns.rs`, along with `/api/room/turns/sync` and `/api/room/turns/export` routes from `room.rs` and `lib.rs`.

**Added:** `yrs` 0.25 crate, `base64` 0.22 crate, `kipukas-server/src/game/crdt.rs` module (19 unit tests covering add/tick/remove/sync/convergence/concurrent edits/seed/export). `seed_from_local()` copies local `GameState.alarms` into the CRDT Doc on room create/join so pre-existing timers become shared. `export_to_local()` copies CRDT alarms back to `GameState` on disconnect so shared timers survive as local timers. `refreshAlarms(multiplayer)` JS helper switches the alarm display between CRDT-backed (multiplayer) and GameState-backed (local) rendering on connect/disconnect.

**Lessons:** yrs `WriteTxn` trait must be imported explicitly for `get_or_insert_array` on `TransactionMut`. The state vector exchange handshake requires both directions — each peer computes the diff the other needs. Multiplayer alarm rendering must read from the CRDT Doc, not `GameState`, since mutations go through yrs routes. Existing tests that checked multiplayer alarm rendering needed updating to add alarms via `crdt::add_alarm()` instead of `turns::add_alarm()`. Seeding the CRDT Doc from local alarms on room create/join (not on peer connect) avoids race conditions with the sync handshake — the Doc has local timers before the first state vector exchange. The `yrs_update` handler in JS already refreshes the alarm display, so the sync handshake naturally updates both peers' UIs after convergence.

**WASM binary size progression:** 69KB (Phase 1) → 72KB (3a) → 183KB (3b, +serde) → ~185KB (Phase 4) → TBD (Phase 5, +yrs — expect ~500-600KB).

---

## Desired Next Features

Features are grouped by priority. Items marked *post-launch* require the game to be publicly available first.

### Near-Term

#### 1. QR Room Join
Embed the room code in a QR code so scanning joins the room directly. This connects two existing features (QR scanner + multiplayer) with minimal new code. The flow: Player A creates a room → room code appears as both text and a QR. Player B scans the QR → auto-joins the room. The QR URL format could be `kpks.us/join?code=ABCD#room=myroom` with a redirect that passes the code to the multiplayer module.

#### 2. Decentralized Identity & Authentication (Yrs Foundation)
**Prerequisite for:** Deck Builder (feature #5), Affinity/Loyalty tracking (feature #7), and cross-device sync.

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
        │localStorage   │ WS relay │     │ Optional │
        │IndexedDB      │ yrs sync │     │ Passkeys │
        └─────────┘     └──────────┘     └──────────┘
```

**Implementation Phases:**

**Phase 2a: yrs Integration**
- Add `yrs` crate to `kipukas-server` (~400-800KB WASM with optimizations)
- Create `IdentityState` module alongside existing `GameState` and `RoomState`
- Implement yrs document with `YMap`/`YArray` types for structured data
- Add `/api/identity/*` routes for keypair generation and document access

**Phase 2b: Local Keypair Identity**
- Generate Ed25519 keypair on first app launch
- Store private key in localStorage (AES-encrypted with device-derived key)
- Display public key as user "ID" (shortened hash for readability)
- Add identity export/import (QR code for key backup)

**Phase 2c: yrs-Based State Containers**
Replace simple `serde_json` state with yrs documents for:
- **Decks**: `YMap<deck_name, YArray<card_slug>>`
- **Combat History**: `YArray<CombatRecord>`
- **Counters**: `YMap<card_slug, loyalty_count>`, `YMap<archetype, affinity_count>`
- Persistence via yrs update events → localStorage/IndexedDB

**Phase 2d: Cross-Device Sync**
- yrs sync protocol over WebSocket relay (reuses existing multiplayer infrastructure)
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
- **Storage**: yrs binary format is compact; localStorage 5MB limit sufficient for card game data.
- **Security**: Private key never leaves device unencrypted; cross-device sync uses authenticated encryption.

#### 3. Deck Builder / Hand Management
**Requires:** Decentralized Identity & Authentication (feature #4) for persistent deck storage.

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

#### 4. Combat History Log
Persist combat results in yrs document so players can review past rounds across sessions. Each outcome (attacker, defender, keal means used, modifier, who won) stored as a `CombatRecord` in a `YArray`.

**UI**: Scrollable log modal accessible from toolbar, filterable by date range or opponent (if identity known).

**Technical**: Append-only `YArray` in yrs document; automatic synchronization if cross-device sync enabled.

#### 5. Affinity & Loyalty Tracking *(post-Yrs)*
**Requires:** Decentralized Identity & Authentication (feature #4) with yrs document infrastructure.

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

#### 6. Replace ZXing with Rust QR Decoder
Eliminate the ~2MB third-party ZXing WASM dependency by compiling a Rust QR decoder into `kipukas-server`. **Caveat:** This has been explored. `rxing` (Rust port of ZXing) produces a ~6MB WASM binary — too large. `rqrr` is small but struggles with Kipukas' anti-cheat camouflaged QR codes, which require robust error correction and perspective distortion handling. This feature is blocked until either `rxing` becomes smaller/more WASM-friendly or `rqrr` improves its decoding of difficult QR patterns. **NOTE:** It was also attempted to improve rqrr detection, but the results were overall worse that ZXing with greater complication and dimishing returns on space saved. Many strategies were attempted, but only adaptive_threshold had any effect. When feature discussions come up ask to check on state of the libs (robustness to detection is the primary concern).

#### 7. Infinite Scroll with Content-Visibility
Replace the sentinel-chain pagination on the index page with a true rolling infinite scrolling system including position tracking and DOM replacements. Card count need to be around 150 to consider the feature.

#### 8. Card Trading
Propose an NFT brokered trade of cards marked in deck. Requires the game to be publicly available with a real player base to validate the mechanic. Also, requires the store website to be online (kipukas.com).

#### 9. Spectator Mode
Allow a third peer to observe a match via a read-only WebSocket connection. Architecturally simple (receive-only relay, no submissions) but requires rooms to support >2 peers. Low priority until competitive, streaming, or particularly compelling (active, visual, and exciting) use cases emerge.

#### 10. Provide Kippa Tools
Expand Kippa's understanding of the game by allowing it to assist users in using site features, gathering specific card data, and resolving issues.
