module.exports = {
  // ============================================
  // injectManifest mode: we control the SW source in sw-src.js
  // and Workbox replaces self.__WB_MANIFEST with the precache list.
  // ============================================
  swSrc: './sw-src.js',
  // Write the generated SW directly into the deploy output (_site). build:sw runs
  // AFTER `jekyll build`, so this overwrites Jekyll's stale copy with a manifest
  // globbed from the CURRENT _site. (Previously swDest was the repo root, so the
  // deployed _site/sw.js was always one build stale — its manifest pointed at
  // files that no longer existed, failing precache install on fresh clients.)
  swDest: './_site/sw.js',

  globDirectory: '_site/',
  globPatterns: [
    // HTML pages
    '**/*.html',
    // Core assets
    'assets/css/**/*.css',
    'assets/js/**/*.js',
    'assets/js-wasm/**/*.{js,wasm}',
    // Rules book JS + search index (for offline rules viewing)
    'game_rules/js/**/*.{js,json}',
    // SVG utility images (small)
    'assets/utility_images/**/*.svg',
    // App icons & favicons
    'assets/ico/**/*.png',
    'assets/ico/*.ico',
    'assets/ico/*.svg',
    // Manifest
    'manifest.json',
    'site.webmanifest',
    // Tiny list of assets the SW warms into runtime caches after PWA install.
    'offline-manifest.json',
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
    // Archived experiment — must never be precached (also excluded from _site)
    'YOLO_rqrr/**',
    'runs/**',
    // Keep first-visit precache lean. The QR decoder is only needed when the
    // scanner opens (cached on demand via the SWR route; warmed on PWA install).
    'assets/js-wasm/zxing_reader.js',
    'assets/js-wasm/zxing_reader.wasm',
    'assets/js-wasm/rqrr-decode-pkg/**',
    // Workbox dev builds are never loaded (only *.prod.js runs).
    'assets/js/workbox/**/*.dev.js',
  ],

  // Maximum file size to precache (2 MB) — skip anything larger
  maximumFileSizeToCacheInBytes: 2 * 1024 * 1024,
};
