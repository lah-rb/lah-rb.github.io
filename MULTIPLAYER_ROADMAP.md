# Kipukas Multiplayer Roadmap

> **Status:** Phase 2 in progress (QR scanner migration)
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

## Phase 2: QR Scanner Migration

### Problem

The current QR scanner flow uses Alpine.js with complex state management (`showScanner`, `showFlash`, `videoReady`, `noCamera`, `showQRModal`) spread across `_layouts/default.html` and `_includes/qr_scanner.html`. It relies on the third-party ZXing WASM library loaded via a separate script tag. The flow has been historically brittle and recently broke.

### Plan

1. **Create `/api/qr/decode` route** in the Rust crate
   - Accept POST with base64 image data or camera frame
   - Use existing ZXing binary
   - Return decoded URL as HTML fragment (with navigation link)

2. **Create `/api/qr/status` route** for camera state
   - Returns HTML fragments for different states: requesting permission, active, error, no camera

3. **Replace Alpine state machine with HTMX**
   - Camera feed stays as a `<video>` element (browser API, can't be WASM'd)
   - Frame capture: periodic `hx-trigger="every 500ms"` POST to `/api/qr/decode`
   - State transitions driven by server responses (HTMX swaps different UI states)

4. **Future: Explor Remove ZXing dependency**
   - Replace ~2MB third-party WASM with Rust QR decoder compiled into `kipukas-server`
   - Net reduction in download size despite adding QR functionality to the crate
   - Project manager has concerns about rust QR decoder maturity where ZXing is fast and accurate in production

### Key files to modify
- `_includes/qr_scanner.html` — Replace Alpine with HTMX attributes
- `kipukas-server/src/routes/qr.rs` — New route module
- `_layouts/default.html` — Remove ZXing script tag, simplify body `x-data`

### Alpine state to remove from `default.html`
```
showScanner, showFlash, videoReady, noCamera, showQRModal
```
These all become server-driven HTML fragments.

---

## Phase 3: Game State Migration

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

## Size Budget

| Component | Current | After Phase 4 (est.) |
|-----------|---------|---------------------|
| HTMX | — | ~50 KB |
| Alpine.js | ~77 KB | ~77 KB (kept for UI) |
| WASM binary | — | ~150 KB (with QR + game logic) |
| ZXing WASM | ~2 MB | 0 (possibly removed) |
| typing.js | ~8 KB | 0 (removed) |
| **Net change** | | **~-1.7 MB** |

The migration should result in a **smaller** total download despite adding multiplayer capabilities.

---

## Testing Strategy

### Rust unit tests
```bash
cd kipukas-server && cargo test
```
Every route handler and game logic module should have comprehensive unit tests. The Rust type system catches most bugs, but edge cases in matchup tables and damage calculations need explicit testing.

### Browser integration
- Open browser DevTools console
- `[kipukas-worker] WASM server initialized` confirms WASM loaded
- `[kipukas-api] No SW controller, routing directly:` confirms fallback path
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
