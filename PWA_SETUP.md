# PWA Caching & Update Strategy — Setup Guide

## Overview

This project uses Workbox **injectManifest** mode to give us full control over the service worker lifecycle. The key design goal is a **user-controlled update flow**: when new content is deployed, users see a toast notification and choose when to apply the update — no surprise reloads.

## Architecture

```
sw-src.js          ← Source SW you edit (imports, message listener, routes)
workbox-config.js  ← Tells workbox-cli which files to precache
  ↓  (npm run build:sw)
sw.js              ← Generated output (self.__WB_MANIFEST replaced with file list)
sw.js.map          ← Source map

assets/js/pwa-update-handler.js  ← Client-side: registers SW, shows update toast
_layouts/default.html            ← Includes pwa-update-handler.js via <script defer>
```

## How the Update Flow Works

### First Visit
1. `pwa-update-handler.js` registers `/sw.js`.
2. Service worker installs → precaches all files in the manifest.
3. Runtime caching begins for images, thumbnails, fonts, etc.

### Subsequent Visits (no deploy)
1. HTML pages are fetched fresh from the network (NetworkFirst, 3s timeout).
2. CSS/JS/WASM are served from cache while being revalidated in the background.
3. Images are served from cache (CacheFirst).

### After a New Deploy
1. Browser detects that `sw.js` has changed → downloads and installs the new SW.
2. New SW enters the **waiting** state (it does NOT `skipWaiting` automatically).
3. `pwa-update-handler.js` detects the waiting SW and shows a toast:
   > 🎉 New version available! **[Update Now]** [Later]
4. User clicks **Update Now** → script sends `{ type: 'SKIP_WAITING' }` to the waiting SW.
5. Waiting SW calls `self.skipWaiting()` → becomes the active SW.
6. `controllerchange` event fires → page reloads once with fresh content.

If the user clicks **Later**, the toast dismisses. The waiting SW remains. Next navigation or page load will re-show the toast.

## Caching Strategies

| Content Type | Strategy | Cache Name | Behavior |
|---|---|---|---|
| **HTML Pages** | `NetworkFirst` | `kipukas-pages` | Fresh from network; falls back to cache after 3s |
| **CSS/JS/WASM** | `StaleWhileRevalidate` | `kipukas-assets` | Serve cache immediately, update in background |
| **Images** | `CacheFirst` | `kipukas-images` | Serve from cache; fetch only if not cached |
| **Google Fonts** | `CacheFirst` | `kipukas-fonts` | Long-term cache (1 year) |

**Runtime cache names are static** (no version hash). This means runtime caches persist across deploys — images and assets that haven't changed are not re-downloaded. Precaching handles versioning via per-file revision hashes in the manifest.

## Precache Scope

The `workbox-config.js` glob patterns precache:
- All HTML pages
- Core CSS, JS, and WASM assets
- SVG utility images
- App icons & favicons
- Manifest files
- Offline fallback page

**Excluded from precache** (handled by runtime caching instead):
- Thumbnail images (`assets/thumbnails/`)
- Full-size card images (`assets/images/`)
- Platform icon sets (`windows11/`, `ios/`, `android/`)
- Build artifacts (`sw.js`, `workbox-*.js`, `package.json`, etc.)
- PDF files over 2 MB

## Build Process

```bash
# Full build: Jekyll site + service worker
npm run build

# Service worker only (after Jekyll has already built _site/)
npm run build:sw
```

The `build:sw` script runs `workbox injectManifest workbox-config.js`, which:
1. Reads `sw-src.js`
2. Scans `_site/` for files matching the glob patterns
3. Replaces `self.__WB_MANIFEST` with the precache manifest (URL + revision hash pairs)
4. Writes the bundled output to `sw.js` + `sw.js.map`

## Configuration

### Update Handler Options

Override defaults by setting `window.PWAUpdateConfig` **before** the script loads:

```html
<script>
  window.PWAUpdateConfig = {
    debug: true,                    // Log lifecycle events to console
    autoReload: false,              // true = skip toast, reload immediately
    notificationDuration: 0,        // ms before auto-dismiss (0 = persistent)
    updateInterval: 30 * 60 * 1000  // How often to poll for SW updates
  };
</script>
<script src="/assets/js/pwa-update-handler.js" defer></script>
```

### Workbox Config

Edit `workbox-config.js` to change:
- `globPatterns` / `globIgnores` — what gets precached
- `maximumFileSizeToCacheInBytes` — skip files larger than this (default 2 MB)

Edit `sw-src.js` to change:
- Runtime caching strategies and cache names
- Cache expiration limits (`maxEntries`, `maxAgeSeconds`)
- The `message` event listener and lifecycle behavior

## Testing

### Local Testing
```bash
npm run build
npx serve _site
# Open DevTools → Application → Service Workers
```

### Force an Update Check
```js
navigator.serviceWorker.ready.then(reg => reg.update());
```

### Clear All Caches
```js
caches.keys().then(names => names.forEach(name => caches.delete(name)));
```

### Unregister Service Worker
```js
navigator.serviceWorker.getRegistrations().then(regs =>
  regs.forEach(reg => reg.unregister())
);
```

## Troubleshooting

| Symptom | Fix |
|---|---|
| Users see old content | Make sure `npm run build:sw` ran after `jekyll build`. Check that `sw.js` contains the updated manifest. |
| Update toast never appears | Check console for errors. Verify `pwa-update-handler.js` is loaded (Network tab). Ensure there is no duplicate `navigator.serviceWorker.register()` in your HTML. |
| Page reloads unexpectedly | Ensure `sw-src.js` does NOT call `self.skipWaiting()` unconditionally. The `SKIP_WAITING` should only happen via `postMessage`. |
| Cache storage keeps growing | Old versioned caches from the previous setup are cleaned up automatically on activate. If it persists, manually clear caches in DevTools. |
| Precache too large | Tighten `globPatterns` / add to `globIgnores` in `workbox-config.js`. Lower `maximumFileSizeToCacheInBytes`. |

## Files

| File | Purpose |
|---|---|
| `sw-src.js` | Service worker source — edit this for SW logic |
| `workbox-config.js` | Workbox CLI config — edit for precache scope |
| `assets/js/pwa-update-handler.js` | Client-side SW registration + update toast |
| `_layouts/default.html` | Includes `pwa-update-handler.js` |
| `package.json` | Build scripts |
| `sw.js` | **Generated** — do not edit directly |

## Resources

- [Workbox injectManifest](https://developer.chrome.com/docs/workbox/modules/workbox-build#injectmanifest)
- [Service Worker Lifecycle](https://web.dev/articles/service-worker-lifecycle)
- [Workbox Strategies](https://developer.chrome.com/docs/workbox/modules/workbox-strategies)
