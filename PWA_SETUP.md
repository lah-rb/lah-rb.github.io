# PWA Caching Strategy - Setup Guide

## Overview

This project now uses a versioned Workbox caching strategy that ensures users receive fresh content after new releases while maintaining offline functionality.

## Key Changes

### 1. Caching Strategies

| Content Type | Strategy | Behavior |
|-------------|----------|----------|
| **HTML Pages** | `NetworkFirst` | Fetches fresh content first, falls back to cache after 3 seconds offline |
| **CSS/JS/WASM** | `StaleWhileRevalidate` | Serves cached content immediately, updates in background |
| **Images** | `CacheFirst` | Serves from cache, fetches only if not cached |
| **Fonts** | `CacheFirst` | Long-term caching (1 year) for Google Fonts |

### 2. Versioning

Each build generates a unique version hash that is included in cache names:
- Cache names: `kipukas-{type}-{versionHash}`
- Old caches are automatically cleaned up on new deployments

### 3. Update Notifications

Users are notified when new content is available via a UI notification.

## Build Process

### Step 1: Generate Version
```bash
node scripts/generate-version.js
```
This creates a version hash in `_site/version.txt` based on the content of your `_site` directory.

### Step 2: Build Service Worker
```bash
npm run build:sw
```
This generates `sw.js` using the Workbox configuration.

### Combined Build
```bash
npm run build
```
Runs both version generation and service worker build.

## Integration

### 1. Register the Service Worker

In your HTML files (typically in `<head>` or before closing `</body>`):

```html
<script src="/assets/js/pwa-update-handler.js"></script>
```

This script:
- Registers the service worker (`/sw.js`)
- Checks for updates every 30 minutes
- Shows a notification when updates are available
- Handles the update process

### 2. Configure Update Behavior

You can customize the update handler by modifying `window.PWAUpdateConfig` before the script loads:

```html
<script>
  window.PWAUpdateConfig = {
    debug: true,                    // Enable console logging
    autoReload: false,              // If true, auto-reloads without showing notification
    notificationDuration: 10000     // Auto-hide notification after 10 seconds (0 = persistent)
  };
</script>
<script src="/assets/js/pwa-update-handler.js"></script>
```

## How It Works

### First Visit
1. Service worker installs and caches all precached assets
2. Runtime caching begins for dynamic content

### Subsequent Visits
1. HTML pages are fetched fresh from the network (with 3-second timeout)
2. Static assets are served from cache while being updated in background
3. Service worker checks for updates periodically

### New Release
1. New version hash is generated
2. `sw.js` is rebuilt with new cache names
3. When user visits:
   - New service worker installs in background
   - Update notification appears
   - User clicks "Update Now" → page refreshes with new content

## Testing

### Local Testing
1. Build: `npm run build`
2. Serve `_site` with a local server (e.g., `npx serve _site`)
3. Open DevTools → Application → Service Workers
4. Check "Update on reload" for easier testing

### Force Update Check
In browser console:
```javascript
navigator.serviceWorker.ready.then(reg => reg.update());
```

### Clear Caches
In browser console:
```javascript
caches.keys().then(names => names.forEach(name => caches.delete(name)));
```

## Troubleshooting

### Users see old content
- Ensure `scripts/generate-version.js` runs before building
- Check that `npm run build:sw` completes without errors
- Verify service worker is registered with correct scope

### Updates not showing
- Check browser console for errors
- Verify `pwa-update-handler.js` is included in HTML
- Ensure service worker file is accessible at `/sw.js`

### Cache too large
- Adjust `maxEntries` in `workbox-config.js`
- Adjust `maxAgeSeconds` for different content types
- Consider excluding large files from precaching

## Files

- `workbox-config.js` - Workbox configuration
- `scripts/generate-version.js` - Version hash generator (JavaScript)
- `assets/js/pwa-update-handler.js` - Client-side update handling
- `package.json` - Build scripts
- `sw.js` - Generated service worker (do not edit directly)

## Additional Resources

- [Workbox Documentation](https://developer.chrome.com/docs/workbox/)
- [Service Worker Lifecycle](https://developers.google.com/web/fundamentals/primers/service-workers/lifecycle)
