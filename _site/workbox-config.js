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
  
  // Precache configuration
  cacheId: 'kipukas-pwa-' + versionHash,
  cleanupOutdatedCaches: true,
  
  // Skip waiting and claim clients for immediate activation
  skipWaiting: true,
  clientsClaim: true,
  
  // Source map for debugging
  sourcemap: true,
  
  // Runtime caching strategies
  runtimeCaching: [
    // Strategy 1: HTML Pages - NetworkFirst (fresh content is critical)
    {
      urlPattern: /\.(?:html)$/,
      handler: 'NetworkFirst',
      options: {
        cacheName: 'kipukas-pages-' + versionHash,
        plugins: [
          {
            cacheWillUpdate: async ({ response }) => {
              // Only cache valid responses
              if (response && response.status === 200) {
                return response;
              }
              return null;
            }
          }
        ],
        networkTimeoutSeconds: 3, // Fall back to cache after 3 seconds
        matchOptions: {
          ignoreSearch: true
        }
      }
    },
    
    // Strategy 2: Static Assets (CSS, JS, WASM) - StaleWhileRevalidate with expiration
    {
      urlPattern: /\.(?:css|js|wasm)$/,
      handler: 'StaleWhileRevalidate',
      options: {
        cacheName: 'kipukas-assets-' + versionHash,
        plugins: [
          {
            cacheWillUpdate: async ({ response }) => {
              if (response && response.status === 200) {
                return response;
              }
              return null;
            }
          }
        ],
        expiration: {
          maxEntries: 200,
          maxAgeSeconds: 30 * 24 * 60 * 60, // 30 days
          purgeOnQuotaError: true
        }
      }
    },
    
    // Strategy 3: Images - CacheFirst (images rarely change)
    {
      urlPattern: /\.(?:png|jpg|jpeg|svg|gif|webp|ico)$/,
      handler: 'CacheFirst',
      options: {
        cacheName: 'kipukas-images-' + versionHash,
        plugins: [
          {
            cacheWillUpdate: async ({ response }) => {
              if (response && response.status === 200) {
                return response;
              }
              return null;
            }
          }
        ],
        expiration: {
          maxEntries: 500,
          maxAgeSeconds: 60 * 24 * 60 * 60, // 60 days
          purgeOnQuotaError: true
        }
      }
    },
    
    // Strategy 4: Google Fonts (if used) - CacheFirst with long expiration
    {
      urlPattern: /^https:\/\/fonts\.(?:googleapis|gstatic)\.com\/.*/i,
      handler: 'CacheFirst',
      options: {
        cacheName: 'kipukas-fonts-' + versionHash,
        expiration: {
          maxEntries: 30,
          maxAgeSeconds: 365 * 24 * 60 * 60 // 1 year
        }
      }
    }
  ]
};
