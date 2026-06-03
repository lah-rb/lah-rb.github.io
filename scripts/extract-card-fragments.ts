/**
 * extract-card-fragments.ts
 *
 * POST-Jekyll build step. For each card, pull the rendered detail panel — the
 * `[data-card-detail]` element's innerHTML — out of its built page in _site/ and
 * write it to `_site/card_fragments/<slug>.html`.
 *
 * The /hand/ multi-card view fetches these fragments to compose full-bleed panes,
 * reusing the EXACT canonical card render for all 7 card types with no per-type
 * duplication. The marker lives in `_layouts/card.html`.
 *
 * Run AFTER `jekyll build`. Card list comes from kipukas-server/cards_catalog.json.
 */

import { join } from 'jsr:@std/path@1';
import { DOMParser } from 'jsr:@b-fuze/deno-dom@0.1';

interface CatalogCard {
  slug: string;
  url: string; // full URL, e.g. https://www.kipukas.cards/slug/  (or /fourohfour)
}

/** Map a card URL to its rendered file under _site/. */
function sitePathForUrl(url: string): string {
  let p: string;
  try {
    p = new URL(url).pathname;
  } catch {
    p = url;
  }
  p = p.replace(/^\//, '');
  // permalinks with a trailing slash render to <dir>/index.html; bare ones to <name>.html
  return p.endsWith('/') ? join('_site', p, 'index.html') : join('_site', `${p}.html`);
}

async function main() {
  const catalogRaw = await Deno.readTextFile(
    join(Deno.cwd(), 'kipukas-server', 'cards_catalog.json'),
  );
  const cards = JSON.parse(catalogRaw) as CatalogCard[];

  const outDir = join(Deno.cwd(), '_site', 'card_fragments');
  await Deno.mkdir(outDir, { recursive: true });

  let written = 0;
  let skipped = 0;
  for (const card of cards) {
    const sitePath = sitePathForUrl(card.url);
    let html: string;
    try {
      html = await Deno.readTextFile(join(Deno.cwd(), sitePath));
    } catch {
      console.warn(`[extract-card-fragments] no rendered page for ${card.slug} (${sitePath}); skipping`);
      skipped++;
      continue;
    }

    const doc = new DOMParser().parseFromString(html, 'text/html');
    const panel = doc?.querySelector('[data-card-detail]');
    if (!panel) {
      console.warn(`[extract-card-fragments] no [data-card-detail] in ${card.slug}; skipping`);
      skipped++;
      continue;
    }

    await Deno.writeTextFile(join(outDir, `${card.slug}.html`), panel.innerHTML.trim());
    written++;
  }

  console.log(
    `[extract-card-fragments] wrote ${written} fragments → _site/card_fragments/ (${skipped} skipped)`,
  );
}

main();
