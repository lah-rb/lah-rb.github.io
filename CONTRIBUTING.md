# Kipukas — Contributing Guide

## State of Project
This project is in purly alpha state. As such, keeping the code base as clean/lean and agile to adaptation as possible is crucial. When working on the project do not plan on user migration steps. We have no users yet and we would rather embrace new constructs fully than mitigate breakage.


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
8. [Planned Sprint: Player Document & GameState → Yrs Consolidation](#planned-sprint-player-document--gamestate--yrs-consolidation)

---

## Practices, Principles & Philosophies

### Offline-First / PWA-First

The site works without internet after the first load. Workbox **injectManifest** mode gives full control over the service worker lifecycle. Updates use a user-controlled flow — a toast notification appears when new content is deployed, and the user chooses when to apply it. No surprise reloads.

### Decentralized Architecture

Game logic runs **100% client-side** in WebAssembly. There is no backend server processing game state. The only hosted component is a stateless WebSocket relay server (~120 lines) that forwards game messages between peers. The server never inspects message payloads — it simply relays them to the other player in the room.

### HTMX Over SPA Frameworks

Instead of React, Vue, or Svelte, the project uses **HTMX** to add dynamic behavior to server-rendered HTML. The "server" happens to be a Rust WASM module running in a Web Worker inside the browser — but HTMX doesn't know or care. This fits naturally with Jekyll's static HTML model: just add `hx-*` attributes to markup.

### Alpine.js + HTMX: DOM Residency Determines the Tool

Alpine.js and HTMX coexist throughout the codebase. The guiding principle is **DOM residency** — whether content always lives in the DOM or comes and goes:

| Layer | Technology | Examples |
|-------|-----------|----------|
| **Always in DOM** (reactive appearance) | Alpine.js | Hamburger menu, visibility toggles, CSS class swaps, animations, transition states |
| **Comes and goes from DOM** (fetched on demand) | HTMX → Rust WASM | Modal content, paginated card lists, QR Scanner interface |
| **Game state authority** | Rust WASM | All game logic, damage state, turn tracking, combat resolution, type matchups |

A feature belongs to HTMX when its content doesn't always need to live in the DOM — modals, long scrollable lists, tool panels that are hidden most of the time. HTMX fetches that content from Rust and swaps it in on demand. A feature stays in Alpine when it's an always-present DOM element that just needs reactive class/style changes. Rust is the single source of truth for all game state — anytime a component needs to know game state, it consults Rust directly via HTMX or the Worker messaging API.

### Type Safety via Rust

Game logic has been ported from JavaScript to Rust, compiled to WASM. The Rust type system catches bugs at compile time that JavaScript hides. The crate currently has **over 100 unit tests** covering route handlers, game logic, matchup tables, combat outcomes, and edge cases.

### Build-Time Code Generation

Card metadata is extracted from Jekyll `_posts/*.html` YAML front matter at build time by a Deno script (`scripts/build-card-catalog.ts`). This generates a Rust source file (`kipukas-server/src/cards_generated.rs`) containing a static array of `Card` structs compiled directly into the WASM binary. No runtime data fetching, no JSON loading, no IndexedDB — just compiled-in data.

### Three-Store State Model

All data is tracked across three distinct stores. Rust is the single source of truth — components always consult Rust (via HTMX or Worker messaging) rather than reading state from JavaScript or the DOM:

| Scope | Store | Persistence | Synced? | Examples |
|-------|-------|-------------|---------|----------|
| **Player** (permanent) | `PLAYER_DOC` (yrs Doc) | `kipukas_player_doc` in localStorage (base64 binary) | No | Card damage, turn alarms, settings |
| **Room** (ephemeral state) | `RoomState` (RefCell) | `kipukas_room` in sessionStorage (JSON bootstrap) | Partially — fists combat via relay | Room code, player name, fists submissions, combat role |
| **Room** (ephemeral CRDT) | `ROOM_DOC` (yrs Doc) | `kipukas_crdt_state` in sessionStorage (base64) | Yes — yrs sync protocol | Shared turn timers (converges via `yrs_update` messages) |

A feature defaults to `PLAYER_DOC` unless it explicitly requires cross-player visibility. Single-player behavior is completely unaffected by multiplayer code. The JavaScript persistence layer (kipukas-api.js) handles the localStorage/sessionStorage read/write because WASM runs in a Web Worker that cannot access browser storage APIs directly — Rust owns the state, JS owns the I/O bridge.

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
| `kipukas-server/src/routes/*.rs` | Route handlers (type matchup, QR, cards, game, room, shared utils) |
| `kipukas-server/src/game/player_doc.rs` | **Authoritative player data store** — yrs CRDT Doc for damage, alarms, settings |
| `kipukas-server/src/game/*.rs` | Damage rendering, turn logic, room/combat state, CRDT sync |
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

### Pattern 2: ~~Sentinel Div for Hidden State~~ → Superseded by Pattern 12

> **⚠️ Superseded.** This pattern was replaced by [Pattern 12: Alpine Fire-and-Forget](#pattern-12-alpine-fire-and-forget-for-always-in-dom-game-state). The sentinel div + `@htmx:after-swap` approach had a CSS specificity bug: each per-button Alpine `x-data` with `:class` bindings conflicted with static Tailwind classes on the same element. Different browsers resolved the specificity race differently — Firefox-based browsers (Floorp, Trivalent) only changed the border color, not the fill. The HTMX swap after each click also caused a visual double-update. Pattern 12 eliminates the sentinel entirely — Alpine computes `allChecked()` reactively from its own state, and fire-and-forget replaces HTMX swaps for per-click mutations.

**Original pattern (for historical context):** WASM rendered a hidden sentinel div (`<div class="keal-all-checked hidden">`) when all keal means checkboxes were checked. Alpine's `x-effect` on the parent watched for this sentinel after each HTMX swap and toggled a CSS class (`.show-final-blows`) that made the `.final-blows-section` visible. Each button had its own `x-data="{ on: true/false }"` with `:class` bindings for visual state, plus `hx-post` for HTMX swaps on every click.

**Why it was replaced:** The per-button `x-data` `:class` bindings set both static Tailwind classes (e.g., `bg-white border-emerald-600`) and Alpine dynamic classes (e.g., `:class="on ? 'bg-red-600 border-red-600' : 'bg-white border-emerald-600'"`). When Alpine toggled `on`, both sets coexisted on the element, creating a CSS specificity race that different browsers resolved differently. Additionally, the double-update (Alpine toggle + HTMX swap) caused visual jank. See Pattern 12 for the correct approach.

### Pattern 3: Alpine × HTMX Coexistence

**The pattern:** Alpine manages UI chrome (modals, toggles, visibility). HTMX manages data (fetching, computing, displaying). They communicate via:

1. **Alpine → HTMX:** `htmx.ajax()` calls from Alpine event handlers (e.g., `@click="htmx.ajax('POST', ...)"`)
2. **HTMX → Alpine:** `x-effect` watching for DOM changes after HTMX swaps (sentinel pattern)
3. **Cross-component:** Custom DOM events (e.g., `document.dispatchEvent(new CustomEvent('close-multiplayer'))` listened by `@close-multiplayer.window="showMultiplayer = false"`)

**The bridge function:** `kipukasRefreshCards()` reads Alpine's reactive `filter` and `searchQuery` state, builds a URL, and calls `htmx.ajax()`. This bridges Alpine UI state to HTMX data fetching without coupling them.

**When to use Alpine:** Elements that always live in the DOM but need reactive appearance changes — show/hide toggles, CSS class switching, animations, transition states. Alpine never fetches data or reads game state.

**When to use HTMX:** Content that doesn't always need to live in the DOM — modal content, paginated lists, tool panels. HTMX fetches from Rust and swaps it in; HTMX also posts user actions (combat submissions) to Rust to update game state.

**Hybrid pattern (HTMX initial load → Alpine reactive state):** Some content is fetched once by HTMX (`hx-trigger="load"`) and then stays in the DOM with Alpine managing reactive state. The keal damage tracker is the primary example — see [Pattern 12](#pattern-12-alpine-fire-and-forget-for-always-in-dom-game-state). In these cases, HTMX handles the initial fetch, Alpine owns the visual reactivity, and fire-and-forget `kipukasWorker.postMessage()` syncs mutations to WASM.

**When to consult Rust directly:** Any time a component needs game state, it asks Rust via HTMX (`hx-get`) or the Worker messaging API (`postToWasmWithCallback`). Components never read state from localStorage or JavaScript variables — Rust is the single source of truth.

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

**The pattern:** After WASM auto-marks damage (e.g., from combat outcome), the keal damage tracker on the card page is stale. A JavaScript helper finds the tracker element, re-fetches the full HTML from WASM, and re-initializes Alpine:

```javascript
function refreshKealTracker() {
  const tracker = document.querySelector('[id^="keal-damage-"]');
  if (tracker) {
    const slug = tracker.id.replace('keal-damage-', '');
    wasmRequest('GET', '/api/game/damage', '?card=' + slug, '', (html) => {
      tracker.innerHTML = html;
      if (typeof Alpine !== 'undefined') Alpine.initTree(tracker);
    });
  }
}
```

**Why it works:** The WASM state is authoritative. When state changes programmatically (not from a user click), the DOM must be explicitly refreshed. A small `setTimeout(refreshKealTracker, 150)` delay ensures the WASM worker has finished processing before the refresh request arrives.

**Alpine re-initialization:** Since the damage tracker uses Pattern 12 (Alpine `x-data` scope rendered by WASM), `refreshKealTracker` must call `Alpine.initTree(tracker)` after the innerHTML swap so Alpine discovers and initializes the new `x-data` scope with correct initial values from PLAYER_DOC. The WASM response includes the full Alpine state (`slots`, `wasted`, helper methods), so the re-initialized tracker reflects the authoritative game state.

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

**The pattern:** WASM state uses `thread_local!` + `RefCell` for safe mutable globals. The three stores each use their own `thread_local!`:

```rust
// PLAYER_DOC — persistent player data (yrs CRDT Doc)
thread_local! {
    static PLAYER_DOC: RefCell<Doc> = RefCell::new(new_player_doc());
}

// RoomState — ephemeral multiplayer state (serde struct)
thread_local! {
    static ROOM: RefCell<RoomState> = RefCell::new(RoomState::default());
}

// ROOM_DOC — shared multiplayer CRDT (yrs Doc, synced between peers)
thread_local! {
    static CRDT_DOC: RefCell<Doc> = RefCell::new(Doc::new());
}
```

**Why it works:** The WASM module runs in a single Web Worker thread. `thread_local!` provides safe global state without `unsafe`. The `RefCell` borrow checker prevents concurrent access at runtime, though in practice the single-threaded worker never triggers it. Each store has its own lifecycle: PLAYER_DOC persists to localStorage forever, RoomState lives in sessionStorage for the browser session, and ROOM_DOC is created/destroyed per multiplayer room.

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

**Why sessionStorage (not localStorage):** Room connections are ephemeral — they should survive page navigation within a session but not persist across browser restarts. `sessionStorage` provides exactly this lifecycle. Player state (damage, turns, settings) is owned by `PLAYER_DOC` in Rust and persisted to `localStorage` as base64 binary via the kipukas-api.js I/O bridge. Note: `sessionStorage` here stores only the reconnection bootstrap info (code, name, creator) — Rust's `RoomState` owns the actual room state in-memory.

### Pattern 11: Self-Contained Tool Component (Alpine × HTMX × Tailwind)

**The pattern:** Each interactive tool lives in a single `_includes/*.html` file that combines four layers: Alpine.js `x-data` for UI-only state (always-in-DOM visibility control), `@click`/`$watch`/`x-effect` for behavior, HTMX `hx-get`/`hx-post` for fetching content from Rust and posting user actions, Tailwind utilities for all styling, and auto-persistence via the Worker `PERSIST_STATE` message bridge. The component is fully self-contained — no external CSS, no separate JS file, no global state leakage.

**Reference implementation:** `_includes/multiplayer_fists_tool.html` (also see `turn_tracker.html`, `local_fists_tool.html`).

**Layer breakdown:**

| Layer | Technology | Role in the component |
|-------|-----------|----------------------|
| **UI state** | Alpine `x-data` | Local booleans like `showFistsMenu`, `roomConnected` — purely visual, never persisted |
| **Behavior** | Alpine `@click`, `$watch`, `x-effect` | `@click` toggles visibility; `$watch` handles side effects; `x-effect` bridges to HTMX |
| **Data fetching** | HTMX `hx-get` + `hx-trigger` | Declarative WASM endpoint on the container div; `x-effect` re-fetches via `htmx.ajax()` on reopen |
| **Styling** | Tailwind utilities | Layout, theming, spacing, responsiveness — all inline classes, zero custom CSS |
| **Persistence** | base64 yrs binary → localStorage (via kipukas-api.js) | WASM state auto-persists after every mutation via Worker `PERSIST_STATE` message; component reads fresh state from WASM on each open |

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
- **Rust is the single source of truth.** The component never reads or writes localStorage directly. It asks Rust for current state via HTMX, and Rust's `PLAYER_DOC` state is auto-persisted to localStorage as base64 yrs binary by the `kipukas-api.js` bridge after every game mutation (via Worker `PERSIST_STATE` message). On page load the bridge restores the binary state → Rust before any component renders.
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

### Pattern 12: Alpine Fire-and-Forget for Always-in-DOM Game State

**The pattern:** Content that is fetched once by HTMX (`hx-trigger="load"`) and then stays in the DOM uses a **single Alpine `x-data` scope** for all reactive visual state. User interactions toggle Alpine state immediately (instant visual feedback) and fire-and-forget to the WASM worker via `kipukasWorker.postMessage()` — no HTMX swap, no MessageChannel, no response needed. The worker processes the mutation, and `PERSIST_STATE` auto-saves to localStorage.

**Reference implementation:** `kipukas-server/src/game/damage.rs` → `render_damage_tracker()` + `_includes/keal_damage_tracker.html`

**Why this exists (what failed before — see Pattern 2):** The previous approach gave each button its own `x-data="{ on: false }"` with `:class` bindings AND static Tailwind classes, plus `hx-post` for HTMX swaps on every click. This caused two bugs:
1. **CSS specificity race:** Alpine's `:class` added dynamic classes (e.g., `bg-red-600`) without removing the static classes (e.g., `bg-white`) on the same element. Different browsers resolved the conflict differently — Firefox-based browsers (Floorp, Trivalent) only changed the border, not the fill.
2. **Visual jank:** Alpine toggled instantly, then HTMX replaced the entire HTML — a double-update that caused flashing on all browsers.

**The three layers:**

| Layer | Technology | Role |
|-------|-----------|------|
| **Initial render** | HTMX `hx-trigger="load"` | Fetches tracker HTML from WASM once on page load |
| **Reactive visual state** | Alpine `x-data` at container level | Owns `slots`, `wasted`, `allChecked()`, `slotClass()` — drives all `:class` bindings |
| **State persistence** | Fire-and-forget `kipukasWorker.postMessage()` | Worker processes POST → `PERSIST_STATE` → localStorage. No response, no swap. |

**How it works (annotated):**

```html
<!-- _includes/keal_damage_tracker.html — thin wrapper -->
<style>
  .final-blows-section { display: none; }
  .show-final-blows .final-blows-section { display: block; }
</style>
<div id="keal-damage-{{ cardSlug }}"
     hx-get="/api/game/damage?card={{ cardSlug }}"
     hx-trigger="load"
     hx-swap="innerHTML">
</div>
```

```html
<!-- WASM renders this as the innerHTML (single Alpine scope) -->
<div x-data="{
  slots: {1: false, 2: true, 3: false},
  wasted: false,
  allChecked() { return Object.values(this.slots).every(function(v) { return v }) },
  slotClass(n) {
    if (this.wasted) return this.slots[n] ? 'bg-slate-400 border-slate-400' : 'bg-transparent border-slate-400';
    return this.slots[n] ? 'bg-red-600 border-red-600' : 'bg-white border-emerald-600'
  },
  fire(body) { kipukasWorker.postMessage({method:'POST',pathname:'/api/game/damage',search:'',body:body}) },
  toggleSlot(n) { if (this.wasted) return; this.slots[n] = !this.slots[n]; this.fire('card=SLUG&slot='+n) },
  toggleWasted() { this.wasted = !this.wasted; this.fire('card=SLUG&action=wasted') }
}"
:class="{ 'show-final-blows': allChecked() || wasted }"
class="w-11/12 md:w-2/3 xl:w-1/2 pb-4 place-self-center">

  <!-- Each button: Alpine @click + :class only. No hx-post, no static bg/border classes. -->
  <button class="mr-1 damage-slot" @click="toggleSlot(1)"
    :class="{'opacity-40 pointer-events-none': wasted}">
    <div class="w-5 h-5 rounded-full border-2 transition-colors duration-300"
         :class="slotClass(1)"></div>
  </button>

  <!-- Final Blows — always in DOM, visibility driven by Alpine :class on container -->
  <div class="final-blows-section">
    <button class="mr-1 damage-slot" @click="toggleWasted()">
      <div class="w-5 h-5 rounded-full border-2 transition-colors duration-300"
           :class="wasted ? 'bg-red-600 border-red-600' : 'bg-white border-emerald-600'"></div>
    </button>
  </div>
</div>
```

**Worker fire-and-forget protocol:** `kipukasWorker.postMessage()` without a MessageChannel port. The worker detects the missing port and processes the request without responding:

```javascript
// kipukas-worker.js — fire-and-forget handler (no port)
if (!port) {
  if (!initialized) await wasmReady;
  handle_request(method, pathname, search || '', body || '');
  if (method === 'POST' && pathname.startsWith('/api/game/')) {
    self.postMessage({ type: 'PERSIST_STATE' });
  }
  return;
}
```

**Why it works:**

- **No CSS specificity conflict.** Alpine's `:class` is the **only** source of `bg-*` and `border-*` classes. No static classes to fight with. Clean transitions via `transition-colors duration-300` on the same DOM nodes.
- **Instant visual feedback.** Alpine toggles the reactive state immediately — no waiting for WASM response. The user sees the color change in the same frame as their click.
- **Rust remains the single source of truth.** The initial `x-data` values (`slots`, `wasted`) come from WASM's authoritative PLAYER_DOC. Fire-and-forget syncs the mutation back. `PERSIST_STATE` saves to localStorage automatically.
- **Programmatic refresh still works.** `refreshKealTracker()` (Pattern 5) re-fetches from WASM and calls `Alpine.initTree()` to re-initialize the `x-data` scope with fresh values after combat outcomes.
- **No sentinel div needed.** Alpine computes `allChecked()` reactively from its own `slots` state. The `:class="{ 'show-final-blows': allChecked() || wasted }"` binding on the container drives Final Blows visibility without any DOM querying.

**When to use this pattern:**
- Content fetched once from WASM that **stays in the DOM** for the page's lifetime
- Interactive elements that need **instant visual feedback** (toggling, switching, animating)
- State that must sync to WASM but **doesn't need a response** to update the UI

**When NOT to use this pattern (use Pattern 11 instead):**
- Modal content that **comes and goes** (open/close cycles) — use HTMX swaps + `x-effect` re-fetch
- Content where the WASM response **determines** what to display (e.g., fists combat results) — use `postToWasmWithCallback` (Pattern 6)

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

**Alpine vs HTMX decision (DOM residency model):**
- Use **Alpine** for: always-in-DOM elements that need reactive appearance changes — visibility toggles, CSS class swaps, animations, transition states
- Use **HTMX** for: content that comes and goes from the DOM — modal content, paginated lists, tool panels. HTMX fetches from Rust and posts user actions to Rust
- Use **Alpine + fire-and-forget** for: content fetched once by HTMX that stays in the DOM with Alpine-driven reactive state — damage trackers (see Pattern 12). HTMX handles initial load, Alpine owns visual reactivity, `kipukasWorker.postMessage()` syncs to WASM
- **Rust is the single source of truth** for all game state — components consult Rust directly, never localStorage or JS variables

**Jekyll exclusions:**
Non-Jekyll directories must be listed in `_config.yml` under `exclude:` to prevent Jekyll from processing them (especially `kipukas-server/target/` which contains thousands of Rust build files).

**Generated files (do not edit manually):**
- `kipukas-server/src/cards_generated.rs` — regenerated by `deno task build:card-catalog`
- `assets/js/alpine.bundle.min.js` — regenerated by `deno task build:alpine`
- `assets/js/htmx.min.js` — vendored by `deno task build:htmx`
- `sw.js` / `sw.js.map` — regenerated by `deno task build:sw`
- `assets/css/output.css` — regenerated by `deno task build:css`

---

## Desired Next Features

Features are grouped by priority. Items marked *post-launch* require the game to be publicly available first.

### Near-Term

#### 1. QR Room Join
Embed the room code in a QR code so scanning joins the room directly. This connects two existing features (QR scanner + multiplayer) with minimal new code. The flow: Player A creates a room → room code appears as both text and a QR. Player B scans the QR → auto-joins the room. The QR URL format could be `kpks.us/join?code=ABCD#room=myroom` with a redirect that passes the code to the multiplayer module.

#### 2. Deck Builder / Hand Management
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

#### 3. Combat History Log
Persist combat results in yrs document so players can review past rounds across sessions. Each outcome (attacker, defender, keal means used, modifier, who won) stored as a `CombatRecord` in a `YArray`.

**UI**: Scrollable log modal accessible from toolbar, filterable by date range or opponent (if identity known).

**Technical**: Append-only `YArray` in yrs document; automatic synchronization if cross-device sync enabled.

### Long-Term

#### 4. Replace ZXing with Rust QR Decoder
Eliminate the ~2MB third-party ZXing WASM dependency by compiling a Rust QR decoder into `kipukas-server`. **Caveat:** This has been explored. `rxing` (Rust port of ZXing) produces a ~6MB WASM binary — too large. `rqrr` is small but struggles with Kipukas' anti-cheat camouflaged QR codes, which require robust error correction and perspective distortion handling. This feature is blocked until either `rxing` becomes smaller/more WASM-friendly or `rqrr` improves its decoding of difficult QR patterns. **NOTE:** It was also attempted to improve rqrr detection, but the results were overall worse that ZXing with greater complication and dimishing returns on space saved. Many strategies were attempted, but only adaptive_threshold had any effect. When feature discussions come up ask to check on state of the libs (robustness to detection is the primary concern).

#### 5. Infinite Scroll with Content-Visibility
Replace the sentinel-chain pagination on the index page with a true rolling infinite scrolling system including position tracking and DOM replacements. Card count need to be around 150 to consider the feature.

#### 6. Card Trading
Propose an NFT brokered trade of cards marked in deck. Requires the game to be publicly available with a real player base to validate the mechanic. Also, requires the store website to be online (kipukas.com).

#### 7. Spectator Mode
Allow a third peer to observe a match via a read-only WebSocket connection. Architecturally simple (receive-only relay, no submissions) but requires rooms to support >2 peers. Low priority until competitive, streaming, or particularly compelling (active, visual, and exciting) use cases emerge.

#### 8. Provide Kippa Tools
Expand Kippa's understanding of the game by allowing it to assist users in using site features, gathering specific card data, and resolving issues.

---

## Current Sprint: Player Document & GameState → Yrs Consolidation

> **Goal:** Replace the serde_json `GameState` with a persistent yrs CRDT document (`PLAYER_DOC`) that owns all local player data. This eliminates future migration cost, makes all player state portable and exportable from day one, and sets the foundation for cross-device sync, decentralized identity, and affinity/loyalty tracking.
>
> **Philosophy:** Kipukas is not meant to lock down the game after purchase. If Kipukas the company ceases to exist, gameplay experience should be unaffected. A player's progression data (affinity, loyalty, damage, decks) lives on *their* device in a conflict-free, exportable format. When a dedicated store account is available, backup/restore becomes a service — not a requirement.

### Architecture Overview

```
Current State (Phase A complete)
┌─────────────────────────────────────┐
│  thread_local! stores:              │
│                                     │
│  PLAYER_DOC (yrs Doc)               │
│    • "cards": YMap<slug, YMap>       │
│    • "alarms": YArray<YMap>          │
│    • "settings": YMap                │
│    (future: "affinity", "loyalty")   │
│                                     │
│  RoomState (RefCell)                │
│    • code, name, fists combat       │
│                                     │
│  ROOM_DOC (yrs Doc)                 │
│    • "alarms" (shared turn timers)  │
└─────────────────────────────────────┘
              │
         localStorage
      "kipukas_player_doc"
       (base64 yrs binary)
```

**Key difference from ROOM_DOC:** The `PLAYER_DOC` is **player-scoped** — created once on first visit, persisted forever in localStorage, restored on every page load. The `ROOM_DOC` remains **room-scoped** — created on room join, destroyed on disconnect. They are independent yrs Doc instances.

**Legacy cleanup (complete):** The old `GameState` serde struct and its `kipukas_game_state` JSON localStorage key have been fully removed. `state.rs` now contains only the shared `Alarm` struct. Migration code has been deleted from `player_doc.rs`, `kipukas-api.js`, and `kipukas-worker.js`. The `routes/util.rs` module provides shared URL/form parsing used by all route handlers.

### Sprint Phases

Each phase is independently shippable. Later phases depend on earlier ones but can be deferred.

#### Phase A: Player Document Infrastructure ✅ Complete

**Status:** Shipped and legacy fully cleaned up. `PLAYER_DOC` (yrs CRDT Doc) is the sole authoritative store for all local player data. The old `GameState` serde struct, its `kipukas_game_state` localStorage key, migration code, and dead routes have all been removed. `state.rs` retains only the shared `Alarm` struct. A shared `routes/util.rs` module provides URL/form parsing for all route handlers.

**Active routes:**

| Route | Method | Purpose |
|-------|--------|---------|
| `/api/player/state` | GET | Export PLAYER_DOC as base64 binary (for persistence) |
| `/api/player/restore` | POST | Restore PLAYER_DOC from base64 binary |
| `/api/player/export` | GET | Download full state as base64 file |
| `/api/player/import` | POST | Upload and merge state from base64 file |

**PLAYER_DOC structure:**

| Root key | yrs type | Contents |
|----------|----------|----------|
| `"cards"` | `YMap<slug, YMap>` | `{ slots: YArray<bool>, wasted: bool }` per card |
| `"alarms"` | `YArray<YMap>` | `{ remaining: i32, name: String, color_set: String }` |
| `"settings"` | `YMap` | `{ show_alarms: bool }` |

---

#### Phase B: Affinity Tracking ✅ Complete

**Status:** Shipped. Players can declare archetypal affinity once per day. Affinity level grows with each declaration. The +1 roll bonus for matching cards is displayed in the fists combat result when the attacker's `genetic_disposition` matches the player's active affinity. The affinity panel is accessible from the toolbar on both the home page and card pages.

**PLAYER_DOC structure:**

```
"affinity" → YMap {
    "Brutal"       → YMap { level: 3, last_declared: "2026-02-25" },
    "Avian"        → YMap { level: 7, last_declared: "2026-02-24" },
    ...
}
```

**Active routes:**

| Route | Method | Purpose |
|-------|--------|---------|
| `/api/player/affinity` | GET | Render affinity panel (all 15 archetypes, current levels, declare button) |
| `/api/player/affinity` | POST | Declare affinity for an archetype (increments level, sets date, enforces once-per-day) |

**UI:** Toolbar tool `_includes/affinity_tool.html` following Pattern 11. Shows all 15 archetypal adaptations with visual level dots (up to 10, then `+N`). "Declare" button per archetype (shows "✓ Today" if already declared). Active affinity highlighted with ★ and amber background. Active affinity summary card at top with "+1 roll bonus on matching cards" note.

**Fists integration:** In `build_result_html()` (routes/room.rs), the Attack Die Modifier computation checks `player_doc::get_active_affinity()`. If the attacker's card `genetic_disposition` matches the active affinity archetype, +1 is added to the displayed total modifier with a labeled note: "+1 from Affinity ({archetype})".

**Files created/modified:**

| File | Changes |
|------|---------|
| `kipukas-server/src/game/player_doc.rs` | Added `VALID_ARCHETYPES`, `declare_affinity()`, `get_affinity()`, `get_all_affinities()`, `get_active_affinity()`, `valid_archetypes()` + 12 unit tests |
| `kipukas-server/src/routes/player.rs` | **New.** Route handlers for `/api/player/affinity` GET/POST + 6 unit tests |
| `kipukas-server/src/routes/mod.rs` | Added `pub mod player` |
| `kipukas-server/src/lib.rs` | Registered `/api/player/affinity` route + dispatch |
| `_includes/affinity_tool.html` | **New.** Toolbar component (Pattern 11) with DNA helix icon |
| `_includes/toolbar.html` | Added `affinity_tool` include slot |
| `index.html` | Enabled `affinity_tool=true` in toolbar include |
| `_layouts/card.html` | Enabled `affinity_tool=true` in toolbar include |
| `kipukas-server/src/routes/room.rs` | Added affinity bonus to `build_result_html()` modifier display |

**Daily limit:** Compare `last_declared` date string against current local date. No server or timezone handling — the WASM module uses the date string passed from JS via the POST body. The `today` param is supplied by client-side `new Date().toISOString().slice(0,10)`.

---

#### Phase C: Loyalty Tracking

**What ships:** Per-card play counter. Loyalty increments when a soul card is used in fists combat (once per day per card). Loyalty badge/counter displayed on card damage tracker pages.

**PLAYER_DOC structure:**

```
"loyalty" → YMap {
    "brox_the_defiant"          → YMap { total_plays: 12, last_played: "2026-02-25" },
    "frost_tipped_arctic_otter" → YMap { total_plays: 3,  last_played: "2026-02-20" },
    ...
}
```

**Trigger:** When a fists submission is POSTed (`/api/room/fists` or `/api/room/fists/final`), after storing the submission, check the card slug. If the card is a Character or Species (`layout` field), increment loyalty for that slug in PLAYER_DOC (enforcing once-per-day).

**Display:** On the card page damage tracker (`/api/game/damage?card=slug`), append a small loyalty badge: "♥ 12 plays" or similar. On Species cards, show progress toward tameability threshold if tameability data exists.

**Files to create/modify:**

| File | Changes |
|------|---------|
| `kipukas-server/src/game/player_doc.rs` | Add `increment_loyalty()`, `get_loyalty()` functions |
| `kipukas-server/src/game/damage.rs` | Render loyalty badge in damage tracker HTML |
| `kipukas-server/src/routes/room.rs` | Hook loyalty increment into fists submission handlers |

---

#### Phase D: Tameability Integration

**What ships:** Species cards show a tameability section with threshold, current loyalty + affinity stack, and "Tamed!" indicator.

**Card data changes:**
- Add `tameability` field to Species card YAML front matter (optional, integer or `"∞"`)
- Update `scripts/build-card-catalog.ts` to extract `tameability`
- Update `Card` struct in `cards_generated.rs` template to include `pub tameability: Option<u32>`

**Tamed condition:** `loyalty.total_plays + affinity.level + incubation_bonus ≥ tameability`

**Display:** On Species card pages, below the damage tracker: progress bar showing `current / threshold`, with a "Tamed ✓" badge when met.

**Files to create/modify:**

| File | Changes |
|------|---------|
| `_posts/*.html` | Add `tameability:` field to Species card YAML |
| `scripts/build-card-catalog.ts` | Extract and emit `tameability` field |
| `kipukas-server/src/cards_generated.rs` | Template updated (auto-generated) |
| `kipukas-server/src/game/damage.rs` | Render tameability section for Species cards |
| `kipukas-server/src/game/player_doc.rs` | Add `is_tamed()` function combining loyalty + affinity + bonuses |

---

#### Phase E: Obfuscated/Encrypted Export/Import

**Goal** Provide users a 'hardcopy' of their data state which is difficult to modify for cheating (maximizing affinity for type as an example).

**What ships:** Ed25519 keypair generated in WASM. Private key encrypted in localStorage. Encrypted backup file download/upload. QR-based key export for device pairing.

**New crate dependency:** `ed25519-dalek` (or `ring` for broader crypto) — evaluate WASM size impact.

**This phase is deferred until Phases A–D are stable.** The PLAYER_DOC binary format is already suitable for encrypted backup — Phase E adds the encryption layer and identity semantics.

---

#### Phase F: Cross-Device Sync (Future)

**What ships:** yrs sync protocol between devices over the existing WebSocket relay. Device pairing via keypair exchange. Automatic conflict resolution via CRDT merge.

**Reuses:** The same `yrs_sv → yrs_sv_reply → yrs_update` handshake proven in multiplayer turn timer sync. The "room" concept extends to "device pairing room" — two devices join a persistent sync channel authenticated by keypair.

**This phase is deferred until Phase E provides the identity/authentication layer.**

---

### Guiding Constraints

- **No new crate dependencies in Phases B–D.** `yrs`, `base64`, `serde`, `serde_json` are already in the binary. The PLAYER_DOC uses the exact same yrs patterns proven in `crdt.rs`.
- **Tests first.** Each phase must include unit tests for new player_doc functions before wiring routes.
- **UI is separate from infrastructure.** Phases B/C/D add UI incrementally via self-contained `_includes/*.html` components (Pattern 11).
- **Single-player unaffected.** Affinity/loyalty tracking is purely local. Multiplayer features (ROOM_DOC, fists combat) remain independent. The only cross-cutting concern is the loyalty increment hook in fists submission.
