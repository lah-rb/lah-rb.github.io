// Load Workbox from local copy (bundled by workbox-cli)
importScripts('https://storage.googleapis.com/workbox-cdn/releases/7.3.0/workbox-sw.js');

const { setCacheNameDetails, clientsClaim } = workbox.core;
const { precacheAndRoute, cleanupOutdatedCaches } = workbox.precaching;
const { registerRoute } = workbox.routing;
const { NetworkFirst, StaleWhileRevalidate, CacheFirst } = workbox.strategies;
const { ExpirationPlugin } = workbox.expiration;

// ============================================
// CACHE NAMING
// ============================================
// Static cache names — no version hash so runtime caches persist across deploys.
// Precaching already handles versioning via file-revision hashes.
setCacheNameDetails({ prefix: 'kipukas-pwa' });

// ============================================
// LIFECYCLE: controlled skipWaiting via postMessage
// ============================================
// Do NOT call self.skipWaiting() automatically.
// The client (pwa-update-handler.js) will send a SKIP_WAITING message
// when the user chooses to update, giving them control over the timing.
self.addEventListener('message', (event) => {
  if (event.data && event.data.type === 'SKIP_WAITING') {
    self.skipWaiting();
  }
});

clientsClaim();

// ============================================
// PRECACHING
// ============================================
// The placeholder below is replaced at build time by workbox-cli
// injectManifest with the list of revisioned URLs.
precacheAndRoute(self.__WB_MANIFEST, {
  ignoreURLParametersMatching: [/^utm_/, /^fbclid$/],
});
cleanupOutdatedCaches();

// ============================================
// RUNTIME CACHING STRATEGIES
// ============================================

// Strategy 1: HTML Pages — NetworkFirst (fresh content is critical)
registerRoute(
  /\.(?:html)$/,
  new NetworkFirst({
    cacheName: 'kipukas-pages',
    networkTimeoutSeconds: 3,
    matchOptions: { ignoreSearch: true },
    plugins: [
      {
        cacheWillUpdate: async ({ response }) =>
          response && response.status === 200 ? response : null,
      },
    ],
  })
);

// Strategy 2: Static Assets (CSS, JS, WASM) — StaleWhileRevalidate
registerRoute(
  /\.(?:css|js|wasm)$/,
  new StaleWhileRevalidate({
    cacheName: 'kipukas-assets',
    plugins: [
      {
        cacheWillUpdate: async ({ response }) =>
          response && response.status === 200 ? response : null,
      },
      new ExpirationPlugin({
        maxEntries: 200,
        maxAgeSeconds: 30 * 24 * 60 * 60, // 30 days
        purgeOnQuotaError: true,
      }),
    ],
  })
);

// Strategy 3: Images — CacheFirst (images rarely change)
registerRoute(
  /\.(?:png|jpg|jpeg|svg|gif|webp|ico)$/,
  new CacheFirst({
    cacheName: 'kipukas-images',
    plugins: [
      {
        cacheWillUpdate: async ({ response }) =>
          response && response.status === 200 ? response : null,
      },
      new ExpirationPlugin({
        maxEntries: 500,
        maxAgeSeconds: 60 * 24 * 60 * 60, // 60 days
        purgeOnQuotaError: true,
      }),
    ],
  })
);

// Strategy 4: Google Fonts — CacheFirst with long expiration
registerRoute(
  /^https:\/\/fonts\.(?:googleapis|gstatic)\.com\/.*/i,
  new CacheFirst({
    cacheName: 'kipukas-fonts',
    plugins: [
      new ExpirationPlugin({
        maxEntries: 30,
        maxAgeSeconds: 365 * 24 * 60 * 60, // 1 year
      }),
    ],
  })
);

// ============================================
// CLEANUP: remove old versioned runtime caches from the previous setup
// ============================================
self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((cacheNames) =>
      Promise.all(
        cacheNames
          .filter((name) => {
            // Delete any old runtime caches that contain a version hash
            // (the old format was kipukas-{type}-{hash}_{date})
            const isOldVersionedCache =
              name.startsWith('kipukas-') &&
              /kipukas-(?:pages|assets|images|fonts)-[a-f0-9]{64}/.test(name);
            // Also delete the old "my-app-cache-" prefix from the backup config
            const isLegacyCache = name.startsWith('my-app-cache-');
            return isOldVersionedCache || isLegacyCache;
          })
          .map((name) => {
            console.log('[SW] Deleting old cache:', name);
            return caches.delete(name);
          })
      )
    )
  );
});
