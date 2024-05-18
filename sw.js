importScripts('https://storage.googleapis.com/workbox-cdn/releases/6.1.5/workbox-sw.js');
const CACHE_VERSION = '0.0.3'

const APP_SHELL_FILES = [
  '/index.html',
  '/offline.html',
  '/assets/css/output.css',
  '/manifest.json',
];

// Precache the app shell files
workbox.precaching.precacheAndRoute(APP_SHELL_FILES, {
  cacheName: 'pwabuilder-app-shell',
  revision: '${CACHE_VERSION}'
});


self.addEventListener('message', (event) => {
  if (event.data && event.data.type === 'SKIP_WAITING') {
    self.skipWaiting();
  }
});

// Configure Workbox routing
workbox.routing.registerRoute(
  new RegExp('/'),
  new workbox.strategies.StaleWhileRevalidate({
    cacheName: 'pwabuilder-offline-${CACHE_VERSION}',
    cacheableResponseIncludes: (response) => response.headers.get('cache-control') === 'public',
    headers: async (request, cachedResponse, context) => {
      return {
        ...await cachedResponse.clone().headers.raw(),
        'Cache-Control': `max-age=${60 * 60 * 24 * 1000}`, // Set a maximum age of one day
      };
    },
  }),
);

// Cache images
workbox.routing.registerRoute(
  /\/assets\/images\/.*/,
  new workbox.strategies.CacheFirst({
    cacheName: 'pwabuilder-images-${CACHE_VERSION}',
  }),
);

// Cache thumbnails
workbox.routing.registerRoute(
  /\/assets\/thumbnails\/.*/,
  new workbox.strategies.CacheFirst({
    cacheName: 'pwabuilder-thumbnails-${CACHE_VERSION}',
  }),
);

// Cache utility images
workbox.routing.registerRoute(
  /\/assets\/utility_images\/.*/,
  new workbox.strategies.CacheFirst({
    cacheName: 'pwabuilder-utility_images-${CACHE_VERSION}',
  }),
);