module.exports = {
  // ============================================
  // injectManifest mode: we control the SW source in sw-src.js
  // and Workbox replaces self.__WB_MANIFEST with the precache list.
  // ============================================
  swSrc: './sw-src.js',
  swDest: './sw.js',

  globDirectory: '_site/',
  globPatterns: [
    // HTML pages
    '**/*.html',
    // Core assets
    'assets/css/**/*.css',
    'assets/js/**/*.js',
    'assets/js-wasm/**/*.{js,wasm}',
    // SVG utility images (small)
    'assets/utility_images/**/*.svg',
    // App icons & favicons
    'assets/ico/**/*.png',
    'assets/ico/*.ico',
    'assets/ico/*.svg',
    // Manifest
    'manifest.json',
    'site.webmanifest',
  ],
  globIgnores: [
    // Build artifacts that should NOT be precached
    'sw.js',
    'sw.js.map',
    'workbox-*.js',
    'workbox-*.js.map',
    'workbox-config*.js',
    'package*.json',
    'tailwind.config.js',
    'scripts/**',
    'PWA_SETUP.md',
    // Thumbnails are handled by runtime CacheFirst — don't precache all sizes
    'assets/thumbnails/**',
    // Full-size images are handled by runtime CacheFirst
    'assets/images/**',
    // Duplicate content in kipukas_rules_book source dirs
    'kipukas_rules_book/src/**',
    'kipukas_rules_book/old/**',
    'kipukas_rules_book/css/**',
    'kipukas_rules_book/images/**',
    'kipukas_rules_book/js/**',
    'kipukas_rules_book/package*.json',
    'kipukas_rules_book/build*.js',
    'kipukas_rules_book/node_modules/**',
    // Platform icon sets — runtime CacheFirst will handle these
    'windows11/**',
    'ios/**',
    'android/**',
  ],

  // Maximum file size to precache (2 MB) — skip anything larger
  maximumFileSizeToCacheInBytes: 2 * 1024 * 1024,
};
