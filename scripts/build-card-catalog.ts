/**
 * build-card-catalog.ts
 *
 * Reads the front matter of every card file (cards/*.html, each named after
 * the card's address) and generates a Rust source file
 * (kipukas-server/src/cards_generated.rs) containing the card catalog as a
 * static array. It is compiled into the engine, which uses it both in the
 * browser (WASM: the card grid, card panels, game state) and at build time
 * (native: it writes every card's page, see kipukas-server/src/site.rs).
 *
 * Jekyll does not read the card files. Their front matter keys keep their
 * Jekyll names (layout = the card's type, permalink = its address,
 * redirect_from = its short code). `minted`, the day a card was added, is a
 * record for people: nothing here or in the engine reads it.
 *
 * Phase 3b: Extended to include game data (keal_means, injury_tolerance,
 * movement, die, brawl_sequence) for the WASM game state module.
 * Uses @std/yaml for proper YAML parsing of nested structures.
 *
 * Run: deno task build:card-catalog
 */

import { walk } from 'jsr:@std/fs@1/walk';
import { join } from 'jsr:@std/path@1';
import { parse as parseYaml } from 'jsr:@std/yaml@1';

interface KealMeans {
  name: string;
  genetics: string[];
  count: number;
}

interface CardMeta {
  slug: string; // permalink without slashes
  title: string;
  layout: string;
  img_name: string;
  img_alt: string;
  thumbnail: string; // grid crop anchor: top | center | bottom
  code: string; // 4-char redirect_from code — compact id for hand-view URLs
  tags: string;
  genetic_disposition: string | null;
  motivation: string | null;
  habitat: string | null;
  url: string; // permalink as-is (e.g. /frost_tipped_arctic_otter/)
  // Phase 3b game data
  injury_tolerance: number;
  keal_means: KealMeans[];
  movement: number;
  die: string;
  brawl_sequence: string;
  // Phase C: tamability (Species cards only, optional)
  tamability: number | null;
  // Hidden from /api/cards browse grid (e.g. the /fourohfour easter-egg card)
  hidden: boolean;
  // The art is wider than tall. The card page tints its toolbar differently.
  landscape: boolean;
  // Kippa context fields (JSON export only, not in Rust)
  description: string | null;
  play_style: string | null;
  scarcity: string | null;
  variation: string | null;
  // Display-only fields: text a card page shows that the game logic never
  // reads. Compiled into the engine so it can render the card itself.
  movement_text: string | null; // movement as written (may be text, e.g. two values)
  atk_die: string | null;
  def_die: string | null;
  criteria: string | null;
  success: string | null;
  failure: string | null;
  activation: string | null;
  detection: string | null;
  promotion: string | null;
  promotion_conditions: string | null;
}

// deno-lint-ignore no-explicit-any
function parseFrontMatter(content: string): Record<string, any> {
  const match = content.match(/^---\s*\n([\s\S]*?)\n---/);
  if (!match) return {};

  try {
    const parsed = parseYaml(match[1]);
    if (typeof parsed === 'object' && parsed !== null) {
      return parsed as Record<string, unknown>;
    }
  } catch (e) {
    console.warn(`YAML parse error: ${e}`);
  }
  return {};
}

function escapeRust(s: string): string {
  return s.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

function optionStr(val: string | undefined | null): string {
  if (!val || String(val).trim() === '') return 'None';
  return `Some("${escapeRust(String(val).trim())}")`;
}

function optionU32(val: number | null): string {
  if (val === null || val === undefined) return 'None';
  return `Some(${val})`;
}

/**
 * Extract keal_means from parsed YAML front matter.
 * Format in YAML:
 *   keal_means:
 *     "Scout's Vision":
 *       genetics: ["Avian", "Cenozoic"]
 *       count: 1
 */
// deno-lint-ignore no-explicit-any
function extractKealMeans(fm: Record<string, any>): KealMeans[] {
  const raw = fm.keal_means;
  if (!raw || typeof raw !== 'object') return [];

  const result: KealMeans[] = [];
  for (const [name, data] of Object.entries(raw)) {
    if (typeof data === 'object' && data !== null) {
      const d = data as { genetics?: string[]; count?: number };
      result.push({
        name: String(name),
        genetics: Array.isArray(d.genetics) ? d.genetics.map(String) : [],
        count: typeof d.count === 'number' ? d.count : 0,
      });
    }
  }
  return result;
}

const THUMB_ANCHORS = ['top', 'center', 'bottom'];

// The folder that holds one file per card. Jekyll does not read it.
const CARDS_DIR = 'cards';

// Two files show the 404 Glitch: fourohfour.html is the playable card
// (reached by its QR code), 404.html is the site's not-found page (no
// tracker, and a promotion). This is the not-found page's slug.
const NOT_FOUND_SLUG = '404';

/**
 * Parse a single card's front matter into a CardMeta, or return null if it is
 * missing the required fields.
 */
function extractCard(name: string, content: string): CardMeta | null {
  const fm = parseFrontMatter(content);

  if (!fm.permalink || !fm.title || !fm.layout || !fm.img_name) {
    console.warn(`Skipping ${name}: missing required front matter`);
    return null;
  }

  // /timebattle/ → timebattle, /fourohfour → fourohfour, /404.html → 404
  const slug = String(fm.permalink).replace(/\//g, '').replace(/\.html$/, '');

  const img_name = String(fm.img_name);
  const thumbnail = THUMB_ANCHORS.includes(String(fm.thumbnail)) ? String(fm.thumbnail) : 'center';

  return {
    slug,
    title: String(fm.title),
    layout: String(fm.layout),
    img_name,
    img_alt: fm.img_alt ? String(fm.img_alt).trim() : '',
    thumbnail,
    code: fm.redirect_from ? String(fm.redirect_from).replace(/\//g, '').trim() : '',
    tags: fm.tags ? String(fm.tags) : '',
    genetic_disposition: fm.genetic_disposition ? String(fm.genetic_disposition) : null,
    motivation: fm.motivation ? String(fm.motivation) : null,
    habitat: fm.habitat ? String(fm.habitat) : null,
    url: String(fm.permalink),
    // Phase 3b game data
    injury_tolerance: typeof fm.injury_tolerance === 'number' ? fm.injury_tolerance : 0,
    keal_means: extractKealMeans(fm),
    movement: typeof fm.movement === 'number' ? fm.movement : 0,
    die: fm.die ? String(fm.die) : '',
    brawl_sequence: fm.brawl_sequence ? String(fm.brawl_sequence) : '',
    tamability: typeof fm.tamability === 'number' ? fm.tamability : null,
    hidden: fm.hidden === true,
    landscape: fm.landscape === true,
    // Kippa context fields
    description: fm.description ? String(fm.description).trim() : null,
    play_style: fm.play_style ? String(fm.play_style).trim() : null,
    scarcity: fm.scarcity ? String(fm.scarcity).trim() : null,
    variation: fm.variation ? String(fm.variation).trim() : null,
    // Display-only fields
    movement_text: text(fm.movement),
    atk_die: text(fm.atk_die),
    def_die: text(fm.def_die),
    criteria: text(fm.criteria),
    success: text(fm.success),
    failure: text(fm.failure),
    activation: text(fm.activation),
    detection: text(fm.detection),
    promotion: text(fm.promotion),
    promotion_conditions: text(fm.promotion_conditions),
  };
}

/** A front matter value as display text, or null if it is absent or empty. */
function text(value: unknown): string | null {
  if (value === undefined || value === null) return null;
  const s = String(value).trim();
  return s === '' ? null : s;
}

async function main() {
  const cardsDir = join(Deno.cwd(), CARDS_DIR);
  const cards: CardMeta[] = [];
  const seen = new Set<string>();

  // Read in name order so the result does not depend on the file system's.
  const files: { name: string; path: string }[] = [];
  for await (const entry of walk(cardsDir, { exts: ['.html'], maxDepth: 1 })) {
    if (entry.isFile) files.push({ name: entry.name, path: entry.path });
  }
  files.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));

  for (const file of files) {
    const content = await Deno.readTextFile(file.path);
    const card = extractCard(file.name, content);
    if (card && !seen.has(card.slug)) {
      seen.add(card.slug);
      cards.push(card);
    }
  }

  // Sort alphabetically by title, the order the binder shows. Cards with the
  // same title (the two 404 Glitch files) stay in file name order.
  cards.sort((a, b) => a.title.localeCompare(b.title));

  // Count total keal means entries for the static array
  let totalKealMeans = 0;
  const kealGeneticsArrays: string[][] = [];
  for (const card of cards) {
    for (const km of card.keal_means) {
      totalKealMeans++;
      kealGeneticsArrays.push(km.genetics);
    }
  }

  // Generate Rust source
  const lines: string[] = [
    '// AUTO-GENERATED by scripts/build-card-catalog.ts',
    '// Do not edit manually. Re-run: deno task build:card-catalog',
    '',
    '#[derive(Debug, Clone)]',
    'pub struct KealMeans {',
    "    pub name: &'static str,",
    "    pub genetics: &'static [&'static str],",
    '    pub count: u8,',
    '}',
    '',
    '#[derive(Debug, Clone)]',
    'pub struct Card {',
    "    pub slug: &'static str,",
    "    pub title: &'static str,",
    "    pub layout: &'static str,",
    "    pub img_name: &'static str,",
    "    pub img_alt: &'static str,",
    "    pub thumbnail: &'static str,",
    "    pub tags: &'static str,",
    "    pub genetic_disposition: Option<&'static str>,",
    "    pub motivation: Option<&'static str>,",
    "    pub habitat: Option<&'static str>,",
    "    pub url: &'static str,",
    "    // 4-character short code printed in the card's QR code (its redirect_from).",
    '    // Lets a scan resolve straight to the card page without the redirect stub.',
    "    pub code: &'static str,",
    '    // Display fields: the text a card page shows, so the engine can render the',
    "    // card itself (game::card_panel). Written by the game's authors and compiled",
    '    // in, so it is trusted and may contain markup.',
    "    pub variation: Option<&'static str>,",
    "    pub movement_text: Option<&'static str>,",
    "    pub play_style: Option<&'static str>,",
    "    pub atk_die: Option<&'static str>,",
    "    pub def_die: Option<&'static str>,",
    "    pub criteria: Option<&'static str>,",
    "    pub success: Option<&'static str>,",
    "    pub failure: Option<&'static str>,",
    "    pub activation: Option<&'static str>,",
    "    pub detection: Option<&'static str>,",
    "    pub scarcity: Option<&'static str>,",
    "    pub description: Option<&'static str>,",
    "    pub promotion: Option<&'static str>,",
    "    pub promotion_conditions: Option<&'static str>,",
    '    // Phase 3b game data',
    '    pub injury_tolerance: u8,',
    "    pub keal_means: &'static [KealMeans],",
    '    pub movement: u8,',
    "    pub die: &'static str,",
    "    pub brawl_sequence: &'static str,",
    '    // Phase C: tamability (Species cards only)',
    '    pub tamability: Option<u32>,',
    '    // Hidden from the /api/cards browse grid (e.g. the /fourohfour easter-egg card)',
    '    pub hidden: bool,',
    '    // The art is wider than tall.',
    '    pub landscape: bool,',
    '}',
    '',
  ];

  // Generate static genetics arrays for each keal means
  // We need these as named statics because &'static [&'static str] can't be
  // created inline in a const array expression.
  let kealIdx = 0;
  for (const card of cards) {
    for (const km of card.keal_means) {
      const genArr = km.genetics.map((g) => `"${escapeRust(g)}"`).join(', ');
      lines.push(`static GENETICS_${kealIdx}: &[&str] = &[${genArr}];`);
      kealIdx++;
    }
  }
  if (totalKealMeans > 0) lines.push('');

  // Generate static keal means arrays per card
  kealIdx = 0;
  for (let i = 0; i < cards.length; i++) {
    const card = cards[i];
    if (card.keal_means.length > 0) {
      const entries = card.keal_means.map((km) => {
        const entry = `KealMeans { name: "${
          escapeRust(km.name)
        }", genetics: GENETICS_${kealIdx}, count: ${km.count} }`;
        kealIdx++;
        return entry;
      });
      lines.push(`static KEAL_${i}: &[KealMeans] = &[${entries.join(', ')}];`);
    }
  }
  if (cards.some((c) => c.keal_means.length > 0)) lines.push('');

  lines.push(`pub const CARD_COUNT: usize = ${cards.length};`);
  lines.push('');
  lines.push('pub static CARDS: [Card; CARD_COUNT] = [');

  for (let i = 0; i < cards.length; i++) {
    const card = cards[i];
    const kealRef = card.keal_means.length > 0 ? `KEAL_${i}` : '&[]';
    lines.push('    Card {');
    lines.push(`        slug: "${escapeRust(card.slug)}",`);
    lines.push(`        title: "${escapeRust(card.title)}",`);
    lines.push(`        layout: "${escapeRust(card.layout)}",`);
    lines.push(`        img_name: "${escapeRust(card.img_name)}",`);
    lines.push(`        img_alt: "${escapeRust(card.img_alt)}",`);
    lines.push(`        thumbnail: "${escapeRust(card.thumbnail)}",`);
    lines.push(`        tags: "${escapeRust(card.tags)}",`);
    lines.push(`        genetic_disposition: ${optionStr(card.genetic_disposition)},`);
    lines.push(`        motivation: ${optionStr(card.motivation)},`);
    lines.push(`        habitat: ${optionStr(card.habitat)},`);
    lines.push(`        url: "${escapeRust(card.url)}",`);
    lines.push(`        code: "${escapeRust(card.code)}",`);
    lines.push(`        variation: ${optionStr(card.variation)},`);
    lines.push(`        movement_text: ${optionStr(card.movement_text)},`);
    lines.push(`        play_style: ${optionStr(card.play_style)},`);
    lines.push(`        atk_die: ${optionStr(card.atk_die)},`);
    lines.push(`        def_die: ${optionStr(card.def_die)},`);
    lines.push(`        criteria: ${optionStr(card.criteria)},`);
    lines.push(`        success: ${optionStr(card.success)},`);
    lines.push(`        failure: ${optionStr(card.failure)},`);
    lines.push(`        activation: ${optionStr(card.activation)},`);
    lines.push(`        detection: ${optionStr(card.detection)},`);
    lines.push(`        scarcity: ${optionStr(card.scarcity)},`);
    lines.push(`        description: ${optionStr(card.description)},`);
    lines.push(`        promotion: ${optionStr(card.promotion)},`);
    lines.push(`        promotion_conditions: ${optionStr(card.promotion_conditions)},`);
    lines.push(`        injury_tolerance: ${card.injury_tolerance},`);
    lines.push(`        keal_means: ${kealRef},`);
    lines.push(`        movement: ${card.movement},`);
    lines.push(`        die: "${escapeRust(card.die)}",`);
    lines.push(`        brawl_sequence: "${escapeRust(card.brawl_sequence)}",`);
    lines.push(`        tamability: ${optionU32(card.tamability)},`);
    lines.push(`        hidden: ${card.hidden},`);
    lines.push(`        landscape: ${card.landscape},`);
    lines.push('    },');
  }

  lines.push('];');
  lines.push('');

  const outPath = join(Deno.cwd(), 'kipukas-server', 'src', 'cards_generated.rs');
  await Deno.writeTextFile(outPath, lines.join('\n'));

  console.log(`[build-card-catalog] Generated ${cards.length} cards → ${outPath}`);
  console.log(
    `[build-card-catalog] Game data: ${totalKealMeans} keal means across ${
      cards.filter((c) => c.keal_means.length > 0).length
    } cards`,
  );

  // Also emit a JSON catalog for Kippa (LLM tool context). The not-found page
  // is left out: it shows the same card as /fourohfour.
  const jsonCards = cards.filter((c) => c.slug !== NOT_FOUND_SLUG).map((c) => ({
    slug: c.slug,
    title: c.title,
    type: c.layout,
    variation: c.variation,
    genetic_disposition: c.genetic_disposition,
    motivation: c.motivation,
    habitat: c.habitat,
    injury_tolerance: c.injury_tolerance,
    movement: c.movement,
    die: c.die,
    brawl_sequence: c.brawl_sequence,
    keal_means: c.keal_means.map((km) => ({
      name: km.name,
      genetics: km.genetics,
      count: km.count,
    })),
    tamability: c.tamability,
    scarcity: c.scarcity,
    tags: c.tags,
    description: c.description,
    play_style: c.play_style,
    url: `https://www.kipukas.cards${c.url}`,
  }));

  const jsonPath = join(Deno.cwd(), 'kipukas-server', 'cards_catalog.json');
  await Deno.writeTextFile(jsonPath, JSON.stringify(jsonCards, null, 2));
  console.log(`[build-card-catalog] JSON catalog: ${jsonCards.length} cards → ${jsonPath}`);

  // The one thing Jekyll needs to know about the cards: how many the binder
  // shows (the filter menu's "Show All" count). Jekyll does not read the card
  // files, so it gets the number as a data file: site.data.card_catalog.count.
  const jekyllDataPath = join(Deno.cwd(), '_data', 'card_catalog.json');
  const shown = cards.filter((c) => !c.hidden).length;
  await Deno.writeTextFile(jekyllDataPath, JSON.stringify({ count: shown }) + '\n');
  console.log(`[build-card-catalog] Jekyll data: ${shown} cards shown → ${jekyllDataPath}`);

  // Hand-view index: maps each card's compact 4-char code → the data the
  // /hand/ page needs to build a pane (art + title) and fetch its fragment.
  // Codes must be exactly 4 chars so the URL hash can be parsed in fixed-width
  // chunks (e.g. /hand/#KVphRrDC = two cards). Hidden cards are excluded.
  const handIndex: Record<string, { slug: string; img_name: string; title: string }> = {};
  for (const c of cards) {
    if (c.hidden) continue;
    if (c.code.length !== 4) {
      console.warn(
        `[build-card-catalog] WARNING: card "${c.slug}" has a non-4-char code "${c.code}" — excluded from hand-index (breaks fixed-width URL parsing).`,
      );
      continue;
    }
    if (handIndex[c.code]) {
      console.warn(
        `[build-card-catalog] WARNING: duplicate code "${c.code}" (${handIndex[c.code].slug} vs ${c.slug}); keeping first.`,
      );
      continue;
    }
    handIndex[c.code] = { slug: c.slug, img_name: c.img_name, title: c.title };
  }
  const dataDir = join(Deno.cwd(), 'assets', 'data');
  await Deno.mkdir(dataDir, { recursive: true });
  const handIndexPath = join(dataDir, 'hand-index.json');
  await Deno.writeTextFile(handIndexPath, JSON.stringify(handIndex));
  console.log(
    `[build-card-catalog] Hand index: ${Object.keys(handIndex).length} codes → ${handIndexPath}`,
  );

  // Offline warm manifest: the assets the SW pulls into runtime caches AFTER the
  // user installs the PWA (kept lean in the precache for fast first visits).
  // - images: raw card .jxl (the real offline gap — art decodes on-demand and
  //   isn't cached until viewed). Warmed into the SW's raw-jxl cache.
  // - extra: the QR decoder (removed from precache; only needed when scanning).
  const offlineManifest = {
    images: cards
      .filter((c) => !c.hidden && c.img_name)
      .map((c) => `/assets/images/${c.img_name}`),
    extra: [
      '/assets/js-wasm/zxing_reader.js',
      '/assets/js-wasm/zxing_reader.wasm',
    ],
  };
  const offlinePath = join(Deno.cwd(), 'offline-manifest.json');
  await Deno.writeTextFile(offlinePath, JSON.stringify(offlineManifest));
  console.log(
    `[build-card-catalog] Offline manifest: ${offlineManifest.images.length} images + ${offlineManifest.extra.length} extra → ${offlinePath}`,
  );
}

main();
