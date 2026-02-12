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
precacheAndRoute([{"revision":"9bebfb5bf3adc9d50fa4cd93b3e651da","url":"zwZA.html"},{"revision":"ce1239f7f02ac613a9df1bc8f3579937","url":"zVBp.html"},{"revision":"80ca86da6c5082f82bfeb12cb18122ad","url":"zIGm.html"},{"revision":"fbb32bf37ce8336cbe55e76151a2f759","url":"yDLL.html"},{"revision":"8bfa603edc380c3b019bac665f3dec0a","url":"y5ZD.html"},{"revision":"bab3cd8dbe1b3942377330fc399d47d3","url":"v5Bi.html"},{"revision":"fc53effc56110e32d0fa54a2b242be31","url":"ro1A.html"},{"revision":"2e1f647037f2868dc7d9314ee8064723","url":"recipes.html"},{"revision":"887a132e161f7bc3c79d61b91cd76355","url":"r4GS.html"},{"revision":"03b57764155b94dc7aaabe476cb12903","url":"privacy_policy.html"},{"revision":"805611341ecb044647fae17bafbdf01a","url":"pVLu.html"},{"revision":"f42214a37bdcecd4781ce3e1acfb47a4","url":"ojod.html"},{"revision":"0fef1143ecb554fbf415991e04de5e0b","url":"offline.html"},{"revision":"4b59bace4dbaa0d6d8f72bfef594b6d7","url":"oOnN.html"},{"revision":"45fef29ad76033ff50e95f3a17d57175","url":"nqTG.html"},{"revision":"a9e4405032eaef8f4537106f926d631e","url":"mechanics_development.html"},{"revision":"805c3890105d1929da1bd3f78aa730b0","url":"mGCe.html"},{"revision":"73143dd6fa549ac60704c9c46f570b8e","url":"k8on.html"},{"revision":"0c814cb2fe7889eaafdaf792595dd82c","url":"jSYU.html"},{"revision":"541fec7d267d43030efef92e273fa0cc","url":"jKPw.html"},{"revision":"e96ab002651097108ad1de300501b0db","url":"ioqs.html"},{"revision":"a93ec90c2b0e823ca68ce4f2caad5501","url":"index.html"},{"revision":"6d28332e4e77c6c929df538b093c2e7c","url":"hLwQ.html"},{"revision":"b1f3add76fed1b4d20088b79d9504fb5","url":"frlU.html"},{"revision":"e8bb52b1bf1ce1f1046917b4a16f06db","url":"fourohfour.html"},{"revision":"e46ae2bf31077fda6cd9b3449dbbd1bb","url":"fFvj.html"},{"revision":"7c5a0f4215a17e7228c20f8bae3f823e","url":"eUN0.html"},{"revision":"a877f8517c025b925791bc42536bb968","url":"dxc9.html"},{"revision":"bfab2a77d8b2b071f534899dc090b497","url":"dVxr.html"},{"revision":"21774a64e5e26a2aeb1cf9462ad577b2","url":"brOL.html"},{"revision":"5e8db99b302526485b9c263d9829f466","url":"bSGc.html"},{"revision":"108aaf0cfa0bca9bf26917135637b373","url":"about_the_binder.html"},{"revision":"6cff210aa9b3afc458555711683b1282","url":"about_articles.html"},{"revision":"13f88c15f64c474b11acda2550312020","url":"aS1Q.html"},{"revision":"7ea469df9bbdd9d0e7ab0fcf89060077","url":"ZrrD.html"},{"revision":"5425a374fed558e2d985a4efeeffa6cd","url":"YRkt.html"},{"revision":"d54fe19aba1a7a1857b26b3eb012634c","url":"WWpl.html"},{"revision":"92a5605d0b1a44c89038631b13dcb0c4","url":"WCA9.html"},{"revision":"38d1a0b31d636d86e7201f6c6ea3c629","url":"TlVv.html"},{"revision":"aa7d9131cca98a7ea0139962a4ae507d","url":"SmCK.html"},{"revision":"64dbd57e8440d8a8710211a3e7481209","url":"SWyA.html"},{"revision":"264714b72afe530f57504aa8e90dbde2","url":"RrDC.html"},{"revision":"5305a18562306d42fafa23f17e8f3d65","url":"PhAU.html"},{"revision":"d7c13f07c5368735bf255de6d091544c","url":"P2IR.html"},{"revision":"bde06406467040d17276a5ff844da0bf","url":"Lbap.html"},{"revision":"635f37371ccdb065f9c5ee5f66561df3","url":"L-E7.html"},{"revision":"52afc27403a44fad170d61c18851b0c1","url":"K_vJ.html"},{"revision":"7659f12fb180c1133d340ba08ffab5fa","url":"KVph.html"},{"revision":"1675a1893f446df5e70873dfdd9a13ad","url":"KE4V.html"},{"revision":"5af3c79b106cbe61efe08570568fd708","url":"KCW4.html"},{"revision":"f8bbfef986e023c59460f6c7ccd59bbb","url":"K-LE.html"},{"revision":"b07ab05151018f82fbc8c818c9183af8","url":"J7a0.html"},{"revision":"aedd6ed1b369928e76002dd86fa8bc9b","url":"J2Ls.html"},{"revision":"38bee1b6387e4611ac9ee45504f8b0f6","url":"IR2B.html"},{"revision":"2671dd59c558a4e95d7e5c749ddfe47f","url":"I19b.html"},{"revision":"824a91db574c210d2842b395885a75f9","url":"Hh2Y.html"},{"revision":"568351934a19f5b23bb88577686c2f86","url":"HMPU.html"},{"revision":"e8bc9088b8bc6907dea5b23681594508","url":"H8wj.html"},{"revision":"df8b0060145f3e2bc57b0148f7e9f430","url":"G8OO.html"},{"revision":"d67492add1bf53cc23e03ace68c82eb4","url":"FuOw.html"},{"revision":"be3c604cc91775250c3afaa891941d75","url":"FOFE.html"},{"revision":"909c19929b2f95761751997d211724e7","url":"F3GW.html"},{"revision":"ca2dd5546a4c540adbe460a7043138da","url":"Eppu.html"},{"revision":"de5e2b6846aa5e5188c4bc68026cb00b","url":"BB5S.html"},{"revision":"297ac2df56d8a09c902c40fc52b2d8ac","url":"AnxM.html"},{"revision":"0b29e992b9cef9f49668bddbb6d0a20b","url":"404.html"},{"revision":"33d50cb5083efb499adbddaad55b909f","url":"what_do_you_see_in_the_breach/index.html"},{"revision":"5771f9a46350b467ac8ce4dad083f287","url":"ushered_through_sabina_emporium/index.html"},{"revision":"5f9b1007958cbd0370e8be1c07073c97","url":"unburdened_central/index.html"},{"revision":"a4525245b1b3b6d7118f009a9d3c99a4","url":"to_catch_a_spirit/index.html"},{"revision":"713cac60cdb2f8d7d1bef1a47ec6a6c5","url":"tira_marvelous_myriad/index.html"},{"revision":"8587bd748b3276df565f92f0d220b18a","url":"timebattle/index.html"},{"revision":"6c73c72e53f47e16cf58bc80097b7322","url":"the_causal_sophist/index.html"},{"revision":"783e954ab53084d895742e93a6a43e9b","url":"tejas_curious_mech/index.html"},{"revision":"b33fa96259c4332aa4b91067e6964bc6","url":"tears_for_oly/index.html"},{"revision":"dabca9b0f9a2eddecc4b13fcc83120f6","url":"suspended_animation/index.html"},{"revision":"c4397694ddfdb13ad7e02fe7d26952ef","url":"string/index.html"},{"revision":"002a727294709cd1717985b68db30571","url":"sticks/index.html"},{"revision":"2f97bae4623f1b63415d11c7da030246","url":"sprite_of_wilds_spirit/index.html"},{"revision":"c814d45cb0f562bfa013e7b87852f45e","url":"spectral_lands_decree_and_hearing/index.html"},{"revision":"ea3ad01ddfd298441eb5a4bda8d0e548","url":"shards_desert/index.html"},{"revision":"56a2484fdc9a33246b2116f0ead07f56","url":"self_care/index.html"},{"revision":"2176d3994e83cc4214bb16ac4bd1fb7c","url":"sboi_threat_plus_plus/index.html"},{"revision":"6bf203f5977a2345f606c2563193ed04","url":"rooster_calling_of_light/index.html"},{"revision":"ec3ccd9dd4c1f9d3e47c677e24fd9d02","url":"pyrostegia_dragon/index.html"},{"revision":"f977923743025ce38cc79bd3801fcc41","url":"plane_table_joker/index.html"},{"revision":"2ce6021201f0d020d518382f827d5786","url":"passage_among_maples/index.html"},{"revision":"debfba6a0ffe4ae44789cca2cb9a1e68","url":"parched_traveler/index.html"},{"revision":"5ad5f8283f3b1fa5c1111dc282daf05a","url":"palace_of_the_allele_sect/index.html"},{"revision":"385e04978ec199a7ec58f5da4bbfc9ab","url":"oshliath_and_osileth/index.html"},{"revision":"32d725fec52cfa0a845ab080fbf52818","url":"orbs_trail/index.html"},{"revision":"9037ea100913ea8eb0b366775acbe345","url":"onironauta/index.html"},{"revision":"d214edeccad708b8c4d9b6a6d4281eb5","url":"neural_network_synapse_virus/index.html"},{"revision":"194125bf0b5d2c141c67a2ef66f3798e","url":"myrthvither_raven/index.html"},{"revision":"7b47f48d417efbb3675769887898f198","url":"mutant_hide_and_seek/index.html"},{"revision":"d8267274ea1de7285542c080ca345e93","url":"mihela_cleanser_of_fields/index.html"},{"revision":"55728ef4fe1758ab8d0938b245f256e7","url":"meteor_shower/index.html"},{"revision":"4122bf8fa0451783460a2319355e43eb","url":"losetany_steppes/index.html"},{"revision":"8a837f2be4da80c1e2c066c7c597abef","url":"location_of_the_deep_apothecary_shop/index.html"},{"revision":"f1abc8a5ba874550ef4d66b6bdd578dd","url":"little_charm/index.html"},{"revision":"3192a444d31658c9d22c92dd92d4ac3c","url":"liliel_healing_fairy/index.html"},{"revision":"4f5e0e04d599fc02f48acf3fedb69546","url":"knightsoul_of_binding_time/index.html"},{"revision":"91e5e97b5336a7759ca17dd0962364a3","url":"kipukas_rules_book/index.html"},{"revision":"9d7a2f47148f1ae6d7e62cd204c90da8","url":"kipukas_rules_book/dist/index.html"},{"revision":"5207f50b1d43a54a342add55643a71fc","url":"incubation_egg/index.html"},{"revision":"29be4b0f437b7b1b66fbbf93861878fb","url":"illia_and_dorsay_the_buck_skull/index.html"},{"revision":"8527938adcb5c37d1384c0ba136d93ee","url":"honey/index.html"},{"revision":"bb0dbf48a71ed077b74a3033e341b175","url":"hilbert_king_of_avian_frogs/index.html"},{"revision":"8dadb559e536a5005017b434061029b8","url":"hidden_portal_of_lower_dreadmont_cave/index.html"},{"revision":"a239b5c6f4f7f3309227c7cdf59b51f9","url":"gray_wolf_harbinger_of_night/index.html"},{"revision":"9d7a2f47148f1ae6d7e62cd204c90da8","url":"game_rules/index.html"},{"revision":"1cd6fc8c451fb5a5e80519a3436cb1a6","url":"frost_tipped_arctic_otter/index.html"},{"revision":"7ba7c2d1e484ce97bd972c1fcac58cf0","url":"freezing_of_the_heart/index.html"},{"revision":"f7bad766bba984035d4bd962979ab201","url":"feeding_the_piffions/index.html"},{"revision":"594fee3ec0b89f3f2e623da4489e61cf","url":"feathers/index.html"},{"revision":"09681d221c77aa1a26633e9bcd302151","url":"enchantress_of_cats/index.html"},{"revision":"c521e5bcec4625f3c9a4c021ebeb77a4","url":"cloth/index.html"},{"revision":"3822173e58b95a3b1653078c8bf08bb7","url":"cartesian_sea/index.html"},{"revision":"95db6d9e98fe40165d2308938b1b2028","url":"brox_the_defiant/index.html"},{"revision":"48e6ea957d04ffb68022530a767fe6c4","url":"branwen_mantillusion_practitioner/index.html"},{"revision":"793cd5bcd0e528a3a523cfaa6ffa1fa4","url":"balanced_inline_processing_colony/index.html"},{"revision":"8afd109485bc3a0e85043ceabefe7964","url":"avian_keepers_den/index.html"},{"revision":"dec8bb1e03487f37f0572285a12708c0","url":"artificer_of_the_salt_chancel/index.html"},{"revision":"a53d56ee7557b7ae40e4f51a7fbed569","url":"arctechnic_wonderer/index.html"},{"revision":"978d9687ab8fc53d401feadd35555b55","url":"allele_sect_explorer/index.html"},{"revision":"86163bc4840ee6550cde93b81183c690","url":"assets/css/output.css"},{"revision":"5a07b39116c16107a3f48a73790876a1","url":"assets/css/input.css"},{"revision":"c189846c3895d30ce140c63ad416ff99","url":"assets/js/pwa-update-handler.js"},{"revision":"68f3b2033113a44e690a44fae77e38dd","url":"assets/js-wasm/zxing_reader.wasm"},{"revision":"0397d9ca714869aa6a087ef8f3b138a3","url":"assets/js-wasm/zxing_reader.js"},{"revision":"ba49f56f0c74d4508ed000590a303de3","url":"assets/js-wasm/typing.js"},{"revision":"474ecc1002d06316614779dbc06290ec","url":"assets/js-wasm/recipes.js"},{"revision":"7b53ee386205c4e8c2858d39fb0ba378","url":"assets/js-wasm/qr_scanner.js"},{"revision":"1cb44dd15ccccda51acf2c62eb87bcd3","url":"assets/js-wasm/ios-pwa-splash.js"},{"revision":"745f80bacc319f9df97deb180b1d0d0a","url":"assets/js-wasm/demoScript.js"},{"revision":"d2361bc323c64bc2d49e79ac9498a093","url":"assets/utility_images/topography.svg"},{"revision":"0ee5a79d6731c7d253d80f4a9d9bae69","url":"assets/utility_images/search.svg"},{"revision":"5d440b44534fbadf2a0079ac1f2e75cd","url":"assets/utility_images/qr_ico_ink.svg"},{"revision":"a649860a7e42af82c2973d703774ef5c","url":"assets/utility_images/kipukas_complete_card_collection.svg"},{"revision":"600b150b048396d6014f84d8e36e2658","url":"assets/utility_images/funnel.svg"},{"revision":"b7054f06e4bda79679f16772d09552dc","url":"assets/utility_images/fists.svg"},{"revision":"e99c389867eb52e237032cf14fb2f21a","url":"assets/utility_images/eye.svg"},{"revision":"4e9fbe48dd4e25504a4aec28a15fee90","url":"assets/utility_images/eye-slash.svg"},{"revision":"81244b7d8e44d258bdb51c523e66cc21","url":"assets/utility_images/back.svg"},{"revision":"04555205ab9d4683bbd6960503cea339","url":"android-chrome-384x384.png"},{"revision":"a9a5fdca8b890c0e4eebd370b3192dd4","url":"android-chrome-192x192.png"},{"revision":"0dab28f91434ba19e4b13b6e436699c2","url":"apple-touch-icon.png"},{"revision":"9c85ec65e585ef1507da8c4445ee2e79","url":"favicon.ico"},{"revision":"6e268fdc29bd357e42e32dd88c3d8ec1","url":"favicon-32x32.png"},{"revision":"6d86bf2dc0448b4107d6f81e30714a58","url":"favicon-16x16.png"},{"revision":"617360864fd77b81f58be6bcaf2e96b7","url":"maskable_icon.png"},{"revision":"e0169a2954ebfc927a1ccacbd9115e35","url":"manifest.json"},{"revision":"66dd761159b8c4985389cc2bf7ffb29e","url":"site.webmanifest"}], {
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
