importScripts('https://cdn.jsdelivr.net/npm/ios-pwa-splash@1.0.0/cdn.min.js');

self.onmessage = function(event) {
  if (event.data.type === 'generateSplash') {
    iosPWASplash(event.data.icon, event.data.color);
  }
};