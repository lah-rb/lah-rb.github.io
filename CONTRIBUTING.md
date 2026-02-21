# Kipukas — Contributing Guide

> Practices, architecture, and workflow for the Kipukas card game platform.

---

## Table of Contents

1. [Practices, Principles & Philosophies](#practices-principles--philosophies)
2. [Full Stack Architecture](#full-stack-architecture)
3. [Technology Stack & Licenses](#technology-stack--licenses)
4. [Development Workflow](#development-workflow)

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

Game logic has been ported from JavaScript to Rust, compiled to WASM. The Rust type system catches bugs at compile time that JavaScript hides. The crate currently has **106 unit tests** covering route handlers, game logic, matchup tables, and edge cases.

### Build-Time Code Generation

Card metadata is extracted from Jekyll `_posts/*.html` YAML front matter at build time by a Deno script (`scripts/build-card-catalog.ts`). This generates a Rust source file (`kipukas-server/src/cards_generated.rs`) containing a static array of `Card` structs compiled directly into the WASM binary. No runtime data fetching, no JSON loading, no IndexedDB — just compiled-in data.

### Two-Scope State Model

All data is tracked in two distinct scopes:

| Scope | Storage | Synced via WebRTC? | Examples |
|-------|---------|-------------------|----------|
| **Local User** | WASM `GameState` + localStorage | No | Damage tracking, turn alarms, card browsing |
| **Global (Room)** | WASM `RoomState` + WebRTC data channel | Yes | Fists combat submissions, combat results |

A feature defaults to local user state unless it explicitly requires cross-player visibility. Single-player behavior is completely unaffected by multiplayer code.

### Minimal Infrastructure

- **Hosting:** GitHub Pages (free, static)
- **Game logic:** In-browser WASM (zero server cost)
- **Multiplayer networking:** Peer-to-peer WebRTC
- **Signaling:** Deno Deploy free tier (stateless, <150 lines)
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

### Multiplayer Architecture

```
Player A's Browser                    Player B's Browser
┌─────────────────────-┐              ┌─────────────────────┐
│  HTMX ←→ SW ←→ WASM  │              │  HTMX ←→ SW ←→ WASM │
│  (local game server) │              │  (local game server)│
│         │            │              │            │        │
│   WebRTC Data Channel  ←──────────→  WebRTC Data Channel  │
└─────────┼────────────┘              └────────────┼────────┘
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

Currently 106 tests covering: route dispatch, type matchup tables, QR URL validation, card filtering/pagination, damage tracking, turn management, room state, and combat resolution.

**Browser integration checks** (DevTools console):

| Message | Confirms |
|---------|----------|
| `[kipukas-worker] WASM server initialized` | Rust WASM loaded in Web Worker |
| `[kipukas-worker] ZXing WASM initialized` | QR decode capability ready |
| `[kipukas-api] No SW controller, routing directly:` | Dev fallback active (expected during `jekyll serve`) |
| `[qr-camera] Camera started, scanning at 2 fps` | Camera + scan loop running |

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