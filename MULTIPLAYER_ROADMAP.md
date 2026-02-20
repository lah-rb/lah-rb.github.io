# Kipukas Multiplayer Roadmap

> **Status:** Phase 4 in progress (WebRTC multiplayer + fists combat)
> **Started:** February 2026
> **Architecture:** HTMX + In-Browser WASM Server + WebRTC

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

### Decision 6: Local User State vs Global (Room) State (Phase 4)

All WASM data is tracked in two distinct scopes:

| Scope | Storage | Synced via WebRTC? | Examples |
|-------|---------|-------------------|----------|
| **Local User** | `game::state::GameState` + localStorage | No | Damage tracking, turn alarms, card binder browsing |
| **Global (Room)** | `game::room::RoomState` + WebRTC data channel | Yes | Fists combat submissions, combat results |

**Why two scopes?** Every player keeps their own binder — damage to their cards, their alarms, their browsing state is private. But multiplayer interactions (fists combat) need both players to see the same data. Existing features (Phases 1–3b) remain **local user** state, preserving current single-player behavior. The fists combat tool is the first **global** feature, where both players submit choices and see a shared result.

**Key principle:** A feature defaults to local user state unless it explicitly requires cross-player visibility. This keeps the single-player experience completely unaffected by multiplayer code.

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

## Phase 3b: Game State Migration (✅ Complete)

### Problem

Damage tracking, turn tracking, and game persistence used Alpine.js `$persist` plugin (localStorage). Each card page had its own `$persist` key for damage state (`{cardName}_damage`), and the turn tracker had `$persist([])` for alarms. A global `clearDamage` token in the body `x-data` triggered all cards to reset via `$watch`. This approach:
- Could not synchronize across devices or players
- Required Alpine to evaluate `$watch` expressions on every card page
- Stored state as scattered localStorage keys with no unified schema
- Made the clear-all mechanism fragile (random token propagation via `$watch`)

### What was built

**Build Script Extension (`scripts/build-card-catalog.ts`)**
- Switched from simple regex parser to `@std/yaml` for proper YAML parsing
- Extended to extract game data: `keal_means` (nested name → {genetics, count}), `injury_tolerance`, `movement`, `die`, `brawl_sequence`
- Generates `KealMeans` struct alongside `Card` struct with static genetics arrays
- 51 keal means across 30 cards compiled into the WASM binary

**Game State Module (`kipukas-server/src/game/`)**
- `game/state.rs` — `GameState`, `CardDamageState`, `Alarm` structs with `thread_local!` + `RefCell` storage. All structs derive `Serialize`/`Deserialize` for multiplayer prep
- `game/damage.rs` — Per-card keal damage tracking: toggle slot, toggle wasted, clear card, clear all. Renders full damage tracker HTML with checkbox `onclick` → `htmx.ajax('POST', ...)` pattern
- `game/turns.rs` — Diel cycle alarm system: add alarm, tick (decrement + expire), remove, toggle visibility. Renders alarm list + timer creation panel HTML
- Custom `Default` for `GameState` (show_alarms defaults to `true`)

**Route Handlers (`kipukas-server/src/routes/game.rs`)**
- `GET /api/game/damage?card={slug}` — Render keal damage tracker for a card
- `POST /api/game/damage` — Toggle slot, toggle wasted, clear card, or clear all
- `GET /api/game/turns` — Render timer creation form or alarm list (`?display=alarms`)
- `POST /api/game/turns` — Add alarm, tick, remove, or toggle visibility
- `GET /api/game/state` — Full game state as JSON (multiplayer prep)
- `POST /api/game/persist` — Serialize to `<script>` that writes localStorage
- `POST /api/game/import` — Accept JSON body, hydrate WASM state
- URL-encoded form body parsing for POST parameters
- 88 total unit tests (up from 66) covering all game logic + route handlers

**POST Method Support**
- `lib.rs` router now matches `(*matched.value, method)` tuple for GET/POST dispatch
- `body` parameter no longer ignored — passed to POST route handlers
- SW already had POST body reading (added in Phase 2 prep) — no SW changes needed

**Bridge Updates (`assets/js/kipukas-api.js`)**
- Dev fallback now handles POST: serializes `parameters` as URL-encoded form body
- Alpine `$persist` migration: scans localStorage for `_x_*_damage` and `_x_alarms` keys, builds `GameState` JSON, POSTs to `/api/game/import`
- State persistence: `beforeunload` handler POSTs to `/api/game/persist`
- State restore: on load, reads `kipukas_game_state` from localStorage and POSTs to `/api/game/import`

**HTML Template Updates**
- `keal_damage_tracker.html` — Replaced ~60 lines of Alpine `$persist` + Jekyll template logic with 5-line HTMX container: `hx-get="/api/game/damage?card={slug}"` + `hx-trigger="load"`
- `turn_tracker.html` — Replaced Alpine `$persist([])` alarms with HTMX-loaded panel + floating alarm display (`#turn-alarms`). Toggle visibility stays Alpine (purely visual)
- `toolbar.html` — Moved `showKealModal` to local `x-data` scope. Clear-all button now calls `htmx.ajax('POST', '/api/game/damage', {values: {action: 'clear_all'}, ...})`. Removed `makeid()` script
- `default.html` — Removed `clearDamage: $persist('U7G789Rc')` and `showKealModal: false` from body `x-data`

### Files created/modified

| File | Action |
|------|--------|
| `scripts/build-card-catalog.ts` | Modified (YAML parsing, game data fields) |
| `kipukas-server/src/cards_generated.rs` | Modified (auto-generated — KealMeans + extended Card) |
| `kipukas-server/Cargo.toml` | Modified (added serde, serde_json) |
| `kipukas-server/src/game/mod.rs` | Created |
| `kipukas-server/src/game/state.rs` | Created |
| `kipukas-server/src/game/damage.rs` | Created |
| `kipukas-server/src/game/turns.rs` | Created |
| `kipukas-server/src/routes/game.rs` | Created |
| `kipukas-server/src/routes/mod.rs` | Modified (added game module) |
| `kipukas-server/src/lib.rs` | Modified (game module, /api/game/* routes, POST dispatch) |
| `assets/js/kipukas-api.js` | Modified (POST body handling, migration, persist/restore) |
| `_includes/keal_damage_tracker.html` | Modified (replaced Alpine with HTMX) |
| `_includes/turn_tracker.html` | Modified (replaced Alpine $persist with HTMX) |
| `_includes/toolbar.html` | Modified (local showKealModal, HTMX clear-all) |
| `_layouts/default.html` | Modified (removed clearDamage, showKealModal) |

### Alpine state removed from `default.html`
```
clearDamage: $persist('U7G789Rc')   — WASM game state clear_all route
showKealModal: false                — moved to local x-data in toolbar
```

### Alpine state removed from card pages
```
{cardName}_damage: $persist({...})  — WASM per-card damage state
{cardName}_clear: $persist([])      — eliminated (was part of clear mechanism)
alarms: $persist([])                — WASM turn state
showAlarms: $persist(true)          — WASM turn state
```

### Alpine state kept
```
showTurnTracker: false    — purely visual toggle (show/hide panel)
turnsToAlarm: 1           — range slider local state (sent to WASM on submit)
showKealModal (relocated) — modal open/close (purely visual, local x-data)
```

### WASM binary size

| Phase | Size | Delta |
|-------|------|-------|
| Phase 1 (typing + router) | 69KB | — |
| Phase 3a (+ cards) | ~72KB | +3KB |
| Phase 3b (+ serde + game state) | 183KB | +111KB |

The increase is almost entirely from `serde_json` (JSON parsing/serialization). This is a one-time cost that enables state persistence and will power WebRTC state sync in Phase 4.

### Lessons learned

1. **`thread_local!` + `RefCell` gives safe mutable state in WASM** — Since the WASM module runs in a single Web Worker thread, `thread_local!` provides safe global state without `unsafe`. The `RefCell` borrow checker prevents concurrent access at runtime, though in practice the single-threaded worker never triggers it.

2. **Custom `Default` needed for non-zero defaults** — Rust's derived `Default` sets `bool` to `false`, but `show_alarms` should default to `true`. A manual `Default` implementation was required to avoid test failures where `reset_state()` would set `show_alarms: false`.

3. **`build-card-catalog.ts` needed proper YAML parsing** — The Phase 3a simple regex parser couldn't handle nested YAML structures like `keal_means`. Switching to `@std/yaml` (Deno standard library) cleanly parses the nested `name → {genetics: [], count: N}` structure.

4. **POST body handling required changes at three layers** — The dev fallback in `kipukas-api.js` needed to serialize `parameters` as URL-encoded body (not query string) for POST requests. The SW already handled this from Phase 2 prep. The WASM router needed tuple matching `(route, method)` instead of nested `if method ==` checks.

5. **State persistence via `beforeunload` + restore on load** — A simple pattern: serialize WASM state to localStorage on page unload, restore it on page load. This bridges the gap between WASM's in-memory state (lost on page reload) and the persistent storage users expect. The migration script handles the one-time transition from Alpine `$persist` keys to the unified `kipukas_game_state` JSON.

6. **`serde_json` adds ~111KB to WASM but enables multiplayer** — The size increase is significant relative to the base binary but still small in absolute terms (183KB total). This is a strategic investment: the same serialization powers localStorage persistence now AND will power WebRTC state diffs in Phase 4.

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

## Resolved Questions (from Phase 4 planning)

1. **TURN server for NAT traversal** — **Deferred.** Symmetric NAT setups are uncommon in the target audience (casual card game players on home/mobile networks). STUN-only (Google's free `stun:stun.l.google.com:19302`) is sufficient for getting the basics working. A TURN server can be added later if real-world testing reveals connectivity issues.

2. **Game state authority** — **Mutual trust is sufficient** for now. Both players run independent WASM servers and sync fists combat submissions via the data channel. For a card game played between friends, there's no strong incentive to cheat. Authoritative host mode can be revisited if competitive ranked play is added.

3. **Spectator mode** — **Not required** for now. Rooms are limited to 2 peers. Spectator support is architecturally simple (read-only data channel) and can be added in a future phase if needed.

4. **Reconnection** — **Players re-exchange connection info.** The signaling server is stateless — it does not persist room state. If the WebRTC connection drops, players disconnect and rejoin by sharing the room code again. This keeps the signaling server simple and avoids stale room state management.

---

## Phase 4: What Was Built (🚧 In Progress)

### Architectural Decision: Local User State vs Global (Room) State

See **Decision 6** above. All existing features (damage, turns, browsing) remain **local user** state. The new fists combat tool is the first **global** feature — both players' submissions are synced via WebRTC and the result is displayed on both screens.

### What was built

**Signaling Server (`signaling-server/main.ts`)**
- ~130 lines of Deno WebSocket relay
- Room creation with 4-character codes (excludes confusable chars 0/O, 1/I)
- SDP offer/answer relay + ICE candidate relay
- Peer presence tracking (join, leave notifications)
- Rooms auto-cleaned when both peers disconnect
- Deployable to Deno Deploy (free tier) or run locally: `deno task dev`

**Room State Module (`kipukas-server/src/game/room.rs`)**
- `RoomState` struct: room_code, room_name, connected flag, `FistsCombat` state
- `FistsCombat` struct: local + remote `FistsSubmission` (role, card slug, keal index)
- `CombatRole` enum: `Attacking` / `Defending` — derives `Serialize`/`Deserialize`
- `attacker()` / `defender()` methods: look up by role across local+remote submissions
- `is_complete()`: true when both local and remote have submitted
- Separate `thread_local!` + `RefCell` storage from game state (room = global, game = local)
- JSON export for WebRTC transmission
- 6 unit tests

**Room Route Handlers (`kipukas-server/src/routes/room.rs`)**
- `GET /api/room/status` — Connected/disconnected UI with create/join forms
- `POST /api/room/create` — Store room code + name
- `POST /api/room/join` — Set connected, store code
- `POST /api/room/connected` — Mark WebRTC peer as connected
- `POST /api/room/disconnect` — Reset room state
- `GET /api/room/fists?card={slug}` — Fists combat form: role selection (attacking/defending) + keal means picker from card data
- `POST /api/room/fists` — Store local submission, trigger WebRTC send via inline script, show waiting/result
- `POST /api/room/fists/sync` — Accept remote peer's JSON submission, store and check completeness
- `GET /api/room/fists/poll` — Polling endpoint for waiting players (HTMX `hx-trigger="every 2s"`)
- `POST /api/room/fists/reset` — Clear both submissions for next round
- `GET /api/room/state` — Full room state as JSON
- Combat result rendering: attacker/defender cards, keal means genetics, archetype matchup via `typing.rs`, die modifier calculation with motivation notes
- 10 unit tests

**JavaScript Multiplayer Module (`assets/js/kipukas-multiplayer.js`)**
- WebSocket connection to signaling server (auto-detects production vs. local dev URL)
- RTCPeerConnection setup: initiator creates data channel + SDP offer, responder waits for data channel
- ICE candidate exchange via signaling relay
- Data channel protocol: `fists_submission` (sync combat choices) and `fists_reset` messages
- `postToWasm()` / `postToWasmWithCallback()` — direct Worker messaging for room state updates
- `execScripts()` — re-execute inline `<script>` tags after innerHTML swap (same pattern as Phase 2)
- Public API on `window.kipukasMultiplayer`: `createRoom()`, `joinRoom()`, `disconnect()`, `submitFists()`, `sendFists()`, `isConnected()`

**UI Integration (`_includes/multiplayer.html`)**
- WiFi-signal icon button in toolbar (local Alpine `showMultiplayer` toggle)
- Anchored dropdown panel with `#room-status` (HTMX-loaded on open) + `#fists-container` (card-context-aware)
- Card pages pass `card_slug` to the include — fists form auto-loads that card's keal means
- Index page gets `multiplayer_home` (no card context — fists shows guidance message)

**Fists Combat Flow**
```
1. Both players connect to the same room via signaling server
2. WebRTC data channel established (peer-to-peer)
3. Each player navigates to a card page and opens the multiplayer panel
4. Each player selects Attacking or Defending, then picks a keal means
5. Player clicks "Lock In Choice" → local submission stored in WASM, sent to peer via data channel
6. Peer's WASM receives submission via /api/room/fists/sync, stores as remote
7. When both submitted: combat result rendered showing:
   - Attacker card name, keal means, archetypes, die
   - Defender card name, keal means, archetypes, die
   - Attack die modifier (computed via typing.rs matchup engine)
   - Motivation modifiers (if applicable)
8. "New Round" button resets both submissions
```

### 10 `/api/room/*` routes added to router

| Route | Method | Purpose |
|-------|--------|---------|
| `/api/room/status` | GET | Room connection status panel |
| `/api/room/create` | POST | Store room code from signaling |
| `/api/room/join` | POST | Join room, mark connected |
| `/api/room/connected` | POST | Confirm WebRTC connected |
| `/api/room/disconnect` | POST | Reset room state |
| `/api/room/fists` | GET | Fists combat form for card |
| `/api/room/fists` | POST | Submit local combat choice |
| `/api/room/fists/sync` | POST | Receive remote peer's choice |
| `/api/room/fists/poll` | GET | Poll for combat completion |
| `/api/room/fists/reset` | POST | Reset for next round |
| `/api/room/state` | GET | Full room state JSON |

### Files created/modified

| File | Action |
|------|--------|
| `signaling-server/main.ts` | Created |
| `signaling-server/deno.json` | Created |
| `kipukas-server/src/game/room.rs` | Created |
| `kipukas-server/src/game/mod.rs` | Modified (added room module) |
| `kipukas-server/src/routes/room.rs` | Created |
| `kipukas-server/src/routes/mod.rs` | Modified (added room module) |
| `kipukas-server/src/lib.rs` | Modified (10 new /api/room/* routes) |
| `assets/js/kipukas-multiplayer.js` | Created |
| `_includes/multiplayer.html` | Created |
| `_includes/toolbar.html` | Modified (multiplayer + multiplayer_home includes) |
| `_layouts/card.html` | Modified (added multiplayer=true to toolbar) |
| `_layouts/default.html` | Modified (added kipukas-multiplayer.js script) |
| `index.html` | Modified (added multiplayer_home=true to toolbar) |
| `_config.yml` | Modified (exclude signaling-server/, MULTIPLAYER_ROADMAP.md) |

### Test count

| Phase | Tests | Delta |
|-------|-------|-------|
| Phase 3b | 88 | — |
| Phase 4 | 106 | +18 (6 room state + 10 room routes + 2 integration) |

### Remaining work

- [ ] Deploy signaling server to Deno Deploy
- [ ] Browser integration testing (two tabs, two devices)
- [ ] Handle edge cases: both players pick same role, cards with no keal means on both sides
- [ ] QR code support for room codes (embed room code in QR for easy sharing)
- [ ] localStorage persist/restore for room state across page reloads

## Open Questions

1. **Additional global features** — Fists combat is the first global feature. What other game mechanics should become global? Turn tracking could be shared so both players see the same diel cycle.

2. **Room persistence** — Currently room state is lost on page reload. Should room connection info be persisted to localStorage so players can auto-reconnect? This would require the signaling server to support reconnection to existing rooms.
