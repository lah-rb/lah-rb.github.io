/**
 * build-card-catalog.ts
 *
 * Reads all _posts/*.html front matter and generates a Rust source file
 * (kipukas-server/src/cards_generated.rs) containing the card catalog
 * as a static array. This is compiled into the WASM binary so the
 * /api/cards route can filter and paginate without any runtime data loading.
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
  // Kippa context fields (JSON export only, not in Rust)
  description: string | null;
  play_style: string | null;
  scarcity: string | null;
  variation: string | null;
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

// Root-level card pages that live outside _posts but still need a tracker.
// Explicit allowlist keeps 404.html and other root pages from being registered.
const EXTRA_CARD_FILES = ['fourohfour.html'];

/**
 * Parse a single card's front matter into a CardMeta, or return null if it is
 * missing the required fields. Shared by the _posts walk and EXTRA_CARD_FILES.
 */
function extractCard(name: string, content: string): CardMeta | null {
  const fm = parseFrontMatter(content);

  if (!fm.permalink || !fm.title || !fm.layout || !fm.img_name) {
    console.warn(`Skipping ${name}: missing required front matter`);
    return null;
  }

  const slug = String(fm.permalink).replace(/\//g, '');

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
    // Kippa context fields
    description: fm.description ? String(fm.description).trim() : null,
    play_style: fm.play_style ? String(fm.play_style).trim() : null,
    scarcity: fm.scarcity ? String(fm.scarcity).trim() : null,
    variation: fm.variation ? String(fm.variation).trim() : null,
  };
}

async function main() {
  const postsDir = join(Deno.cwd(), '_posts');
  const cards: CardMeta[] = [];
  const seen = new Set<string>();

  for await (const entry of walk(postsDir, { exts: ['.html'], maxDepth: 1 })) {
    if (!entry.isFile) continue;

    const content = await Deno.readTextFile(entry.path);
    const card = extractCard(entry.name, content);
    if (card && !seen.has(card.slug)) {
      seen.add(card.slug);
      cards.push(card);
    }
  }

  for (const name of EXTRA_CARD_FILES) {
    const content = await Deno.readTextFile(join(Deno.cwd(), name));
    const card = extractCard(name, content);
    if (card && !seen.has(card.slug)) {
      seen.add(card.slug);
      cards.push(card);
    }
  }

  // Sort alphabetically by title (matching current Jekyll sort:title)
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
    lines.push(`        injury_tolerance: ${card.injury_tolerance},`);
    lines.push(`        keal_means: ${kealRef},`);
    lines.push(`        movement: ${card.movement},`);
    lines.push(`        die: "${escapeRust(card.die)}",`);
    lines.push(`        brawl_sequence: "${escapeRust(card.brawl_sequence)}",`);
    lines.push(`        tamability: ${optionU32(card.tamability)},`);
    lines.push(`        hidden: ${card.hidden},`);
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

  // Also emit a JSON catalog for Kippa (LLM tool context)
  const jsonCards = cards.map((c) => ({
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
