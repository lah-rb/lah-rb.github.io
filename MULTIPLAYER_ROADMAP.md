# Kipukas Multiplayer Roadmap

> **Status:** Phase 3a complete (Card grid infinite scroll migrated to HTMX + WASM)
> **Started:** February 2026
> **Architecture:** HTMX + In-Browser WASM Server + WebRTC (future)

---

## Vision

Transform Kipukas from a single-player card binder PWA into a **mostly-decentralized multiplayer** game platform. Each player's browser runs its own WASM "game server" locally. A tiny signaling server only brokers WebRTC connections — all game logic stays client-side. The result: multiplayer with minimal infrastructure costs, offline-first by design, and the same codebase serving both single and multiplayer modes.

---

## Goals

1. **Replace brittle Alpine.js patterns** with HTMX + WASM server-driven HTML fragments
2. **Port client-side JS utilities** (typing.js, damage tracking, etc.) to type-safe Rust
3. **Establish the `/api/*` routing pattern** that will carry through to multiplayer
4. **Keep it offline-first** — the PWA continues to work without a network connection
5. **Minimize infrastructure** — the signaling server is the only hosted component; game state lives in each player's browser
6. **Incremental migration** — Alpine.js and HTMX coexist; features migrate one at a time

---

## Crucial Architectural Decisions

### Decision 1: HTMX over a JavaScript framework

**Why not React/Vue/Svelte?** The site is a Jekyll static site with Tailwind CSS. Adding a full SPA framework would require a total rewrite. HTMX fits the existing server-rendered HTML model — it just adds `hx-*` attributes to existing markup. The "server" happens to be WASM running in the browser, but HTMX doesn't know or care.

**Key insight:** HTMX makes standard HTTP fetches. By intercepting those fetches at the Service Worker layer, we can route them to WASM without HTMX knowing anything about our architecture.

### Decision 2: In-Browser WASM Server (not a remote API)

**Why not a real backend?** Kipukas is hosted on GitHub Pages — there is no server. Adding one would mean ongoing hosting costs and latency. By compiling the game logic to WASM and running it in a Web Worker, we get:

- **Zero latency** — requests never leave the browser
- **Offline play** — works without internet after first load
- **Type safety** — Rust catches bugs at compile time that JavaScript hides
- **Multiplayer-ready** — the same WASM binary runs on every player's device; state synchronization becomes a matter of sending diffs, not re-implementing logic

### Decision 3: Option C — SW + Web Worker Sidecar (Hybrid)

Three architectures were considered:

| Option | Where WASM runs | Pros | Cons |
|--------|-----------------|------|------|
| **A: WASM in Service Worker** | SW thread | Simplest routing | SW has no ES module imports; `importScripts` is sync-only; debugging is painful |
| **B: WASM on main thread** | Page thread | No message-passing | Blocks UI during computation; no web worker isolation |
| **C: SW + Web Worker sidecar** ✅ | Dedicated Worker | Clean separation; off-main-thread; module imports work | Requires MessageChannel relay through the page |

**Option C was chosen** because:
- The Web Worker runs as `{ type: 'module' }`, enabling clean ES imports of the wasm-bindgen glue
- WASM computation is off the main thread (won't block UI even for complex game logic)
- The MessageChannel relay is transparent — HTMX makes a fetch, gets a Response, never knows about the plumbing
- The complexity is isolated in three small files that rarely need to change

### Decision 4: matchit Router (Axum's engine)

The Rust crate uses [`matchit`](https://crates.io/crates/matchit) — the same radix-tree router that powers Axum. This means:
- Route patterns like `/api/game/:id/state` work out of the box
- If we ever need a real server (e.g., for the signaling server), the route definitions are directly portable to Axum
- ~69KB WASM binary including the router — tiny

### Decision 5: Dual-Path Execution

The bridge script (`kipukas-api.js`) implements two execution paths:

- **Production (SW active):** Full relay — HTMX fetch → SW intercepts → page bridge → Web Worker → WASM → MessageChannel → SW → Response
- **Development / first load (no SW):** Direct shortcut — HTMX `beforeRequest` event → page bridge → Web Worker → WASM → DOM swap

This ensures the type matchup works immediately on first visit and during `jekyll serve` development without waiting for the SW to install.

---

## Phase 1: Foundation (✅ Complete)

### What was built

**Rust WASM Crate (`kipukas-server/`)**
- `lib.rs` — Entry point: `handle_request(method, path, query) → HTML string`
- `typing.rs` — Complete port of `typing.js` with type-safe enums for Archetypes, Motivations, and all matchup tables
- `routes/type_matchup.rs` — Parses query parameters, runs matchup logic, returns HTML fragment
- 17 unit tests covering matchup logic, route handling, and edge cases
- 69KB release WASM binary (with LTO + size optimization)

**JavaScript Bridge Layer**
- `kipukas-worker.js` — Module Web Worker that loads WASM and handles request messages
- `kipukas-api.js` — Page bridge with SW relay + development fallback
- `sw-src.js` — Added `/api/*` route interception with MessageChannel + 5-second timeout

**HTMX Integration**
- Vendored HTMX 2.0.4 (~50KB) via `deno.json` npm import
- `type_matchup.html` — Added `hx-get="/api/type-matchup"`, `hx-target="#type-result"`, `hx-include` for form inputs
- Checkboxes have `name="atk"/"def"` with `value` attributes for native form serialization
- Radio buttons have `name="motAtk"/"motDef"` with `value` attributes
- Alpine.js retained for UI state management (toggles, max-3 disabled logic)

**Build Pipeline**
- `deno task build:wasm` — wasm-pack build → `assets/js-wasm/kipukas-server-pkg/`
- `deno task build:htmx` — Vendor HTMX → `assets/js/htmx.min.js`
- Full build order updated: WASM → HTMX → CSS → Alpine → Rules → Jekyll → SW

### Files created/modified

| File | Action |
|------|--------|
| `kipukas-server/Cargo.toml` | Created |
| `kipukas-server/src/lib.rs` | Created |
| `kipukas-server/src/typing.rs` | Created |
| `kipukas-server/src/routes/mod.rs` | Created |
| `kipukas-server/src/routes/type_matchup.rs` | Created |
| `assets/js/kipukas-worker.js` | Created |
| `assets/js/kipukas-api.js` | Created |
| `assets/js/htmx.min.js` | Created (vendored) |
| `sw-src.js` | Modified (added `/api/*` route) |
| `_layouts/default.html` | Modified (added HTMX + bridge scripts) |
| `_includes/type_matchup.html` | Modified (removed typing.js, added hx-* attributes) |
| `deno.json` | Modified (htmx import, build tasks, fmt/lint excludes) |
| `_config.yml` | Modified (exclude kipukas-server/, kipukas_rules_book/) |
| `.gitignore` | Modified (kipukas-server/target/, htmx.min.js) |
| `WORKFLOW.md` | Modified (WASM server docs, updated pipeline) |

### Lessons learned

1. **Jekyll processes everything** — The Rust crate's `target/` directory (with thousands of files) and `kipukas_rules_book/node_modules/` must be excluded in `_config.yml`
2. **SW isn't available on first load** — The development fallback via `htmx:beforeRequest` is essential, not optional
3. **HTMX + Alpine coexistence works** — Alpine manages DOM visibility and UI state; HTMX handles data fetching and HTML swapping; they don't conflict
4. **Module Web Workers need `{ type: 'module' }** — Required for ES import of the wasm-bindgen glue code

---

## Phase 2: QR Scanner Migration (✅ Complete)

### Problem

The QR scanner flow used Alpine.js with complex state management (`showScanner`, `showFlash`, `videoReady`, `noCamera`, `showQRModal`) spread across `_layouts/default.html` and `_includes/qr_scanner.html`. It relied on the third-party ZXing WASM library loaded via a separate script tag. The flow was historically brittle and had recently broken.

### What was built

**Rust WASM Routes (`kipukas-server/src/routes/qr.rs`)**
- `/api/qr/status` — UI state machine: returns HTML fragments for privacy modal, scanning UI, close, and error states
- `/api/qr/found` — URL validation: accepts decoded QR URL, validates it as a Kipukas domain (kpks.us, kipukas.cards), returns redirect fragment
- Percent-decode utility for URL-encoded query parameters
- 14 unit tests covering URL validation, state transitions, and edge cases

**Camera Module (`assets/js/qr-camera.js`)**
- Replaces old Alpine-driven `qr_scanner.js` entirely
- Manages camera start/stop via browser `getUserMedia` API
- Frame capture loop at 2 fps: canvas → raw RGBA pixels → Web Worker (zero-copy transfer)
- Listens for `QR_FOUND` messages from worker, swaps result HTML into DOM
- Exposed globally as `window.kipukasQR` for use by WASM-returned HTML fragments
- `toggle()` function checks `localStorage` for privacy acceptance, routes through `htmx.ajax()`

**Web Worker Updates (`assets/js/kipukas-worker.js`)**
- Loads ZXing WASM alongside Rust WASM in the same worker
- Handles `QR_FRAME` messages: ZXing decodes pixels → Rust formats result via `/api/qr/found`
- Posts `QR_FOUND` back to main thread with formatted HTML + decoded URL

**Bridge Updates (`assets/js/kipukas-api.js`)**
- Fixed dev fallback query string handling (empty params, double `?` prefix, target resolution)
- Inline `<script>` re-execution after `innerHTML` swap

**HTMX-Driven State Machine**
- All scanner state transitions driven by WASM-returned HTML fragments
- Privacy modal → scanning UI → QR found → redirect (all server-driven)
- Buttons use `onclick` + `htmx.ajax()` instead of `hx-get` attributes (see lessons learned)

### Files created/modified

| File | Action |
|------|--------|
| `kipukas-server/src/routes/qr.rs` | Created |
| `kipukas-server/src/routes/mod.rs` | Modified (added qr module) |
| `kipukas-server/src/lib.rs` | Modified (added /api/qr/* routes) |
| `assets/js/qr-camera.js` | Created |
| `assets/js/kipukas-worker.js` | Modified (ZXing loading + QR_FRAME handler) |
| `assets/js/kipukas-api.js` | Modified (dev fallback bugfixes, script execution) |
| `_includes/qr_scanner.html` | Modified (replaced Alpine with HTMX + #qr-container) |
| `_layouts/default.html` | Modified (removed ZXing script tag, removed Alpine QR state, added qr-camera.js) |
| `.gitignore` | Modified (removed build output exclusions for GitHub Pages) |
| `deno.json` | Modified (build:wasm task deletes wasm-pack .gitignore) |

### Alpine state removed from `default.html`
```
showScanner, showFlash, videoReady, noCamera, showQRModal
```
These are now server-driven HTML fragments returned by `/api/qr/status`. The `showFlash` toggle remains as a local Alpine `x-data` within the WASM-returned scanning UI (purely visual, appropriate for Alpine).

### Lessons learned

1. **`importScripts()` is blocked in module workers** — The worker runs as `{ type: 'module' }` for ES import of wasm-bindgen glue, but ZXing is a classic script. Fix: `fetch()` the script text and execute with `(0, eval)(text)` (indirect eval runs in global scope, defining the ZXing factory on `globalThis`).

2. **ZXing needs `locateFile` when loaded via eval** — After eval, ZXing resolves `zxing_reader.wasm` relative to the worker URL (`/assets/js/`), not the script's original location (`/assets/js-wasm/`). Fix: pass `locateFile: (file) => '/assets/js-wasm/${file}'` to the ZXing factory.

3. **wasm-pack silently breaks git tracking** — `wasm-pack` auto-generates a `.gitignore` containing `*` in its output directory, preventing the WASM package from being committed. Combined with root `.gitignore` excluding other build outputs, all Phase 1+2 assets were missing from GitHub Pages. Fix: cleaned `.gitignore` and added `rm -f .gitignore` to the `build:wasm` task.

4. **HTMX attributes in WASM-returned HTML bypass the WASM pipeline** — Buttons with `hx-get` in dynamically-inserted HTML fire real network fetches that hit Jekyll's 404 (or the SW relay, which adds latency). Fix: use `onclick` + `htmx.ajax()` calls instead — these go through the same direct JS path regardless of SW state.

5. **`innerHTML` doesn't execute `<script>` tags** — Both the dev fallback in `kipukas-api.js` and the QR_FOUND handler in `qr-camera.js` needed explicit script re-execution: clone each `<script>` with `document.createElement('script')` and replace the inert original.

6. **Dev fallback had three query string bugs** — Empty `{}` params from `htmx.ajax()` were truthy (dropping URL query strings), `url.search` already includes `?` (causing double `??`), and target resolution needed to check `evt.detail.target` before `hx-target` attribute lookup.

### Future: Remove ZXing dependency
- Replace ~2MB third-party ZXing WASM with Rust QR decoder compiled into `kipukas-server`
- Net reduction in download size despite adding QR functionality to the crate
- Concern: Rust QR decoder maturity vs. ZXing's proven production accuracy
- Decision deferred — ZXing works well for now

---

## Phase 3a: Card Grid Infinite Scroll (✅ Complete)

### Problem

The index page rendered all ~56 card buttons simultaneously in the DOM using Alpine.js `x-show` + `x-intersect` to simulate virtual DOM behavior. Every card had:
- An outer `<div>` with `x-intersect`/`x-intersect:leave` toggling an `inView*` state variable
- An inner `<a>` with a compound `x-show` expression checking filters, search regex, and `inView*` state
- A nested `x-data` binding with `window.innerWidth` for responsive `srcset` logic

This created ~110+ DOM elements on load, ~56 `IntersectionObserver` instances, and forced Alpine to evaluate every `x-show` expression on every filter/search change. The `loading="lazy"` attribute on images was insufficient because all `<img>` elements were already parsed by the browser. Performance would degrade linearly as cards were added post-launch.

### What was built

**Build Script (`scripts/build-card-catalog.ts`)**
- Reads all `_posts/*.html` front matter at build time
- Generates `kipukas-server/src/cards_generated.rs` — a static array of 56 `Card` structs
- Card metadata: slug, title, layout, img_name, img_alt, tags, genetic_disposition, motivation, habitat, url
- Cards sorted alphabetically by title (matching existing Jekyll `sort:'title'`)
- Integrated into build pipeline: `build:card-catalog` runs before `build:wasm`

**Rust WASM Route (`kipukas-server/src/routes/cards.rs`)**
- `GET /api/cards?page=0&per=12&all=true` — paginated, filtered card catalog
- Query parameters: `page`, `per`, `filter` (comma-separated), `search`, `all`
- Filter matching: layout, genetic_disposition, motivation, habitat (OR logic, matching Alpine behavior)
- Search: case-insensitive substring match on tags + slug + title
- Returns HTML fragments: card `<a>` elements + trailing sentinel `<div>` for next page
- Sentinel uses `hx-trigger="revealed"` for HTMX native infinite scroll
- 9 unit tests covering pagination, filtering, search, edge cases

**HTMX Infinite Scroll (`index.html`)**
- `#card-grid` div with `hx-get="/api/cards?page=0&per=12&all=true"` and `hx-trigger="load"`
- Initial load fetches first 12 cards from WASM + sentinel for page 1
- As user scrolls, each sentinel triggers fetch for next page, replacing itself with more cards + next sentinel
- Only ~12 cards exist in DOM at a time (plus sentinel), vs. ~110+ previously

**Filter/Search Integration**
- `window.kipukasRefreshCards()` — reads Alpine reactive state, builds query string, calls `htmx.ajax()`
- Filter checkboxes call `kipukasRefreshCards()` via `$nextTick()` on click
- Search input uses `@input.debounce.300ms` to trigger refresh
- Search toggle clears `searchQuery` on close, restoring "Show All" state
- Filter logic: `all=true` → show all; `search=X` → search mode; `filter=A,B` → category filter; no params → empty grid

**Responsive Images (Simplified)**
- Replaced Alpine `x-data` + `:srcset` with native `srcset` + `sizes` attributes
- `srcset="/assets/thumbnails/x1/{img} 160w, .../x2/{img} 320w, .../x5/{img} 800w"`
- `sizes="(min-width: 768px) 240px, 160px"` — browser picks optimal resolution natively
- Eliminates one Alpine reactive binding per card and one `resize` event listener per card

### Files created/modified

| File | Action |
|------|--------|
| `scripts/build-card-catalog.ts` | Created |
| `kipukas-server/src/cards_generated.rs` | Created (auto-generated) |
| `kipukas-server/src/routes/cards.rs` | Created |
| `kipukas-server/src/routes/mod.rs` | Modified (added cards module) |
| `kipukas-server/src/lib.rs` | Modified (added cards_generated module, /api/cards route) |
| `index.html` | Modified (replaced Alpine card grid with HTMX #card-grid + kipukasRefreshCards) |
| `_includes/filter.html` | Modified (added kipukasRefreshCards() calls, removed inView* resets) |
| `_includes/toolbar.html` | Modified (search button clears query on close + refreshes, search input debounced) |
| `deno.json` | Modified (added build:card-catalog task, integrated into build:wasm) |

### Alpine state removed from `index.html`
```
inView* (56 variables) — one per card, toggled by x-intersect
x-show compound expressions on ~110 elements
x-data with window.innerWidth per card (responsive srcset)
<template x-if="true"> wrapper
```

### Performance impact

| Metric | Before (Alpine) | After (HTMX + WASM) |
|--------|-----------------|----------------------|
| DOM elements on load | ~110+ | ~24 (12 cards × 2) |
| IntersectionObservers | ~56 | 1 (sentinel) |
| Alpine reactive bindings | ~170+ | 0 on card grid |
| Images parsed on load | All 56 | 12 (truly lazy) |
| Filter response | Re-evaluate all x-show | WASM returns only matching cards |
| Search response | Regex on every card | Rust substring match |

### Lessons learned

1. **Card catalog in WASM avoids runtime data fetching** — By generating a Rust source file from Jekyll front matter at build time, the card metadata lives in the WASM binary (~5KB overhead for 56 cards). No JSON fetch, no localStorage, no IndexedDB — just compiled-in data.

2. **HTMX `revealed` trigger is a native infinite scroll** — No custom JavaScript needed for the scroll detection. HTMX's built-in `revealed` trigger fires when the sentinel div enters the viewport, fetching the next page and replacing itself. The sentinel chain continues until the last page returns no sentinel.

3. **`kipukasRefreshCards()` bridges Alpine UI state to HTMX data fetching** — The function reads Alpine's reactive `filter` and `searchQuery` state, builds a URL, and calls `htmx.ajax()`. This preserves Alpine for UI chrome (checkbox states, search bar visibility) while routing data operations through WASM.

4. **Native `srcset` + `sizes` replaces Alpine responsive logic** — The browser's native image selection algorithm handles responsive thumbnails better than JavaScript `window.innerWidth` checks, without the overhead of Alpine reactive bindings and resize event listeners.

5. **Empty filters = empty grid is intentional UX** — When the user opens search mode (toggling `filter.all` to false), the grid goes blank, signaling "ready to search". Results appear as the user types. Closing search restores "Show All". This matches the previous Alpine behavior where no filter conditions being true resulted in an empty display.

---

## Phase 3b: Game State Migration

### Problem

Damage tracking, turn tracking, and game persistence currently use Alpine.js `$persist` plugin (localStorage). This works for single-player but doesn't synchronize across devices or players.

### Plan

1. **Create `/api/game/state` route** (GET/POST)
   - GET: Returns current game state as HTML fragments (damage counters, turn order, active effects)
   - POST: Updates state (damage dealt, turn advanced, item used)
   - State stored in WASM memory (with periodic serialization to localStorage via a dedicated route)

2. **Port damage tracker to Rust**
   - `kipukas-server/src/game/damage.rs` — HP tracking, damage calculation, status effects
   - Returns styled HTML fragments matching current Tailwind classes

3. **Port turn tracker to Rust**
   - `kipukas-server/src/game/turns.rs` — Turn order, phase management, timer logic
   - Returns HTML fragment with current turn state

4. **Create `/api/game/persist` route**
   - POST: Serialize current game state to JSON
   - Response includes `<script>` tag that writes to localStorage (or use HTMX `hx-on` to trigger save)
   - GET: Load saved state, return HTML fragments to restore UI

5. **State diffing for multiplayer prep**
   - Implement `serde::Serialize` + `serde::Deserialize` on game state structs
   - Add `/api/game/diff` route that returns a compact state diff (JSON)
   - This becomes the payload for WebRTC data channels in Phase 4

### Key dependencies to add
```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

### Migration path for `$persist` data
- Read existing localStorage values on first load
- POST them to `/api/game/import` to initialize WASM state
- From then on, WASM owns the state and persists via `/api/game/persist`

---

## Phase 4: WebRTC Multiplayer + Signaling Server

### Architecture

```
Player A's Browser                    Player B's Browser
┌─────────────────────┐              ┌─────────────────────┐
│  HTMX ←→ SW ←→ WASM │              │  HTMX ←→ SW ←→ WASM │
│  (local game server) │              │  (local game server) │
│         │            │              │            │         │
│    WebRTC Data Channel ←──────────→ WebRTC Data Channel   │
│         │            │              │            │         │
└─────────┼────────────┘              └────────────┼─────────┘
          │                                        │
          └──────── Signaling Server ──────────────┘
                   (WebSocket, tiny)
```

### Signaling Server

**Purpose:** Only brokers WebRTC connections. Does NOT process game logic.

**Implementation options:**
- **Minimal:** Deno Deploy edge function (~50 lines of WebSocket relay)
- **Self-hosted:** Tiny Axum server (reuses route patterns from kipukas-server)
- **Serverless:** Cloudflare Workers / AWS Lambda WebSocket API

**What it handles:**
1. Room creation (generate room code)
2. SDP offer/answer relay
3. ICE candidate exchange
4. Player presence (connected/disconnected)

**What it does NOT handle:**
- Game state
- Game logic
- Authentication (Phase 4 is trusted — players exchange room codes out-of-band using QRs with room info embedded)

### WebRTC Integration

1. **Create `/api/multiplayer/connect` route**
   - Returns HTML with connection UI (room code input, create/join buttons)
   - HTMX-driven: submit room code → connect → show status

2. **Create `/api/multiplayer/sync` route**
   - Called when a WebRTC data channel message arrives
   - Accepts opponent's state diff, merges into local WASM state
   - Returns updated HTML fragments (opponent's board, shared game state)

3. **Data channel protocol**
   - JSON state diffs generated by `/api/game/diff`
   - Each message is `{ seq: number, diff: GameStateDiff }`
   - Conflict resolution: last-writer-wins with sequence numbers (sufficient for turn-based)

4. **Game flow**
   ```
   Player A creates room → gets room code "ABCD"
   Player A shares "ABCD" with Player B (voice, text, QR, etc.)
   Player B joins room "ABCD"
   Signaling server brokers WebRTC connection
   Both players' WASM servers exchange initial state
   Each turn:
     1. Active player makes moves (local HTMX → local WASM)
     2. Local WASM generates state diff
     3. Diff sent via WebRTC data channel
     4. Opponent's WASM applies diff, updates their UI via HTMX
   ```

### Why this is "mostly decentralized"

- **Game logic:** 100% client-side (WASM)
- **Game state:** 100% client-side (WASM memory + localStorage)
- **Networking:** Peer-to-peer (WebRTC data channels)
- **Only centralized component:** Signaling server (stateless, <100 lines, needed only during connection setup)

After the WebRTC connection is established, the signaling server can go offline and the game continues. Players can even play over LAN without internet.

---

## Migration Strategy

### Coexistence Period

Alpine.js and HTMX will coexist throughout the migration. The pattern:

1. **Alpine manages UI chrome** — modals, dropdowns, visibility toggles, animations
2. **HTMX manages data** — fetching, computing, displaying game state
3. **Gradual Alpine removal** — as each feature migrates to HTMX, its Alpine `x-data` properties shrink

The goal is NOT to remove Alpine entirely. Alpine remains excellent for lightweight UI interactions. The goal is to move **data and logic** out of Alpine into the WASM server, using HTMX as the bridge.

### When to remove Alpine for a feature

A feature should migrate from Alpine to HTMX when:
- It involves **data processing** (typing calculations, damage math, state management)
- It has **complex state machines** (QR scanner flow)
- It needs to be **synchronized across players** (game state)

A feature should STAY in Alpine when:
- It's purely **visual** (show/hide toggle, animation, CSS class switching)
- It has **no data dependencies** (hamburger menu, modal open/close)

---

## Testing Strategy

### Rust unit tests
```bash
cd kipukas-server && cargo test
```
Every route handler and game logic module should have comprehensive unit tests. The Rust type system catches most bugs, but edge cases in matchup tables and damage calculations need explicit testing.

### Browser integration
- Open browser DevTools console
- `[kipukas-worker] WASM server initialized` confirms Rust WASM loaded
- `[kipukas-worker] ZXing WASM initialized` confirms QR decode capability
- `[qr-camera] Camera started, scanning at 2 fps` confirms camera + scan loop
- `[kipukas-api] No SW controller, routing directly:` confirms fallback path (dev only)
- Check Network tab for `/api/*` requests (should be intercepted by SW in production, absent in dev)

### Multiplayer testing (Phase 4)
- Two browser tabs on same machine (WebRTC works locally)
- Two devices on same network
- Two devices on different networks (requires TURN server for restrictive NATs)

---

## Open Questions

1. **TURN server for NAT traversal** — WebRTC peer connections fail behind symmetric NATs. Do we self-host a TURN server, use a free provider (e.g., Metered.ca free tier), or accept that some networks won't support multiplayer?

2. **Game state authority** — In the current plan, both players run independent WASM servers and sync via diffs. For competitive play, should one player be the "host" (authoritative state)? Or is mutual trust sufficient for a card game?

3. **Spectator mode** — Should room connections support observers who receive state diffs but can't send moves? This is architecturally simple (read-only data channel) but needs UI.

4. **Reconnection** — If a WebRTC connection drops mid-game, can players reconnect and resync? This requires the signaling server to maintain room state briefly, or players to exchange connection info again.

5. **Alpine.js long-term** — Should Alpine eventually be replaced entirely by HTMX + CSS-only interactions? Or is the hybrid approach the permanent architecture?
