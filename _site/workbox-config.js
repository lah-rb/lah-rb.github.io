const fs = require('fs');
const versionHash = fs.readFileSync('./_site/version.txt', 'utf8').trim();

module.exports = {
  globDirectory: '_site/',
  globPatterns: [
    '**/*.{html,png,css,webp,js,wasm,svg,yml,ico,pdf,json,webmanifest}'
  ],
  swDest: './sw.js',
  ignoreURLParametersMatching: [
    /^utm_/,
    /^fbclid$/
  ],
  runtimeCaching: [{
    urlPattern: /https?:\/\/[^\/]+/,
    handler: 'StaleWhileRevalidate',
    options: {
      cacheName: 'my-app-cache-' + versionHash,
      // Optionally, you can set a maximum age for the cache entries
      // cacheableResponse: { statuses: [0, 200, 404] }
    }
  }],
  clientsClaim: true,
  skipWaiting: true,
  cacheId: 'my-app-cache-' + versionHash,
  importScripts: ['./custom-sw.js'],
};


  