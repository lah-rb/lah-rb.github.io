# Kipukas Development Workflow

> **Runtime:** [Deno](https://deno.land/) — all JavaScript/TypeScript tooling runs through Deno.
> No Node.js, npm, or package.json required.

---

## Prerequisites

| Tool | Install |
|------|---------|
| **Deno** ≥ 2.x | `curl -fsSL https://deno.land/install.sh \| sh` |
| **Ruby + Jekyll** | `gem install jekyll bundler` then `bundle install` in project root |
| **Rust + wasm-pack** | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` then `cargo install wasm-pack` |
| **WeasyPrint** *(optional, PDF only)* | `brew install weasyprint` or `pip install weasyprint` |

After installing Deno, restart your shell so `deno` is on your `PATH`.

---

## Project Structure

```
lah-rb.github.io/
├── deno.json                  # ← all deps, tasks, fmt & lint config
├── workbox-config.cjs         # Workbox injectManifest config (CJS for compat)
├── sw-src.js                  # Service worker source (Workbox replaces __WB_MANIFEST)
├── _config.yml                # Jekyll config
├── Gemfile                    # Ruby/Jekyll gems
│
├── assets/
│   ├── css/
│   │   ├── input.css          # Tailwind v4 source (CSS-first config)
│   │   └── output.css         # ← generated (gitignored)
│   ├── js/
│   │   ├── alpine.bundle.min.js  # ← generated (gitignored)
│   │   ├── pwa-update-handler.js  # PWA update UI
│   │   └── workbox/           # Vendored Workbox runtime libs
│   └── js-wasm/
│       ├── kipukas-server-pkg/ # ← generated WASM pkg (wasm-pack output)
│       └── ...                 # QR scanner WASM (third-party)
│
├── kipukas-server/             # Rust WASM "server" crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs              # handle_request() entry point + matchit router
│       ├── typing.rs           # Type matchup logic (ported from typing.js)
│       └── routes/
│           ├── mod.rs
│           ├── type_matchup.rs # /api/type-matchup → HTML fragment
│           └── qr.rs           # /api/qr/status, /api/qr/found → QR scanner UI fragments
│
├── scripts/
│   └── bundle-alpine.ts       # esbuild: Alpine.js + plugins → IIFE bundle
│
├── _layouts/
│   └── default.html           # References local Alpine bundle + Tailwind output
│
└── kipukas_rules_book/
    ├── deno.json               # Sub-project deps & tasks
    └── scripts/
        ├── build.ts            # Rules book → static HTML
        └── build-pdf.ts        # Rules book → PDF via WeasyPrint
```

---

## Quick Start

```bash
# First-time setup: vendor Workbox runtime libraries
deno task setup:workbox

# Full production build
deno task build

# Development (run in separate terminals, or use tmuxinator)
deno task dev:css          # Tailwind watch mode
jekyll serve --livereload  # Jekyll dev server at localhost:4000
```

Or use the tmuxinator config:

```bash
tmuxinator start kpksdev
```

---

## Available Tasks

All tasks are defined in `deno.json` and run via `deno task <name>`.

### Build Tasks

| Task | What it does |
|------|-------------|
| `deno task build` | **Full pipeline:** WASM → HTMX → CSS → Alpine → Rules → Jekyll → SW |
| `deno task build:wasm` | Compile Rust → WASM (`kipukas-server/ → assets/js-wasm/kipukas-server-pkg/`) |
| `deno task build:htmx` | Vendor HTMX from npm → `assets/js/htmx.min.js` |
| `deno task build:css` | Compile + minify Tailwind CSS v4 (`input.css → output.css`) |
| `deno task build:alpine` | Bundle Alpine.js + 5 plugins into one minified IIFE |
| `deno task build:sw` | Inject precache manifest into service worker (`sw-src.js → sw.js`) |
| `deno task build:rules` | Build the kipukas rules book (HTML + assets → `game_rules/`) |
| `deno task setup:workbox` | Vendor Workbox runtime libraries to `assets/js/workbox/` |

### Development Tasks

| Task | What it does |
|------|-------------|
| `deno task dev:css` | Tailwind CSS in watch mode (rebuilds on file changes) |

### Quality Tasks

| Task | What it does |
|------|-------------|
| `deno task fmt` | Format JS/TS/JSON files with Deno's built-in formatter |
| `deno task lint` | Lint JS/TS files with Deno's built-in linter |
| `deno task check` | Run both `fmt --check` and `lint` (CI-friendly) |

---

## Full Build Pipeline

The `deno task build` command runs these steps in order:

```
1. build:wasm       Rust → WASM (kipukas-server → assets/js-wasm/kipukas-server-pkg/)
2. build:htmx       Vendor HTMX → assets/js/htmx.min.js
3. build:css        Tailwind v4 → assets/css/output.css (minified)
4. build:alpine     Alpine.js + plugins → assets/js/alpine.bundle.min.js
5. build:rules      Rules book markdown → kipukas_rules_book/dist/
6. (move)           mv kipukas_rules_book/dist → game_rules/
7. jekyll build     Jekyll compiles everything → _site/
8. build:sw         Workbox injects precache manifest → sw.js
```

After step 8, the `_site/` directory is the deployable output.

---

## Dependency Management

All JavaScript dependencies are declared in `deno.json` under `"imports"`:

```jsonc
{
  "imports": {
    "alpinejs":               "npm:alpinejs@3.14.9",
    "@alpinejs/persist":      "npm:@alpinejs/persist@3.14.9",
    "@alpinejs/intersect":    "npm:@alpinejs/intersect@3.14.9",
    "@alpinejs/focus":        "npm:@alpinejs/focus@3.14.9",
    "@alpinejs/anchor":       "npm:@alpinejs/anchor@3.14.9",
    "@alpinejs/collapse":     "npm:@alpinejs/collapse@3.14.9",
    "@tailwindcss/cli":       "npm:@tailwindcss/cli@4",
    "tailwindcss":            "npm:tailwindcss@4",
    "@tailwindcss/typography": "npm:@tailwindcss/typography@0.5",
    "@tailwindcss/forms":     "npm:@tailwindcss/forms@0.5",
    "workbox-cli":            "npm:workbox-cli@7.3.0",
    "esbuild":                "npm:esbuild@0.25"
  }
}
```

To update a dependency, edit the version in `deno.json` and run the relevant build task.
Deno resolves and caches packages automatically — no `install` step required.

---

## Tailwind CSS v4

Tailwind is configured **CSS-first** in `assets/css/input.css` (no `tailwind.config.js`):

```css
@import 'tailwindcss';
@plugin '@tailwindcss/typography';
@plugin '@tailwindcss/forms';

@theme {
  --color-primary: #667eea;
  --color-secondary: #764ba2;
  /* ... custom theme tokens ... */
}
```

This approach is the Tailwind v4 standard — all customization lives in CSS.

---

## Alpine.js

Alpine.js and its plugins are bundled into a single file by `scripts/bundle-alpine.ts`:

- **Input:** `alpinejs` + `@alpinejs/persist`, `intersect`, `focus`, `anchor`, `collapse`
- **Output:** `assets/js/alpine.bundle.min.js` (~77 KB, minified IIFE)
- **Layout:** `_layouts/default.html` loads one `<script defer>` tag

To add/remove Alpine plugins, edit the import map in `deno.json` and the entry content in `scripts/bundle-alpine.ts`, then run `deno task build:alpine`.

---

## WASM Server (In-Browser API)

The `kipukas-server/` Rust crate compiles to WebAssembly and runs **entirely in the browser** as an in-page API server. It replaces client-side JavaScript utilities (like `typing.js`) with type-safe Rust.

### Architecture (Option C — SW + Web Worker Sidecar)

```
HTMX fetch("/api/type-matchup?atk=Brutal&def=Avian")
  → Service Worker intercepts /api/*
  → SW creates MessageChannel, posts to page
  → kipukas-api.js (page bridge) transfers port to Web Worker
  → kipukas-worker.js loads WASM, calls handle_request()
  → Rust processes request via matchit router, returns HTML fragment
  → Response flows back through MessageChannel to SW
  → SW returns real Response to HTMX
  → HTMX swaps HTML fragment into DOM
```

### Key Files

| File | Role |
|------|------|
| `kipukas-server/src/lib.rs` | WASM entry: `handle_request(method, path, query)` |
| `kipukas-server/src/typing.rs` | Type matchup logic (ported from `typing.js`) |
| `kipukas-server/src/routes/*.rs` | Route handlers returning HTML fragments |
| `assets/js/kipukas-worker.js` | Module Web Worker — loads WASM, processes requests |
| `assets/js/kipukas-api.js` | Page bridge — relays SW ↔ Web Worker via MessageChannel |
| `sw-src.js` (api route) | Intercepts `/api/*` fetches, relays to page |

### Building

```bash
# Requires: cargo install wasm-pack
deno task build:wasm   # wasm-pack build → assets/js-wasm/kipukas-server-pkg/
```

### HTMX

[HTMX](https://htmx.org/) (v2.0.4, ~50KB) enables server-driven HTML fragment swapping. Combined with the in-browser WASM server, HTMX attributes like `hx-get="/api/type-matchup"` trigger real fetches that the SW intercepts and routes to WASM — no custom JavaScript needed for UI updates.

Vendored via: `deno task build:htmx` → `assets/js/htmx.min.js`

---

## QR Scanner (Phase 2 — HTMX + WASM Migration)

Phase 2 migrated the QR scanner from a fragile Alpine.js state machine to HTMX + WASM server-driven HTML fragments. The scanner now follows the same architectural pattern established in Phase 1: HTMX requests → WASM routes → HTML fragment responses.

### Architecture

```
                            ┌─────────────────────────────────┐
                            │  kipukas-worker.js (Web Worker) │
                            │                                 │
  qr-camera.js ──QR_FRAME──→  ZXing WASM decodes pixels      │
       ↑                    │       ↓ decoded URL             │
       │                    │  Rust WASM formats HTML         │
       │     ←──QR_FOUND────│  (/api/qr/found route)         │
       │                    └─────────────────────────────────┘
       ↓
  DOM swap (redirect to card page)

  Toolbar button → kipukasQR.toggle()
       → htmx.ajax('/api/qr/status?action=open&privacy=...')
       → WASM returns privacy modal or scanning UI
       → HTML fragment swapped into #qr-container
```

### Key Files

| File | Role |
|------|------|
| `kipukas-server/src/routes/qr.rs` | `/api/qr/status` (UI state) and `/api/qr/found` (URL validation) |
| `assets/js/qr-camera.js` | Camera start/stop, frame capture loop → Web Worker |
| `assets/js/kipukas-worker.js` | Loads ZXing WASM + Rust WASM; decodes QR frames, formats results |
| `_includes/qr_scanner.html` | Toolbar button + `#qr-container` target div |

### QR Scanner State Machine

All state transitions are driven by WASM-returned HTML fragments:

| Action | Route | Returns |
|--------|-------|---------|
| Open scanner (no privacy) | `/api/qr/status?action=open&privacy=false` | Privacy modal with Accept/Cancel buttons |
| Open scanner (privacy OK) | `/api/qr/status?action=open&privacy=true` | Scanning UI (`<video>`, close/flash buttons, `<script>kipukasQR.start()</script>`) |
| Close scanner | `/api/qr/status?action=close` | Empty string (clears container) |
| Camera error | `/api/qr/status?action=error&msg=...` | Error message fragment |
| QR decoded | `/api/qr/found?url=...` | "QR Found!" + redirect `<script>` |

### ZXing Integration

ZXing WASM (~2MB) handles the actual barcode decoding. It runs alongside the Rust WASM in the same Web Worker:

- **Loading:** `fetch()` + indirect `eval()` (because module workers don't support `importScripts()`)
- **Path resolution:** `locateFile` callback points ZXing to `/assets/js-wasm/zxing_reader.wasm`
- **Decode loop:** `qr-camera.js` sends raw RGBA pixel buffers via `postMessage` (zero-copy transfer) at 2 fps
- **Result flow:** Worker decodes → calls Rust `/api/qr/found` for URL validation → posts `QR_FOUND` back to main thread

### Difficulties Overcome

1. **`importScripts()` blocked in module workers** — The worker runs as `{ type: 'module' }` for ES import of the wasm-bindgen glue, but ZXing is a classic script. `importScripts()` is disallowed in module workers. Fix: `fetch()` the script text and execute with `(0, eval)(text)` (indirect eval runs in global scope).

2. **ZXing WASM file path resolution** — After loading via eval, ZXing looked for `zxing_reader.wasm` relative to the worker's URL (`/assets/js/`) instead of where the file actually lives (`/assets/js-wasm/`). Fix: pass `locateFile: (file) => '/assets/js-wasm/${file}'` to the ZXing factory.

3. **Build outputs not tracked by git** — `wasm-pack` auto-generates a `.gitignore` containing `*` in its output directory, which silently prevented the WASM package from being committed. Combined with the root `.gitignore` excluding `alpine.bundle.min.js` and `htmx.min.js`, all Phase 1+2 build outputs were missing from GitHub Pages. Fix: cleaned `.gitignore`, and added `rm -f .gitignore` to the `build:wasm` task so wasm-pack's gitignore is deleted on every build.

4. **HTMX attributes in WASM-returned HTML don't fire through the SW relay** — Buttons returned by WASM routes with `hx-get` attributes would fire real network fetches (bypassing the WASM pipeline) in production where the SW relay is active. Fix: use `onclick` + `htmx.ajax()` calls instead, which go through the same direct JS path regardless of SW state.

5. **Inline `<script>` tags not executed after `innerHTML` swap** — Both `kipukas-api.js` (dev fallback) and `qr-camera.js` (QR_FOUND handler) needed explicit script re-execution: clone each `<script>` element with `document.createElement('script')` and replace the inert original.

6. **Dev fallback query string handling** — Three bugs in `kipukas-api.js`: empty `{}` params from `htmx.ajax()` were truthy (dropping URL query strings), `url.search` already includes the `?` prefix (causing double `??`), and the fallback target resolution needed to check `evt.detail.target` first (set by `htmx.ajax`) before falling back to `hx-target` attribute lookup.

---

## Service Worker & PWA

- **Source:** `sw-src.js` — contains caching strategies + `self.__WB_MANIFEST` placeholder
- **Build:** `deno task build:sw` uses Workbox CLI to inject a precache manifest
- **Runtime:** Workbox libraries are vendored locally at `assets/js/workbox/workbox-v7.3.0/`
- **Update UI:** `assets/js/pwa-update-handler.js` shows a toast when a new SW is available

The workbox config is in `workbox-config.cjs` (CommonJS format required by workbox-cli).

---

## Code Quality

### Formatting

Deno's built-in formatter handles JS, TS, and JSON:

```bash
deno task fmt         # format files in-place
deno task check       # check formatting + lint (CI mode)
```

Configuration (in `deno.json`):
- **Line width:** 100
- **Indent:** 2 spaces
- **Quotes:** single
- **Scope:** `scripts/`, `assets/js/`, `deno.json`
- **Excludes:** vendored workbox, generated bundles, `_site/`, `node_modules/`

### Linting

```bash
deno task lint        # lint with recommended rules
```

- **Rule set:** `recommended` (Deno's curated set)
- **Scope:** `scripts/`, `assets/js/`
- **Excludes:** `assets/js-wasm/` (third-party compiled WASM bindings), vendored workbox, generated bundles

---

## tmuxinator (Development Session)

The `.tmuxinator.yml` starts a 4-pane layout:

| Pane | Command |
|------|---------|
| 1 | `jekyll serve --livereload --watch` (dev server at `:4000`) |
| 2 | `jekyll build --watch` (rebuilds on file changes) |
| 3 | `deno task dev:css` (Tailwind watch mode) |
| 4 | Opens browser to `http://localhost:4000` |

```bash
tmuxinator start kpksdev
```

---

## What Changed from Node.js

| Before (Node) | After (Deno) |
|---------------|-------------|
| `package.json` + `package-lock.json` | `deno.json` (imports + tasks) |
| `npm install` | Automatic (Deno caches on first run) |
| `tailwindcss` standalone binary (v3) | `@tailwindcss/cli` v4 via `deno run -A npm:...` |
| `tailwind.config.js` | CSS-first config in `input.css` |
| 6 Alpine CDN `<script>` tags | 1 local bundled `alpine.bundle.min.js` |
| Workbox loaded from Google CDN | Vendored locally in `assets/js/workbox/` |
| `kipukas_rules_book/build.js` (CJS) | `kipukas_rules_book/scripts/build.ts` (Deno TS) |
| `kipukas_rules_book/build-pdf.js` (CJS) | `kipukas_rules_book/scripts/build-pdf.ts` (Deno TS) |
| No formatter/linter configured | `deno fmt` + `deno lint` with recommended rules |
| `npm run <script>` | `deno task <name>` |
