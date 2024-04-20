// This is the "Offline copy of pages" service worker

const CACHE = "pwabuilder-offline-0.0.0";

const APP_SHELL_FILES = [
  '/',
  '/index.html',
  '/offline.html',
  '/static/js/main.js',
  '/static/css/main.css',
];
self.addEventListener('install', function(event) {
  event.waitUntil(
   caches.open(CACHE).then(function(cache) {
     return cache.addAll(APP_SHELL_FILES);
   })
  );
});
self.addEventListener('activate', function(event) {
  event.waitUntil(
   caches.keys().then(function(keyList) {
     return Promise.all(keyList.map(function(key) {
       if (key !== CACHE) {
         return caches.delete(key);
       }
     }));
   })
  );
});
self.addEventListener('fetch', function(event) {
  event.respondWith(
  caches.match(event.request).then(function(response) {
    // If the request is cached, respond with the cached content
    if (response) {
      return response;
    }

    // If the request is not in the cache, fetch the resource and cache it
    return fetch(event.request).then(function(response) {
      return caches.open(CACHE).then(function(cache) {
        cache.put(event.request.url, response.clone());
        return response;
      });
    });
  })
  );
});

importScripts('https://storage.googleapis.com/workbox-cdn/releases/5.1.2/workbox-sw.js');

self.addEventListener("message", (event) => {
  if (event.data && event.data.type === "SKIP_WAITING") {
    self.skipWaiting();
  }
});

workbox.routing.registerRoute(
  new RegExp('/*'),
  new workbox.strategies.StaleWhileRevalidate({
    cacheName: CACHE
    cacheableResponseIncludes: (response) => response.headers.get('cache-control') === 'public',
    headers: async (request, cachedResponse, context) => {
      return {
        ...await cachedResponse.clone().headers.raw(),
        'Cache-Control': `max-age=${60 * 60 * 24 * 1000}` // Set a maximum age of one day
      };
    }
  })
);
