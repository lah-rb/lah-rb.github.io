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
    handler: 'StaleWhileRevalidate'
  }],
  clientsClaim: true,
  skipWaiting: true,
  // Use the versionHash in a way that Workbox understands (e.g., as part of the cache names)
  cacheId: 'my-app-cache-' + versionHash,
};

  