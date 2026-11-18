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
8. [Completed Sprint: Player Document & GameState → Yrs Consolidation](#completed-sprint-player-document--gamestate--yrs-consolidation)

---

## Practices, Principles & Philosophies

### Offline-First / PWA-First

The site works without internet after the first load. Workbox **injectManifest** mode gives full control over the service worker lifecycle. Updates use a user-controlled flow — a toast notification appears when new content is deployed, and the user chooses when to apply it. No surprise reloads.

### Decentralized Architecture

Game logic runs **100% client-side** in WebAssembly. There is no backend server processing game state. The only hosted component is a small WebSocket relay (a Cloudflare Worker, `signaling-server/`) that forwards game messages between two players. It never inspects message payloads — it simply passes them to the other player in the room. What it stores is who is in each room (a room code, a room name and two player ids), never game state.

### HTMX Over SPA Frameworks

Instead of React, Vue, or Svelte, the project uses **HTMX** to add dynamic behavior to server-rendered HTML. The "server" happens to be a Rust WASM module running in a Web Worker inside the browser — but HTMX doesn't know or care. This fits naturally with Jekyll's static HTML model: just add `hx-*` attributes to markup.

### Alpine.js + HTMX: DOM Residency Determines the Tool

Alpine.js and HTMX coexist throughout the codebase. The guiding principle is **DOM residency** — whether content always lives in the DOM or comes and goes:

| Layer | Technology | Examples |
|-------|-----------|----------|
| **Always in DOM** (reactive appearance) | Alpine.js | Hamburger menu, visibility toggles, CSS class swaps, animations, transition states |
| **Comes and goes from DOM** (fetched on demand) | HTMX → Rust WASM | The room dialog's contents (create, join, leave), QR Scanner interface. Being replaced by engine-owned views |
| **Showing game state** (always in the DOM, or inside a dialog) | Engine-owned view (Rust renders, page patches) | Damage tracker, turn timer, fists in a room, affinity panel, type matchup calculator, binder, hand page — see [Pattern 13](#pattern-13-engine-owned-views) |
| **Game state authority** | Rust WASM | All game logic, damage state, turn tracking, combat resolution, type matchups |

A feature belongs to HTMX when its content doesn't always need to live in the DOM — modals, long scrollable lists, tool panels that are hidden most of the time. HTMX fetches that content from Rust and swaps it in on demand. A feature stays in Alpine when it's an always-present DOM element that just needs reactive class/style changes. Rust is the single source of truth for all game state — anytime a component needs to know game state, it consults Rust directly via HTMX or the Worker messaging API.

### Type Safety via Rust

Game logic has been ported from JavaScript to Rust, compiled to WASM. The Rust type system catches bugs at compile time that JavaScript hides. The crate currently has **over 100 unit tests** covering route handlers, game logic, matchup tables, combat outcomes, and edge cases.

### The Cards Are Compiled into the Engine

Every card is one file in `cards/`, named after the card's address (`/timebattle/` is `cards/timebattle.html`), holding YAML front matter and nothing else. The engine reads those files when it is compiled: its build script (`kipukas-server/build.rs`) turns them into a static array of `Card` structs (`kipukas-server/src/catalog.rs`). No runtime data fetching, no JSON loading, no IndexedDB — just compiled-in data.

Cargo runs the build script again whenever a card file changes, so an engine is always compiled from the card files as they are, whether for the browser (`deno task build:wasm`) or for the site build (`card-pages`). There is no separate step and no generated source in the repository.

The catalog holds everything a card shows, not only what the game logic reads, so the engine can render a card by itself.

How a card file is read is in its own small crate, `kipukas-server/catalog/`:

- **The field decides the type.** A text field takes its value as written, whatever it looks like (`404`, `no` and `1e3` stay text). A number field must hold a whole number, a yes-or-no field `true` or `false`.
- **A card file that is wrong stops the build**, with every problem listed by file (and line): a missing `permalink`, `title`, `layout` or `img_name`; a file not named after its address; a short code that is not 4 characters, or missing on a card the binder shows; two cards with one address or one short code; a number, flag or `thumbnail` that is not one; YAML that does not parse. Nothing is skipped quietly.
- **Long text may be written the house way**: in quotes over several lines, with the later lines starting at the left edge. Strict YAML wants those lines indented, and the parser (`yaml-rust2`) holds to it, so the reader indents them itself before parsing. Indented or not, the value is the same.

Jekyll does not read the card files (`cards/` is excluded in `_config.yml`). Their front matter keeps its Jekyll key names: `layout` is the card's type, `permalink` its address, `redirect_from` its short code. `minted` is the day the card was added (`YYYY-MM-DD`). It is a record for people: nothing reads it, and a card's page carries no date. Jekyll needs nothing from the cards: even the number of cards in the binder's filter menu is the engine's to say.

### Card Pages Are Written by the Engine

Every card has a page at a fixed address (`/timebattle/`), a short address that redirects to it (`/yDLL`, the QR code printed on the card), and a sitemap entry. They are plain files, so the host serves them as they are, search engines and link previews read them without running scripts, and an address with no file is a real 404.

The engine writes those files. The same Rust code that renders a card in the browser is built natively and run after every Jekyll build:

| Part | Where |
|---|---|
| The card shell: the page every card is shown in (scripts, styles, toolbar, the frame around the art), with holes where the card goes | `card-shell.html`, rendered once by Jekyll |
| What goes in the holes: head tags, slug, art, the detail panel | `kipukas-server/src/site.rs` |
| Each card's preview picture for link shares | `kipukas-server/src/preview.rs` |
| The data file the service worker reads: the list of art fetched for offline play | `kipukas-server/src/site.rs` |
| Reading and writing the files, and keeping the preview pictures it has made | `kipukas-server/src/bin/card-pages.rs` |
| Running it after every Jekyll build, and keeping the shell out of the site | `_plugins/card_pages.rb` |

A hole is written `[[card.<name>]]`. Anything that differs between cards must come through a hole: Jekyll knows nothing about any one card. The build fails if the shell has a hole the engine does not know, lacks one it needs, or if a file that is not the card's page already exists at a card's address.

The panel in the served page is the engine's render with no player state. In the browser the panel is an engine-owned view (Pattern 13): the engine renders it again with the player's state and the difference is patched in. A crawler, a link preview and a browser whose scripts have not run yet all see the whole card, keal means included.

The site's not-found page is a card page too: `cards/404.html` is in the catalog as a hidden card and is written to `/404.html`.

`card-pages` also keeps Kippa's copy of the catalog, `kipukas-server/cards_catalog.json` (`src/kippa.rs`), up to date. That file is in the repository and is handed to Kippa by hand; it is not part of the site. It is rewritten only when the catalog changes, so commit it when it shows up as modified.

`site.rs`, `preview.rs` and `kippa.rs` are left out of the browser's engine (`#[cfg(not(target_arch = "wasm32"))]`).

#### Preview pictures

When a card's address is pasted into a chat or a post, the app showing it reads the page's head for a picture (`og:image`). Those apps read JPEG; the card art is JPEG XL only. So the build writes one JPEG per card, `/assets/previews/<slug>.jpg`, and each card page names its own.

- **What it shows:** the whole art, never cropped, scaled to fit a 1200 × 630 canvas (the shape link previews are shown in) and centred on a mat of the art's own average colour, darkened. Every preview is that size.
- **Who fetches it:** only the app building a link preview. It is named in a head tag, not shown on the page, and it matches no pattern in the service worker's precache list (`workbox-config.cjs`). Keep it that way: a visitor should never download previews.
- **When it is made:** `card-pages` reads the art from the built site (`_site/assets/images/`), so nothing is added to the source tree. Making a preview is slow next to everything else the build does, so each one is kept in Jekyll's cache folder (`.jekyll-cache/card-previews/`) under a name derived from the art's bytes and the recipe (`preview::cache_key`). A build makes only the previews that are not there: after new or changed art, after a change to the constants in `preview.rs`, or after the cache is emptied (`jekyll clean`). Making all of them takes about 8 seconds; with them cached, `card-pages` adds well under a second to a build.
- **Changing how previews look:** edit `preview.rs`. A change to its constants renames every preview by itself; for any other change to what is drawn, raise `RECIPE` so the old ones are not reused.

### Card Schema: Types as Compositions of Fields

What a card shows is defined once, in `kipukas-server/src/game/card_panel.rs`:

- **`Field`** is the complete set of fields a card can show (variation, movement, die rolls, combat, history, …).
- **A card type is a composition**, not a template of its own: an ordered list of reusable groups of rows. Every type opens with the identity group and closes with the provenance group; Character and Species share the combatant group. A row gives a field the label that type uses for it.
- **`CARD_TYPES`** lists every type. Its `name` is the `layout` in the card's file.

| To… | Do this |
|---|---|
| Add a field | Add it to the card file's front matter, to `Card` in `src/catalog.rs`, to the reader and writer in the `catalog/` crate (`parse_card`, `emit`), to `Field` (with how to read it), and to the group(s) that show it |
| Show an existing field on another type | Add a row to one of that type's groups |
| Retire a field | Delete its `Field` variant; the compiler lists every group that used it |
| Add or retire a type | Add or delete its entry in `CARD_TYPES` |

A row is optional by nature: a card with no text for a field does not get the row. The Trap type's "game effects" row is there for the one trap that has that text.

The tests in that file fail if a card has a `layout` with no type, if a type has no cards, or if a card file has text its type never shows (which is how Timebattle's unseen `play_style` was found).

This schema is the only description of what a card shows. The card pages, the hand view and the browser all render from it.

### Three-Store State Model

All data is tracked across three distinct stores. Rust is the single source of truth — components always consult Rust (via HTMX or Worker messaging) rather than reading state from JavaScript or the DOM:

| Scope | Store | Persistence | Synced? | Examples |
|-------|-------|-------------|---------|----------|
| **Player** (permanent) | `PLAYER_DOC` (yrs Doc) | `kipukas_player_doc` in localStorage (base64 binary) | Yes — opt-in cross-device sync via WebSocket relay (HMAC-authenticated) | Card damage, turn alarms, settings, affinity, loyalty |
| **Room** (who and where) | `RoomState` (RefCell) | `kipukas_room` in localStorage: `{v, code, name, creator, playerId, peerId, ts}`, forgotten after 12 h without use | No — each player's own | Room code and name, this player's id in the room, the opponent's id, whether the opponent is reachable |
| **Room** (shared game state) | `ROOM_DOC` (yrs Doc) | `kipukas_room_doc` in localStorage (base64), saved after every change | Yes — yrs sync protocol | Shared turn timers (`timers`, keyed by timer id) and fists combat (`fists`, one entry per round per player) |

A feature defaults to `PLAYER_DOC` unless it explicitly requires cross-player visibility. Anything both players must agree on goes in `ROOM_DOC`: it is the only thing that is synced, persisted across page loads and repaired after a dropped connection. `RoomState.fists` is just a cache of the current round for the renderers. Single-player behavior is completely unaffected by multiplayer code. The JavaScript persistence layer (kipukas-api.js) handles the localStorage read/write because WASM runs in a Web Worker that cannot access browser storage APIs directly — Rust owns the state, JS owns the I/O bridge.

### Minimal Infrastructure

- **Hosting:** GitHub Pages (free, static)
- **Game logic:** In-browser WASM (zero server cost)
- **Multiplayer networking:** WebSocket relay
- **Relay:** Cloudflare Workers free tier — one Worker plus one Durable Object per room (`signaling-server/`)
- **No database, no authentication, no paid services**

### Formatting & Linting

`deno fmt` and `deno lint` enforce consistent style on scripts and JavaScript assets. Run `deno task check` to verify both in a single command. Configuration lives in `deno.json` under `fmt` and `lint` keys. --PLEASE RUN deno fmt and deno lint/ deno lint --fix FOR CODE QUALITY-- --ATTEMPT TO FIX LINTING ERRORS AS THEY ARE FOUND--

---

## Full Stack Architecture

### Request Flow (Production)

```
User opens the room dialog, which has hx-get="/api/room/status"
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
   kipukas-worker.js runs WASM: handle_request("GET", "/api/room/status", "")
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
           └─── Relay (one object per room) ────────┘
                 (forwards game messages)
```

The relay server handles room management (create/join/rejoin) and message forwarding. It never inspects game message payloads — game logic stays 100% client-side in WASM.

**Every page change is a full page load**, which restarts the game engine worker and the socket. Multiplayer is built to make that harmless rather than to avoid it:

- All shared state is in `ROOM_DOC`, saved to localStorage after every change and restored before the first render of the next page (`POST /api/room/resume`).
- Whenever the two players (re)connect they exchange yrs state vectors, so anything either missed while away arrives then. Nothing depends on a one-off message being delivered.
- The socket is owned by one small state machine (`kipukas-link.js`) that retries for as long as the player is in a room.

### Key Files

| File | Role |
|------|------|
| `kipukas-server/src/lib.rs` | Engine entry point (`handle_request`) + route registration |
| `kipukas-server/wasm/` | The engine wrapped as the WASM module the browser loads. A crate of its own so link-time optimisation still runs (see the note in `kipukas-server/Cargo.toml`) |
| `kipukas-server/src/site.rs` | The card pages of the built site: head tags, filling the card shell, short-address pages, sitemap entries. Build time only |
| `kipukas-server/src/preview.rs` | A card's preview picture for link shares: the art scaled onto a 1200 × 630 canvas, as a JPEG. Build time only |
| `kipukas-server/src/bin/card-pages.rs` | The native program that writes the card pages and preview pictures into `_site/` |
| `card-shell.html` | The card shell: the one page Jekyll renders for all cards, with `[[card.…]]` holes |
| `_plugins/card_pages.rb` | Jekyll hook: runs `card-pages` after every build |
| `kipukas-server/src/routes/*.rs` | Route handlers (type matchup, QR, cards, game, room, player, shared utils) |
| `kipukas-server/src/routes/player.rs` | Player routes — affinity, loyalty, signed export/import, cross-device sync |
| `kipukas-server/src/game/player_doc.rs` | **Authoritative player data store** — yrs CRDT Doc for damage, alarms, settings, affinity, loyalty |
| `kipukas-server/src/game/crypto.rs` | HMAC-SHA256 signing/verification for tamper-resistant exports |
| `kipukas-server/src/game/*.rs` | Damage rendering, turn logic, room state, CRDT sync |
| `kipukas-server/src/game/crdt.rs` | The room document (`ROOM_DOC`): shared timers, persistence, sync handshake |
| `kipukas-server/src/game/fists.rs` | Fists combat in the room document — pure functions over a `Doc`, tested with two documents |
| `kipukas-server/src/page.rs` | What the engine asks of the page after a post: room-document updates to send to the other player, and views to draw again |
| `kipukas-server/src/game/card_panel.rs` | The card schema (fields, and card types as compositions of them) and the card detail panel renderer |
| `kipukas-server/src/catalog.rs` | The card catalog: `Card`, and the cards themselves, included from the build script's output |
| `kipukas-server/build.rs` | Build script: reads `cards/` into the catalog each time the engine is compiled |
| `kipukas-server/catalog/` | How a card file is read and checked, and how the catalog is written as Rust. Runs at build time only |
| `kipukas-server/src/kippa.rs` | The catalog as Kippa reads it (`cards_catalog.json`). Build time only |
| `kipukas-server/src/game/matchup.rs` | The type matchup calculator: what is picked, the boxes and the result |
| `kipukas-server/src/routes/hand.rs` | The hand page: a pane per card named in the address |
| `kipukas-server/src/clock.rs` | The time, for what the engine does by the clock (a finished timer fading, the day of an affinity declaration). Read at route handlers and passed down |
| `assets/js/kipukas-api.js` | Page bridge — SW relay + dev fallback + state persistence + sync broadcast hook. Also owns the relay address, room storage (`kipukasRoomSession`), the load-time room resume, and `Alpine.store('room')` |
| `assets/js/kipukas-worker.js` | Web Worker — loads WASM + ZXing, handles requests |
| `assets/js/kipukas-view.js` | Engine-owned views: draws `data-engine-view` elements from the engine when they are shown, sends taps and forms to it, patches the result in place |
| `assets/js/kipukas-multiplayer.js` | Multiplayer manager — creating, joining and leaving a room, what to send when the socket opens, and carrying room-document updates between the engine and the other player. It draws nothing of the game: timers and fists are the engine's views |
| `assets/js/kipukas-link.js` | The relay socket's state machine: one socket, one retry timer, backoff, heartbeat. No DOM access, so it runs under `deno test` |
| `tests/link_test.ts` | Tests for `kipukas-link.js`, including two clients through a local relay |
| `_includes/room_status.html` | Status pill shown on every page while in a room |
| `assets/js/kipukas-crypto.js` | AES-GCM encryption/decryption for signed exports (lazy-loaded) |
| `assets/js/kipukas-sync.js` | Cross-device sync module — signaling, HMAC auth, yrs handshake (lazy-loaded) |
| `assets/js/qr-camera.js` | Camera + ZXing QR scan loop |
| `sw-src.js` | Service worker source (Workbox injectManifest) |
| `signaling-server/src/worker.js` | The relay's entry point (Cloudflare Worker): hands each connection to its room |
| `signaling-server/src/room-object.js` | One room (Durable Object): sockets, storage, presence, expiry |
| `signaling-server/src/room.js` | The room rules (who may enter, taking over a place, leaving) as pure functions |
| `signaling-server/test/` | Tests for the rules and for the whole relay running locally |
| `signaling-server/wrangler.jsonc` | Relay deployment config: address, allowed sites, room lifetime |
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

> **⚠️ Superseded.** This pattern was replaced by Pattern 12 (Alpine Fire-and-Forget), which has itself been replaced by [Pattern 13: Engine-Owned Views](#pattern-13-engine-owned-views). The sentinel div + `@htmx:after-swap` approach had a CSS specificity bug: each per-button Alpine `x-data` with `:class` bindings conflicted with static Tailwind classes on the same element. Different browsers resolved the specificity race differently — Firefox-based browsers (Floorp, Trivalent) only changed the border color, not the fill. The HTMX swap after each click also caused a visual double-update. Pattern 12 eliminates the sentinel entirely — Alpine computes `allChecked()` reactively from its own state, and fire-and-forget replaces HTMX swaps for per-click mutations.

**Original pattern (for historical context):** WASM rendered a hidden sentinel div (`<div class="keal-all-checked hidden">`) when all keal means checkboxes were checked. Alpine's `x-effect` on the parent watched for this sentinel after each HTMX swap and toggled a CSS class (`.show-final-blows`) that made the `.final-blows-section` visible. Each button had its own `x-data="{ on: true/false }"` with `:class` bindings for visual state, plus `hx-post` for HTMX swaps on every click.

**Why it was replaced:** The per-button `x-data` `:class` bindings set both static Tailwind classes (e.g., `bg-white border-emerald-600`) and Alpine dynamic classes (e.g., `:class="on ? 'bg-red-600 border-red-600' : 'bg-white border-emerald-600'"`). When Alpine toggled `on`, both sets coexisted on the element, creating a CSS specificity race that different browsers resolved differently. Additionally, the double-update (Alpine toggle + HTMX swap) caused visual jank. See Pattern 12 for the correct approach.

### Pattern 3: Alpine × HTMX Coexistence

**The pattern:** Alpine manages UI chrome (modals, toggles, visibility). HTMX manages data (fetching, computing, displaying). They communicate via:

1. **Alpine → HTMX:** `htmx.ajax()` calls from Alpine event handlers (e.g., `@click="htmx.ajax('POST', ...)"`)
2. **HTMX → Alpine:** `x-effect` watching for DOM changes after HTMX swaps (sentinel pattern)
3. **Cross-component:** Custom DOM events (e.g., `document.dispatchEvent(new CustomEvent('close-multiplayer'))` listened by `@close-multiplayer.window="showMultiplayer = false"`)

**Alpine telling the engine:** where Alpine opens something whose state the engine holds, its handler says so with `kipukasViews.send(route, body)`, which posts to the engine and draws again the views of that route. The binder's search button is the example: `@click="showSearchBar = !showSearchBar; …; kipukasViews.send('/api/cards', showSearchBar ? 'searching=open' : 'searching=close')"`.

**When to use Alpine:** Elements that always live in the DOM but need reactive appearance changes — show/hide toggles, CSS class switching, animations, transition states. Alpine never fetches data or reads game state.

**When to use HTMX:** For nothing new. It still fetches the room dialog's contents and the QR scanner's status from Rust and swaps them in.

**When to use an engine-owned view:** Content that shows game state — the keal damage tracker, the solo turn timer, the affinity panel. Neither Alpine nor HTMX holds it: Rust renders it, a tap asks Rust for a change, and the page patches in the result. It may sit inside a dialog that Alpine opens and closes. New tools should be built this way; the tools still on HTMX (Pattern 11) are being moved over one at a time. See [Pattern 13](#pattern-13-engine-owned-views).

**When to consult Rust directly:** Any time a component needs game state, it asks Rust via HTMX (`hx-get`) or the Worker messaging API (`postToWasmWithCallback`). Components never read state from localStorage or JavaScript variables — Rust is the single source of truth.

### Pattern 4: x-effect for Modal Refresh

**The pattern:** The multiplayer modal uses `x-effect` to re-fetch `#room-status` from WASM every time the modal opens:

```html
x-effect="if (showMultiplayer) $nextTick(() => {
  htmx.ajax('GET', '/api/room/status', {target:'#room-status', swap:'innerHTML'});
})"
```

An engine-owned view (Pattern 13) needs none of this: it is drawn each time it comes to be displayed.

**Why it works:** `hx-trigger="load"` only fires on initial page load. When a user modifies state (e.g., marks damage) and reopens the modal, the stale HTML would be shown. The `x-effect` ensures fresh data every time the modal becomes visible.

**Why not just `hx-trigger="revealed"`?** The `revealed` trigger fires when an element enters the viewport via scrolling, not when it becomes visible via Alpine's `x-show`. Custom events or `x-effect` are the correct Alpine → HTMX bridge for modal visibility.

### Pattern 5: ~~WASM State → DOM Sync (refreshKealTracker)~~ → Superseded by Pattern 13

> **⚠️ Superseded.** Replaced by [Pattern 13: Engine-Owned Views](#pattern-13-engine-owned-views).

**Original pattern (for historical context):** The damage tracker's visible state lived in an Alpine scope (Pattern 12). When the engine marked damage by itself, after a lost round of fists, the page's copy was stale, so a helper (`refreshKealTracker`) fetched the state as JSON from the engine and wrote it into the existing Alpine scope with `Alpine.$data()`.

**Why it existed in that form:** replacing the tracker's `innerHTML` and calling `Alpine.initTree()` failed on Firefox, Trivalent, DuckDuckGo and Cromite: Alpine did not reliably discover the new `x-data` scopes after replacement. Writing into the existing scope avoided re-initialisation.

**Why it was replaced:** it was a repair for a problem the design created. Two holders of the same state (Alpine for display, the engine for truth) need something to reconcile them, and every new way of changing damage needed its own call to the repair. With the engine as the only holder there is nothing to reconcile.

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

**Why it works:** The WASM module runs in a single Web Worker thread. `thread_local!` provides safe global state without `unsafe`. The `RefCell` borrow checker prevents concurrent access at runtime, though in practice the single-threaded worker never triggers it. Each store has its own lifecycle: PLAYER_DOC persists to localStorage forever; RoomState and ROOM_DOC exist for as long as the player is in a room, are saved to localStorage, and are rebuilt from it on every page load.

### Pattern 9: WebSocket Relay Protocol

**The pattern:** Peers exchange JSON messages through the relay. Each game message is wrapped in `{ type: "relay", data: { type: "...", ... } }` for transport. The relay forwards `relay` messages to the other peer without inspection. The inner `data.type` field determines how the client processes the message:

| Message Type | Direction | Payload | Purpose |
|-------------|-----------|---------|---------|
| `hello` | Both → peer | `{ proto: 2, playerId }` | Sent when the opponent appears: this player's id in the room and the room-document format they speak |
| `yrs_sv` | Both → peer | `{ sv: base64 }` | Yrs CRDT state vector (sync handshake step 1) |
| `yrs_sv_reply` | Both → peer | `{ sv: base64 }` | Yrs CRDT state vector reply (sync handshake step 2) |
| `yrs_update` | Both → peer | `{ update: base64 }` | Yrs CRDT binary update (a change to timers or fists combat) |

There are no per-feature messages. A feature changes `ROOM_DOC` and the resulting update is relayed; the opponent applies it. If the update is lost (the relay drops messages for a player whose socket is gone, and a page change closes the socket), the state-vector handshake that runs on every `peer_joined` delivers it.

**Why JSON over binary:** With 56 cards and simple turn-based interactions, message frequency is ~1-2 per combat round. JSON is human-readable for debugging and trivially parsed. Binary would add complexity for negligible performance gain.

**Protocol version:** `hello.proto` is bumped when the room document's shape changes. Site updates are applied when each user accepts the update prompt, so two players can be on different copies. A player whose opponent sends no `hello` (an older copy) or a different `proto` sees "Opponent needs to update" in the status pill; timers and combat do not sync until both are current.

**Fists combat in the document:** the `fists` map holds one entry per round per player, keyed `<round>:<playerId>` and written only by that player (`game/fists.rs`).

- The current round is the highest round present. Starting a new round is writing an empty entry at `round + 1`, so cancelling never needs the opponent's cooperation, and two players doing it at once converge.
- Earlier rounds stay. A player who was away when a round ended still finds its outcome and applies it, even if the opponent has already moved on.
- "Did you win?" is recorded as `attacker_won` in the answering player's entry. Each player applies a finished round once on their own device (the defender's card takes the damage) and records `settled` in their entry, so a reload, a reconnect or both players answering cannot mark damage twice. Conflicting answers are shown as disputed and mark nothing.
- Closing the fists dialog only hides it. "Cancel round" in the waiting view starts a new round for both players.
- What a player has picked and not locked in is not in the document. It is that player's engine's alone (`FistsChoice` in `game/room.rs`); the opponent learns of a choice when it is locked in.
- Nothing polls. A player who is waiting sees the result when the opponent's update arrives: applying it asks for the dialog to be drawn again (see "Changes that must reach the other player" under Pattern 13).

**Shared timers in the document:** the `timers` map is keyed by a stable timer id that travels with the timer between a player's own timers and a room. Both players seed their own timers on create and join; the same timer held by both (each keeps a copy when a session ends) lands on one key and stays one timer.

**Connection lifecycle:** `kipukas-link.js` owns the socket: one socket and one retry timer at a time, capped exponential backoff that never gives up while the player is in a room, a connect timeout, a 20 s ping, and an immediate retry when the tab becomes visible, the network returns, or the page is restored. A reconnect is always a *resume*: only a create or join the user asked for initialises the engine's room.

**Talking to the relay:** a page says what it wants in the address it connects to, because the relay hands each connection to its room (one Durable Object per room code) before any message flows:

```
/ws?action=create&name=<room name>&player=<id>
/ws?action=join&code=<code>&name=<room name>&player=<id>
/ws?action=rejoin&code=<code>&name=<room name>&player=<id>
```

| From the relay | Meaning |
|---|---|
| `room_created` / `room_joined` `{ code, name, v, peer, resumed, recreated }` | The reply to the request. `peer` is `present`, `away` or `none` |
| `peer_joined` `{ peerId }` | Both players are connected. Sent to both on every (re)connect of either; it is what starts the handshake |
| `peer_away` | The other player's connection closed (a page change, a locked phone). They keep their place |
| `peer_left` | The other player left the room for good |
| `superseded` | This player connected again elsewhere; this socket is being closed and must not reconnect |
| `error` `{ code, message }` | The request was refused (`not_found`, `name_mismatch`, `full`, …); the socket is then closed |

A page sends `relay` (pass this to the other player), `leave` (give up my place) and `ping`.

**A player's place outlives their connection.** The relay keys each of a room's two places by the player's id, not by a socket. Returning with the same id always gets the same place back, and the newest connection for an id replaces any older one. A room with nobody connected is kept for 12 hours. After that, or if the relay ever loses a room, a `rejoin` simply recreates it under the same code: the players hold all the game state, so nothing is lost. A player who lost their saved session can join again with the room name and takes over their own old place, provided it is away; a connected player is never displaced.

**Across a page change:** `kipukas-api.js` posts `/api/room/resume` (room, ids and the saved room document) before HTMX issues its first request, so the new page renders in room mode with shared state from the start. It then loads `kipukas-multiplayer.js`, which connects with `action=rejoin`. On `peer_joined` both sides say `hello` and run the handshake.

### Pattern 10: Room Persistence in localStorage, One Tab at a Time

**The pattern:** `kipukas-api.js` owns room storage through `globalThis.kipukasRoomSession`:

| Key | Contents |
|-----|----------|
| `kipukas_room` | `{ v, code, name, creator, playerId, peerId, ts }` — forgotten after 12 hours without use |
| `kipukas_room_doc` | The room document (base64), saved after every POST to `/api/room/*` or `/api/game/*` via the worker's `PERSIST_STATE` message |
| `kipukas_room_tab` | Id of the tab that holds the room connection |

**Why localStorage (not sessionStorage):** sessionStorage is per tab. A card opened from the phone's camera app lands in a new tab, which then had no room. localStorage is shared by every tab of the browser, so the new tab resumes the same room as the same player.

**Why one tab at a time:** the relay gives each player one connection (a newer one replaces an older one, which is told `superseded`), and each tab runs its own engine loaded from storage. Saving the session claims the room for this tab (`kipukas_room_tab`). Other tabs see the `storage` event, close their socket and show "Open in another tab". A tab that no longer holds the room does not save the room document, and any tab whose stored state was changed by another tab reloads the next time it is shown instead of writing stale state back. Showing a tab is therefore what hands the room to it.

**Limits:** an iOS home-screen app and Safari do not share storage, so a camera-app scan (which opens Safari) cannot resume a room that lives in the installed app. The same holds on Android when the default browser is not the one that installed the app. Use the in-site scanner there.

**Testing note:** two tabs of one browser are now one player. To play both sides on one machine, use a private window or a second browser profile for the second player.

### Pattern 11: Self-Contained Tool Component (Alpine × HTMX × Tailwind)

**The pattern:** Each interactive tool lives in a single `_includes/*.html` file that combines four layers: Alpine.js `x-data` for UI-only state (always-in-DOM visibility control), `@click`/`$watch`/`x-effect` for behavior, HTMX `hx-get`/`hx-post` for fetching content from Rust and posting user actions, Tailwind utilities for all styling, and auto-persistence via the Worker `PERSIST_STATE` message bridge. The component is fully self-contained — no external CSS, no separate JS file, no global state leakage.

**Reference implementation:** `_includes/multiplayer.html`, the room dialog (create, join, leave). It is the last tool built this way.

> **Being replaced.** A tool's content is better held as an engine-owned view (Pattern 13): the turn timer, alone and in a room (`turn_tracker.html`, `multiplayer_turn_tool.html`), fists in a room (`multiplayer_fists_tool.html`), the affinity panel (`affinity_tool.html`) and the type matchup calculator (`local_fists_tool.html`) were built this way and have been moved over. What stays from this pattern is the outer layer: an Alpine `x-data` flag that opens and closes the dialog. The `hx-get`, `hx-trigger="load"` and `x-effect` re-fetch described below are not needed for a view, which is drawn each time its dialog opens.

**Layer breakdown:**

| Layer | Technology | Role in the component |
|-------|-----------|----------------------|
| **UI state** | Alpine `x-data` | Local booleans like `showMultiplayer` — purely visual, never persisted |
| **Room state** | `$store.room` | `{ inRoom, link, peer, outdated }`, registered by `kipukas-api.js` and kept current by `kipukas-multiplayer.js`. Read at initialisation, so a tool is right from the first render after a page change |
| **Behavior** | Alpine `@click`, `$watch`, `x-effect` | `@click` toggles visibility; `$watch` handles side effects; `x-effect` bridges to HTMX |
| **Data fetching** | HTMX `hx-get` + `hx-trigger` | Declarative WASM endpoint on the container div; `x-effect` re-fetches via `htmx.ajax()` on reopen |
| **Styling** | Tailwind utilities | Layout, theming, spacing, responsiveness — all inline classes, zero custom CSS |
| **Persistence** | base64 yrs binary → localStorage (via kipukas-api.js) | WASM state auto-persists after every mutation via Worker `PERSIST_STATE` message; component reads fresh state from WASM on each open |

**How the layers connect (annotated from `multiplayer.html`):**

```html
<!-- 1. Alpine x-data: declare the tool's own visual state locally -->
<div x-data="{ showMultiplayer: false }" class="relative place-content-center"
     @close-multiplayer.window="showMultiplayer = false">

    <!-- 2. @click toggles visibility (Alpine handles show/hide) -->
    <button aria-label="Toggle Multiplayer" @click="showMultiplayer = !showMultiplayer" class="toolbar-tile">...</button>

    <!-- 3. Modal overlay: Tailwind utilities for layout + theming -->
    <div x-show="showMultiplayer" x-cloak
        class="fixed inset-0 flex items-center justify-center z-50"
        x-transition.duration.350ms

        <!-- 4. x-effect: bridge Alpine → HTMX. Re-fetch WASM data every time modal opens -->
        x-effect="if (showMultiplayer) $nextTick(() => {
            if (typeof htmx !== 'undefined') {
                htmx.ajax('GET', '/api/room/status', {target:'#room-status', swap:'innerHTML'});
            }
        })">

        <!-- Backdrop: click-to-close -->
        <div class="absolute inset-0 bg-slate-300 opacity-75" @click="showMultiplayer = false"></div>

        <!-- Modal content: Tailwind for card-like appearance -->
        <div class="bg-amber-50 z-50 rounded-lg shadow-xl w-full max-w-sm max-h-[85vh] overflow-y-auto relative">

            <!-- 5. HTMX container: declares WASM endpoint, loads on page init -->
            <div id="room-status"
              hx-get="/api/room/status"
              hx-trigger="load"
              hx-swap="innerHTML">
            </div>

            <!-- 6. Close button: Alpine @click, Tailwind styling -->
            <button @click="showMultiplayer = false"
              class="w-full bg-slate-400 hover:bg-slate-500 text-amber-50 font-bold py-2 px-4 rounded text-sm">
              Close
            </button>
        </div>
    </div>
</div>
```

For the same outer layer around an engine-owned view, see `multiplayer_fists_tool.html`: the dialog is opened and closed by Alpine, `$store.room.inRoom` decides whether its button shows, and its contents are one `data-engine-view` element with no `hx-get` and no `x-effect`.

**Why it works:**

- **Local state stays local.** A tool's own UI state is scoped to its `x-data` block. The one piece of shared state is `$store.room`: whether the player is in a room is the same fact for every tool on the page, and reading it from a store means a tool is correct when it initialises. (It used to arrive as window events, which a tool created after the event had fired never saw; every page change began with the multiplayer tools hidden.)
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

### Pattern 12: ~~Alpine Fire-and-Forget for Always-in-DOM Game State~~ → Superseded by Pattern 13

> **⚠️ Superseded.** Replaced by [Pattern 13: Engine-Owned Views](#pattern-13-engine-owned-views).

**Original pattern (for historical context):** The damage tracker was fetched once by HTMX and then kept in the DOM with a single Alpine `x-data` scope holding `slots` and `wasted`. A tap flipped the Alpine state at once and sent the change to the engine with `kipukasWorker.postMessage()` and no reply ("fire-and-forget"). It replaced Pattern 2 and fixed that pattern's CSS specificity race (`:class` became the only source of colour classes) and its double-update flash.

**Why it was replaced:** the state had two holders. Alpine's copy was right only as long as every change went through a tap on that page. A lost round, New Game, the same card shown twice (the hand view) or a tracker injected after load all needed extra code to bring Alpine's copy back in line (Pattern 5), and injecting a tracker meant re-initialising Alpine scopes, which was unreliable on several browsers.

The worker's handling of a request with no reply port is unchanged and still used by `postToWasm()`.

### Pattern 13: Engine-Owned Views

**The pattern:** A part of the page that shows game state is rendered by the engine and owned by it. The page keeps no copy of the state.

```html
<!-- The attribute is the engine route that renders the view. -->
<div data-engine-view="/api/card/panel?card=brox_the_defiant"></div>
```

```html
<!-- What the engine renders: the state is in the markup itself, and each
     control names the change a tap asks for. -->
<button type="button" class="mr-1 damage-slot" data-slot="2" aria-pressed="true"
        aria-label="Chain Raid damage 1 of 2"
        data-engine-post="/api/game/damage" data-engine-body="card=brox_the_defiant&slot=2">
  <div class="w-5 h-5 rounded-full border-2 transition-colors duration-300 bg-red-600 border-red-600"></div>
</button>
```

`assets/js/kipukas-view.js` does these things:

1. **Draws** every `data-engine-view` element from its route when it is shown: when the page loads, when one is added later (the hand view adds a pane per card), and each time one that was not displayed comes to be (a dialog opening). A view inside a closed dialog is not drawn at all until the dialog opens, so it opens on what is true now and costs nothing on pages where nobody opens it. A view may arrive already filled: a card page is served with its panel rendered, and drawing it only patches in the player's state.
2. **On a tap** on a `data-engine-post` control inside a view, sends the change to the engine, then draws the view again from its route.
3. **On a form's submit** (its button or the Enter key), does the same, sending the form's fields after its `data-engine-body`, and clears the form:

   ```html
   <form data-engine-post="/api/game/turns" data-engine-body="action=add">
     <input type="text" name="name"> <input type="number" name="turns" value="1">
     <button type="submit">New Timer</button>
   </form>
   ```

   A control with its own `data-engine-post` inside such a form must be `type="button"`, or its tap would also submit the form.

   A form with `data-engine-live="<milliseconds>"` is sent as it is typed in, that long after the typing pauses, and keeps what is typed. A form need not be inside a view: the binder's search field is in the toolbar, and posting it draws the views of the route it posts to.
4. **Draws related views again:** after a post, every other displayed view of the route that was posted to. The timer form posts to `/api/game/turns`; the list of timers is a view of `/api/game/turns?display=alarms`, so it shows the new timer. Give a view a route of its own when it should *not* be drawn again by its neighbour's taps: the timer form is at `/api/game/turns/form`, so a tap in the list does not clear what is being typed.
5. **Sends what the engine asks to be sent later:** an element with `data-engine-after="<milliseconds>"` beside its `data-engine-post` is sent by itself after that long, as a tap would be, and the view is drawn again. Each drawing replaces what was waiting with what the new drawing asks for. This is how something happens by the clock without the page keeping time: the engine reads the clock (`kipukas-server/src/clock.rs`), decides what the picture is now, and says how long until it next changes.

   ```html
   <!-- Inside a timer that has run out: look again in 15 seconds. -->
   <span hidden data-engine-post="/api/game/turns" data-engine-body="action=expire" data-engine-after="15000"></span>
   ```

   The element is `hidden` so that it is not also something to tap.
6. **Fills in what only the page knows:** `{hash}` in a view's route becomes what follows the `#` in the page's address, each time the view is drawn, and the view is drawn again when that changes. The hand page is `data-engine-view="/api/hand?codes={hash}"`. `{columns}` becomes how many columns the view's own CSS grid has just now. Add to `PAGE_VALUES` in `kipukas-view.js` for another such value; the time is not one (the engine reads the clock).
7. **Sends a post when its element scrolls into view:** an element with `data-engine-seen` beside its `data-engine-post` is sent once enough of it is in view, and the view is drawn again. If it is still in view after that, it is sent again. This is how the binder grows as it is read (below).
8. **Leaves alone what is not the view's to change:** when a view is drawn again, a view inside it and any element marked `data-engine-keep` stay exactly as they are (see "Views inside views" below).
9. **Patches** the new markup into the page with Idiomorph, changing only what differs. Elements keep their identity, so focus stays put and `transition-colors` animates the change.
10. **Draws again the views the engine names.** A post can change what a view elsewhere shows: a lost round of fists marks damage on a card's panel; the other player's move changes the fists dialog and the timers. The engine route says so with `page::redraw("/api/card/panel")` (`kipukas-server/src/page.rs`), the engine's worker passes it on after the post as a `REDRAW` message, and the displayed views of that route are drawn again, whatever their query. No page script has to know what a change touches.

When a page script changes the engine's state from outside a view, it calls `kipukasViews.refresh()` (every view) afterwards; New Game does. Views that are not displayed are skipped; they are drawn when shown.

**Reference implementations:**

- A card's detail panel: `kipukas-server/src/game/card_panel.rs` → `render_card_panel()`. On a card page it is in the served HTML (written at build time by the same function, see "Card Pages Are Written by the Engine") and taken over as a view when the page loads. In the hand view each pane holds one (below).
- The damage tracker inside it: `kipukas-server/src/game/damage.rs` → `render_damage_tracker()`.
- The turn timer, two views that work together, alone and in a room: `kipukas-server/src/game/turns.rs` → `render_turn_panel()` (a form) and `render_alarm_list()`; template `_includes/turn_tracker.html`. A timer that has run out says "Complete!" for 15 seconds, fades, and is gone: the engine remembers when this device first saw it run out and draws it by how long ago that was (shown, then with `opacity-0` on the same element so the transition runs, then not at all), asking to be looked at again each time. A page change in between changes nothing. This applies to a room's timers as well, each player's page by its own clock; a room's run-out timer is only left undrawn, and stays in the room document until the next advance removes it.
- The affinity panel, a view inside an Alpine dialog: `kipukas-server/src/routes/player.rs`; template `_includes/affinity_tool.html`.
- Fists in a room, a view inside an Alpine dialog that both players' moves change: `kipukas-server/src/routes/room.rs` (from `handle_fists_get`); template `_includes/multiplayer_fists_tool.html`. One route draws this player's side of the round (the form, waiting, the result with "Did you win?", the outcome) and takes every tap. The engine holds what is picked before it is locked in, so the form can be drawn again at any moment (the opponent locking in adds a note to it) and still shows the picks; a radio button is a post, checked by the engine's next drawing. Locking in with something still to pick is refused by the engine, which says what is missing in its next drawing. Alpine opens and closes the dialog, and tells the engine of the closing (`kipukasViews.post`), because a round that is over then gives way to the next.
- The type matchup calculator, three views of one route that are drawn together: `kipukas-server/src/game/matchup.rs`; template `_includes/local_fists_tool.html`. The engine holds what is picked, as a scratch pad that lasts until the page is left. A checkbox is a post; the page does not check it, the engine's next drawing does, and that drawing also disables the rest of a side that has its three. Alpine only folds the two sections.
- The binder: its grid, its filter menu and its search, all of one route. `kipukas-server/src/routes/cards.rs` holds what the binder shows: the boxes ticked in the menu, whether a search is on and what is typed, and how many tiles are on the page so far. The menu's parts (`_includes/filter.html`) are views of `/api/cards?menu=…`, so ticking a box draws the menu and the grid again; Alpine only opens the menu and folds its groups. The search field (`_includes/toolbar.html`) is a form sent as it is typed in; an open search shows the cards matching what is typed and nothing while nothing is typed, and closing it gives back what the menu says. The grid is a list that grows as it is read: in `index.html`, one element, `data-engine-view="/api/cards?columns={columns}"`. The engine holds which cards the binder shows and how many are on the page so far. It renders those tiles, at least a first batch of three rows, and after them a marker (`data-engine-seen`) that asks for the next batch when it scrolls into view. "More" is a change of the engine's state followed by a redraw, like any tap; because every tile is kept (`data-engine-keep`, with an id), the redraw only adds the new tiles, and a tile that stays through a change of filter keeps its loaded art. The request says how many to show in all, so asking twice does it once.
- The hand page, a view that holds a view per card: `kipukas-server/src/routes/hand.rs`; template `hand.html`, which is one element. The engine reads the cards from the address and renders a pane for each. A hand opens offline for cards never viewed before, because the engine has every card.

**Changes that must reach the other player.** In a room some state is shared (the room document, `game/crdt.rs`), and a change one player makes has to be sent to the other. The engine cannot send anything; the page holds the connection. So:

1. The engine route makes the change to the room document and queues the resulting update: `crdt::queue_for_peer(update)`.
2. After every post, the engine's worker takes what the engine has queued for the page (`POST /api/handover`, `kipukas-server/src/page.rs`) and posts the updates to the page as a `ROOM_UPDATES` message.
3. `kipukas-multiplayer.js` sends each update to the other player, in the message the players already exchange (`yrs_update`).
4. When an update arrives from the other player, the multiplayer script posts it to the engine (`/api/room/yrs/apply`) and does nothing more. The engine applies it and asks for the views that show the room to be drawn again (`page::redraw`, item 10 above): the timers, the fists dialog, and a card's panel if the update brought a lost round. Anything to send back (that a finished round was applied here) is queued as in step 1.

A view's control is therefore the same in a room as out of one: the turn timer's form and buttons post to `/api/game/turns` either way, and `game/turns.rs` decides whether the change is to the player's own timers or the room's. And nothing in a room polls: a player waiting on the other sees the change when it arrives. An update that cannot be sent just now (the other player is away) is made up by the exchange of state the players do whenever they connect.

**Views inside views.** Usually compose in Rust: the panel contains the damage tracker by rendering it inline, not by holding another `data-engine-view`, and a tap on the tracker redraws the panel it sits in.

A view may hold other views where each inner one should be drawn by itself: the hand holds a panel per card, and a tap on one card's damage draws that panel, not the whole hand. The rules:

- **An inner view is its own.** The outer view decides that it exists; after that the outer view's drawings leave it alone, whatever they render inside it. Render its contents there anyway (as `render_pane` does), so it arrives filled.
- **`data-engine-keep`** does the same for any other element the page has changed since it was drawn and the engine cannot know about. A pane's art fades in once it has loaded (`onload` removes `opacity-0`); kept, it is not sent back to transparent when the hand is drawn again.
- **Kept things are still removed** when the outer view no longer renders them. Give what holds them an `id` if there can be several (`hand-pane-0-KVph`), so that the same thing in the same place is matched to itself and a different one is replaced, not morphed into.

**Why it works:**

- **One holder of the state.** The page shows what the engine last rendered. A tap, a lost round, New Game and another device all end in the same redraw from the same state, so there is nothing to keep in sync and no per-case repair code.
- **Nothing to initialise.** The markup has no `x-data`, no bindings and no scripts, so a view can be injected anywhere without re-initialising a framework. This is what was unreliable on Firefox and its relatives. State that a form needs before it is sent lives in the form's own fields: the timer's color is a group of radio buttons styled with `peer-checked:`, not an Alpine variable.
- **Fast enough to wait for.** Requests go straight to the engine's worker, not through `fetch` and the service worker. A tap is drawn in a few milliseconds (measured at 1–4 ms on a laptop), so nothing is toggled ahead of the engine.
- **Order is preserved.** The worker handles requests in the order they were sent, so quick repeated taps cannot overtake each other.
- **The same markup can be produced ahead of time.** Because a view is a pure function of engine state, the engine renders it at build time too. Card pages are static HTML from the same Rust code, taken over in the browser. Nothing moves when that happens: the tracker is already in the page, so the rows below it do not jump when the engine answers.

**Rules for a view's markup:**

- The same state must always render to the same markup (patching compares markup). The time counts as state where a view depends on it; such a view takes the time as an argument (`render_alarm_list_at(now)`) so it can be tested at any moment.
- Anything the other player can influence goes through `escape_html`.
- Keep elements that should stay put (art, focusable controls) present in every state and change their attributes, rather than adding and removing them.
- No `id` attributes on what a view renders unless the page can only ever hold one copy. The timer form is rendered twice on a page (alone, and in the room's dialog), and controls that looked each other up by `id` read the wrong copy. A form's controls reach each other through the form.

**When to use this pattern:**
- Content that shows game state the engine owns, whether it is always in the DOM or inside a dialog.

**When NOT to use it:**
- Purely visual state the engine has no interest in (a menu or dialog being open): Alpine.
- The room dialog (create, join, leave) and the status pill: they show the connection, which the page still holds (`kipukas-multiplayer.js`, `kipukas-link.js`). They move when the connection does.

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
| [hmac-sha256](https://crates.io/crates/hmac-sha256) | 1.1 | MIT / Apache-2.0 | HMAC-SHA256 signing for tamper-resistant player data exports. At build time, names the cached preview pictures |
| [jpeg-encoder](https://crates.io/crates/jpeg-encoder) | 0.7 | (MIT / Apache-2.0) and IJG | Writes the cards' preview pictures. Build time only, not in the WASM module |
| [yaml-rust2](https://crates.io/crates/yaml-rust2) | 0.13 | MIT / Apache-2.0 | Reads the card files' front matter when the engine is compiled. Build time only, not in the WASM module |

### Server & Runtime

| Technology | License | Purpose |
|---|---|---|
| [Deno](https://deno.land/) | MIT | Task runner, build scripts |
| [Cloudflare Workers](https://developers.cloudflare.com/workers/) (workerd) | Apache-2.0 | Relay runtime: a Worker and one Durable Object per room |
| [Ruby](https://www.ruby-lang.org/) + Bundler | BSD 2-Clause / MIT | Jekyll runtime |

### Build Tooling

| Tool | License | Purpose |
|---|---|---|
| [esbuild](https://esbuild.github.io/) | MIT | Bundle Alpine.js + plugins into single minified file |
| [wasm-pack](https://rustwasm.github.io/wasm-pack/) | MIT / Apache-2.0 | Compile Rust crate → WASM package |
| cargo (native) | MIT / Apache-2.0 | Build and run the engine's `card-pages` program after each Jekyll build |
| [Workbox CLI](https://developer.chrome.com/docs/workbox/modules/workbox-cli) | MIT | Generate precache manifest into service worker |
| [@tailwindcss/cli](https://tailwindcss.com/) | MIT | Compile Tailwind CSS |
| [tmuxinator](https://github.com/tmuxinator/tmuxinator) | MIT | Multi-pane terminal dev environment |

### Jekyll Plugins

| Plugin | License | Purpose |
|---|---|---|
| jekyll-paginate | MIT | Pagination |
| jekyll-seo-tag | MIT | SEO meta tags of the pages Jekyll builds. The engine writes the cards' |
| jekyll-sitemap | MIT | Sitemap of the pages Jekyll builds. The engine adds the cards |
| jekyll-redirect-from | MIT | Writes `redirects.json`. The engine writes the cards' short-address pages and adds them to it |
| `_plugins/card_pages.rb` (this repo) | — | Runs the engine's `card-pages` after every build |

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
1. build:wasm          → wasm-pack compile kipukas-server/wasm → assets/js-wasm/kipukas-server-pkg/
                         (the engine's build script reads the card files first)
2. build:htmx          → Vendor HTMX → assets/js/htmx.min.js
3. build:css           → Tailwind compile + minify → assets/css/output.css
4. build:alpine        → esbuild bundle Alpine + plugins → assets/js/alpine.bundle.min.js
5. build:rules         → Build rules book subproject → game_rules/
6. jekyll build        → Generate _site/ from all sources. As its last step,
                         _plugins/card_pages.rb has the engine write the card
                         pages, their preview pictures and the cards' data file
7. build:sw            → Workbox injectManifest → sw.js with precache manifest
8. build:minify        → Minify the site's JavaScript
```

Step 6 runs `cargo run --bin card-pages` in `kipukas-server/`, a native build of the engine. The first run compiles it; later runs start in a fraction of a second. The first run with an empty preview cache also makes every card's preview picture (about 8 seconds). The same happens on every rebuild of `jekyll serve` and `jekyll build --watch`, because Jekyll removes files it did not write itself at the start of each build.

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

**Connection state machine tests** (fake socket and clock):

```bash
deno test tests/
```

**Relay tests** (the room rules, and the whole relay running locally and driven over real WebSockets, including the site's own connection code):

```bash
cd signaling-server && deno task test
```

**Browser integration checks** (DevTools console):

| Message | Confirms |
|---------|----------|
| `[kipukas-worker] WASM server initialized` | Rust WASM loaded in Web Worker |
| `[kipukas-worker] ZXing WASM initialized` | QR decode capability ready |
| `[kipukas-api] No SW controller, routing directly:` | Dev fallback active (expected during `jekyll serve`) |
| `[qr-camera] Camera started, scanning at 2 fps` | Camera + scan loop running |
| `[multiplayer] Link: connecting → open` | WebSocket to the relay open |
| `[multiplayer] Peer connected via WebSocket relay!` | Game message relay established |

**Multiplayer testing:**
- One machine: a normal window and a private window (two tabs of one browser are the same player — see Pattern 10)
- To use a local relay instead of the deployed one, see "The relay" below
- Two devices on same network
- Two devices on different networks (works everywhere — WebSocket relay traverses all firewalls/NATs)

### The relay

The relay lives in `signaling-server/` and runs on Cloudflare Workers. Cloudflare's command-line tool (`wrangler`) is run through Deno like the other build tools, so nothing else needs installing.

**Run it locally:**

```bash
cd signaling-server && deno task dev      # ws://localhost:8787/ws
```

Then, in the console of each browser window, point the site at it and reload:

```js
localStorage.setItem('kipukas_relay_url', 'ws://localhost:8787/ws')   // remove the item to go back
```

**Deploy it** (needs a Cloudflare account that holds the `kipukas.us` zone):

```bash
cd signaling-server
deno task login      # once per machine: opens the browser to authorise
deno task deploy
```

`wrangler.jsonc` holds what is deployed:

- `routes`: the relay's address, `signal.kipukas.us`. Cloudflare creates the DNS record. The same name is in `assets/js/kipukas-api.js` (`KIPUKAS_RELAY_URL`); change both together.
- `ALLOWED_ORIGINS`: the sites whose pages may use the relay. Local copies of the site are always allowed.
- `ROOM_TTL_SECONDS`: how long a room with nobody connected is kept.
- `migrations`: declares the `Room` Durable Object. The free plan offers the SQLite-backed kind, which is what is used.

The relay and the site must be deployed together when the messages between them change (`PROTOCOL_VERSION` in `signaling-server/src/room.js`). `deno task logs` shows the deployed relay's live log.

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

**Adding or changing a card:**
1. Create or edit `cards/card_name.html` (YAML front matter only), named after the card's address: `/card_name/` is `cards/card_name.html`. Give a new card `minted: YYYY-MM-DD`, the day it is added (a record only; nothing reads it). `layout` is the card's type and must be one of the types in `card_panel.rs`
2. Run `deno task build:wasm` — it rebuilds the browser's engine, which reads the card files as it compiles. A card file that is wrong stops here, with what is wrong and where
3. Build the site (`deno task build`), or let a running `jekyll serve` rebuild: it picks up the new engine files and writes the card's page and its preview picture

Do not skip step 2. The site build compiles its own engine from the card files, so the served page shows the change either way, but the browser's engine is the one built in step 2: until it is rebuilt it redraws the panel from the cards it was compiled with.

**Adding a new `/api/*` route:**
1. Create or extend a route handler in `kipukas-server/src/routes/`
2. Register the route in `kipukas-server/src/lib.rs` (the `matchit` router)
3. Add unit tests in the same file
4. Rebuild WASM: `deno task build:wasm`

**Alpine vs HTMX decision (DOM residency model):**
- Use **Alpine** for: always-in-DOM elements that need reactive appearance changes — visibility toggles, CSS class swaps, animations, transition states
- Use **HTMX** for: nothing new. It still serves the room dialog's contents (create, join, leave) and the QR scanner's status, and is being replaced
- Use an **engine-owned view** for: content that shows game state — damage trackers, the solo turn timer, the affinity panel (see Pattern 13). Rust renders it, a tap asks Rust for a change, the page patches in the result. No Alpine scope, no HTMX swap
- **Rust is the single source of truth** for all game state — components consult Rust directly, never localStorage or JS variables

**Jekyll exclusions:**
Non-Jekyll directories must be listed in `_config.yml` under `exclude:` to prevent Jekyll from processing them (especially `kipukas-server/target/` which contains thousands of Rust build files). The card files (`cards/`) are listed there too: Jekyll must not build pages from them.

**Generated files (do not edit manually):**
- `kipukas-server/cards_catalog.json` — Kippa's copy of the catalog, rewritten by the site build when the cards change
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
**Requires:** Decentralized Identity & Authentication for persistent deck storage.

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
Replace the batch-by-batch loading on the index page with a true rolling infinite scrolling system including position tracking and DOM replacements. Card count need to be around 150 to consider the feature. The engine already holds how many tiles the binder shows (`routes/cards.rs`), which is where position tracking would go.

#### 6. Card Trading
Propose an NFT brokered trade of cards marked in deck. Requires the game to be publicly available with a real player base to validate the mechanic. Also, requires the store website to be online (kipukas.com).

#### 7. Spectator Mode
Allow a third peer to observe a match via a read-only WebSocket connection. Architecturally simple (receive-only relay, no submissions) but requires rooms to support >2 peers. Low priority until competitive, streaming, or particularly compelling (active, visual, and exciting) use cases emerge.

#### 8. Provide Kippa More Tools
Expand Kippa's understanding of the game by allowing it to assist users in using site features, gathering specific card data, and resolving issues.

#### 9. Incubation Bonus & Tameability Refinement
The core tameability infrastructure shipped in the Player Document sprint: `tamability` field on all Species cards, loyalty tracking per card, affinity tracking per archetype, and a tameability progress bar on card pages showing `(loyalty + matching affinity) / threshold`. What remains is integrating the **incubation egg card bonus** into the computation, adding an `is_tamed()` helper combining all three bonus sources, and UI polish (tameability progress on card grid, tamed badge on index page).

---

## Completed Sprint: Player Document & GameState → Yrs Consolidation

> **Philosophy:** Kipukas is not meant to lock down the game after purchase. If Kipukas the company ceases to exist, gameplay experience should be unaffected. A player's progression data (affinity, loyalty, damage, decks) lives on *their* device in a conflict-free, exportable format. When a dedicated store account is available, backup/restore becomes a service — not a requirement.

**All 5 phases shipped.** The old serde_json `GameState` has been fully replaced by `PLAYER_DOC`, a persistent yrs CRDT document that owns all local player data. Legacy code, migration paths, and dead routes have been removed.

### PLAYER_DOC Structure (Final)

| Root key | yrs type | Contents |
|----------|----------|----------|
| `"cards"` | `YMap<slug, YMap>` | `{ slots: YArray<bool>, wasted: bool }` per card |
| `"alarms"` | `YArray<YMap>` | `{ remaining: i32, name: String, color_set: String }` |
| `"settings"` | `YMap` | `{ show_alarms: bool }` |
| `"affinity"` | `YMap<archetype, YMap>` | `{ level: i64, last_declared: String }` |
| `"loyalty"` | `YMap<slug, YMap>` | `{ total_plays: i64, last_played: String }` |

### Phase Summary

| Phase | What Shipped |
|-------|-------------|
| **A — Player Document Infrastructure** | `PLAYER_DOC` (yrs CRDT Doc) as sole authoritative store. Routes: `/api/player/state`, `/api/player/restore`, `/api/player/export`, `/api/player/import`. Old `GameState` serde struct and `kipukas_game_state` localStorage key fully removed. |
| **B — Affinity Tracking** | Declare archetypal affinity once per day via toolbar panel (`_includes/affinity_tool.html`). Level grows with each declaration. +1 roll bonus shown in fists combat result when attacker's `genetic_disposition` matches active affinity. Routes: `/api/player/affinity` GET/POST. The panel is an engine-owned view (Pattern 13). |
| **C — Loyalty Tracking** | Per-card play counter increments on fists combat submission (once per day per card). Loyalty badge on card pages. Species cards show tameability progress bar: `(loyalty + matching affinity) / tamability threshold`. `tamability` field added to all Species card YAML. |
| **D — Signed & Encrypted Export/Import** | Two-layer protection: Rust HMAC-SHA256 integrity + JS AES-GCM confidentiality. `.kipukas` file format. Hamburger menu → "Player Data" modal. Routes: `/api/player/export/signed`, `/api/player/import/signed`. Lazy-loaded `kipukas-crypto.js`. |
| **E — Cross-Device Sync** | Sync PLAYER_DOC between two devices via existing WebSocket relay. Mutual HMAC-SHA256 authentication, yrs CRDT merge for conflict resolution. Hamburger menu → "Sync Devices" modal. Live sync broadcasts on every `PERSIST_STATE`. Lazy-loaded `kipukas-sync.js`. 5 new sync routes. |

### Guiding Constraints (Retained)

- **Tests first.** Each phase included unit tests for new player_doc functions before wiring routes.
- **UI is separate from infrastructure.** Each phase added UI incrementally via self-contained `_includes/*.html` components (Pattern 11).
- **Single-player unaffected.** Affinity/loyalty tracking is purely local. Multiplayer features (ROOM_DOC, fists combat) remain independent. The only cross-cutting concern is the loyalty increment hook in fists submission.
