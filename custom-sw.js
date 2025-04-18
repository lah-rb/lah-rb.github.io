// custom-sw.js

const versionHash = self.versionHash || ''; // Ensure versionHash is accessible in the service worker scope

self.addEventListener('activate', (event) => {
  const currentVersionHash = fs.readFileSync('./_site/version.txt', 'utf8').trim();

  if (currentVersionHash !== versionHash) {
    self.versionHash = currentVersionHash; // Update the versionHash in the service worker
    self.skipWaiting();
    self.clients.matchAll().then(clients => {
      clients.forEach(client => {
        if (client.navigation) {
          client.navigation('/');
        } else {
          client.navigate('/');
        }
      });
    });
  }
});

self.addEventListener('fetch', (event) => {
  const cachedResponse = caches.match(event.request);

  const networkResponsePromise = fetch(event.request).then(response => {
    if (response.ok) {
      return response;
    } else {
      return cachedResponse;
    }
  }).catch(() => {
    return cachedResponse;
  });

  event.respondWith(networkResponsePromise);
  event.waitUntil(networkResponsePromise);
});
