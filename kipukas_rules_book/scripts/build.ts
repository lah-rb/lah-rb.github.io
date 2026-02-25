#!/usr/bin/env -S deno run -A

/**
 * build.ts — Produces a fully self-contained static dist/ folder.
 *
 * Steps:
 *   1. Clean dist/
 *   2. Copy main site CSS → dist/css/styles.css (unified Tailwind build)
 *   3. Copy vendor JS (marked, DOMPurify) → dist/js/vendor/
 *   4. Copy app JS (rules-book.js) → dist/js/
 *   5. Copy print CSS → dist/css/
 *   6. Pre-render rules.md → HTML and inject into index.html
 *   7. Generate search index JSON → dist/js/search-index.json
 *   8. Copy images/ → dist/images/ (if exists)
 *   9. Process index.html → dist/index.html (rewrite refs, inject content)
 *
 * Ported from build.js (Node/CJS) → Deno-native TypeScript.
 * Phase 6: Unified Tailwind build — no more internal Tailwind compilation.
 */

import { Marked } from 'marked';
import { JSDOM } from 'jsdom';
import createDOMPurify from 'dompurify';
import { dirname, fromFileUrl, join, relative } from 'jsr:@std/path@1';
import { copySync, ensureDirSync, walkSync } from 'jsr:@std/fs@1';
import { existsSync } from 'jsr:@std/fs@1/exists';

const ROOT = dirname(dirname(fromFileUrl(import.meta.url)));
const SITE_ROOT = dirname(ROOT); // parent: the main site repo root
const DIST = join(ROOT, 'dist');

// ── Helpers ──────────────────────────────────────────────────

function clean(dir: string): void {
  try {
    Deno.removeSync(dir, { recursive: true });
  } catch {
    // directory may not exist
  }
  ensureDirSync(dir);
}

function copyFileSync(src: string, dest: string): void {
  ensureDirSync(dirname(dest));
  Deno.copyFileSync(src, dest);
}

function copyDirSync(src: string, dest: string): boolean {
  if (!existsSync(src)) return false;
  copySync(src, dest, { overwrite: true });
  return true;
}

// ── 1. Clean dist/ ──────────────────────────────────────────

console.log('🧹  Cleaning dist/...');
clean(DIST);

// ── 2. Copy main site CSS (unified Tailwind build) ──────────

console.log('🎨  Copying main site CSS...');
const mainCSS = join(SITE_ROOT, 'assets', 'css', 'output.css');
const outputCSS = join(DIST, 'css', 'styles.css');

if (existsSync(mainCSS)) {
  copyFileSync(mainCSS, outputCSS);
  console.log(`   → ${relative(ROOT, outputCSS)} (from main site build)`);
} else {
  console.error('❌ Main site CSS not found at', mainCSS);
  console.error('   Run "deno task build:css" from the site root first.');
  Deno.exit(1);
}

// ── 3. Copy vendor JS ───────────────────────────────────────

console.log('📦  Copying vendor JS...');

const vendors = [
  {
    src: join(ROOT, 'node_modules', 'marked', 'lib', 'marked.umd.js'),
    dest: join(DIST, 'js', 'vendor', 'marked.umd.js'),
  },
  {
    src: join(ROOT, 'node_modules', 'dompurify', 'dist', 'purify.min.js'),
    dest: join(DIST, 'js', 'vendor', 'purify.min.js'),
  },
];

for (const v of vendors) {
  copyFileSync(v.src, v.dest);
  console.log(`   → ${relative(ROOT, v.dest)}`);
}

// ── 4. Copy app JS ──────────────────────────────────────────

console.log('📄  Copying app JS...');

const jsFiles = ['rules-book.js'];
for (const file of jsFiles) {
  copyFileSync(join(ROOT, 'js', file), join(DIST, 'js', file));
  console.log(`   → dist/js/${file}`);
}

// ── 5. Copy print CSS ───────────────────────────────────────

console.log('🖨️   Copying print CSS...');
copyFileSync(join(ROOT, 'css', 'print.css'), join(DIST, 'css', 'print.css'));
console.log('   → dist/css/print.css');

// ── 6. Pre-render rules.md → HTML ───────────────────────────

console.log('📖  Pre-rendering rules.md → HTML...');

let mdText = Deno.readTextFileSync(join(ROOT, 'rules.md'));

// Convert manual page break markers to HTML divs
mdText = mdText.replace(/<!--\s*pagebreak\s*-->/gi, '<div class="page-break"></div>');

// ── Image classification (mirrors js/markdown.js) ────────────
const IMAGE_CLASSES: { test: RegExp; classes: string }[] = [
  { test: /_qr\./i, classes: 'w-20 h-20 object-cover rounded shadow-sm inline-block' },
  {
    test: /\/(capital|basecamp|soul_token|d6|diel_die|d20)\./i,
    classes: 'w-64 h-auto rounded-lg shadow-md mx-auto block',
  },
  {
    test: /\/(A_frame|log_cabin|plain_house|modern_home|boat|tunnel|dock)\./i,
    classes: 'w-36 h-auto rounded-lg shadow-md mx-auto block',
  },
  { test: /\/initials\./i, classes: 'w-16 h-auto !shadow-none' },
  { test: /\/front_cover/i, classes: 'w-96 h-auto rounded-xl shadow-lg mx-auto block' },
  {
    test: /\/(hilbert_king_of_avian_frogs|myrthvither_raven)(_back)?\./i,
    classes: 'w-96 h-auto rounded-xl shadow-lg',
  },
  { test: /\/keal_means_/i, classes: 'max-w-sm h-auto rounded-lg shadow-md' },
  { test: /\/(QR_modal|QR_scanner)\./i, classes: 'max-w-sm h-auto rounded-lg shadow-md' },
  { test: /\/fists_/i, classes: 'max-w-sm h-auto rounded-lg shadow-md' },
  { test: /\/recipe_lookup/i, classes: 'max-w-sm h-auto rounded-lg shadow-md' },
  {
    test: /\/(motives|placement_courtesy|12_slot_rec_mat)/i,
    classes: 'max-w-md h-auto rounded-lg shadow mx-auto block',
  },
  { test: /\/map_top_example/i, classes: 'max-w-sm h-auto rounded-xl shadow-lg' },
  {
    test: /\/(map_perspective|map_top)\./i,
    classes: 'max-w-lg h-auto rounded-xl shadow-lg mx-auto block',
  },
];
const DEFAULT_IMG_CLASSES = 'max-w-md h-auto rounded-lg shadow-md';

function classifyImage(src: string): string {
  for (const rule of IMAGE_CLASSES) {
    if (rule.test.test(src)) return rule.classes;
  }
  return DEFAULT_IMG_CLASSES;
}

// Same custom renderer as before — handles {#id} header anchors + image classes
const headerRegex = /\{#([^}]+)\}\s*$/;

interface HeadingToken {
  text: string;
  depth: number;
}

interface ImageToken {
  href: string;
  title: string | null;
  text: string;
}

const customRenderer = {
  heading({ text, depth }: HeadingToken): string {
    const match = text.match(headerRegex);
    let id: string, cleanText: string;
    if (match) {
      id = match[1];
      cleanText = text.replace(headerRegex, '').trim();
    } else {
      id = text
        .toLowerCase()
        .replace(/<[^>]*>/g, '')
        .replace(/[^\w\s-]/g, '')
        .replace(/\s+/g, '_')
        .replace(/-+/g, '_')
        .trim();
      cleanText = text;
    }
    if (!cleanText) return '';
    const anchor = `<a class="header-anchor" href="#${id}" title="Link to this section">#</a>`;
    return `<h${depth} id="${id}">${cleanText}${anchor}</h${depth}>\n`;
  },
  image({ href, title, text }: ImageToken): string {
    const classes = classifyImage(href || '');
    const alt = (text || '').replace(/"/g, '&quot;');
    const titleAttr = title ? ` title="${title.replace(/"/g, '&quot;')}"` : '';
    return `<img src="${href}" alt="${alt}" class="${classes}"${titleAttr} loading="lazy">`;
  },
};

const markedInstance = new Marked({ renderer: customRenderer, breaks: true, gfm: true });
const rawHtml = markedInstance.parse(mdText) as string;

// Sanitize with DOMPurify via jsdom (allow class + loading attrs for Tailwind image styling)
const dom = new JSDOM('');
const DOMPurify = createDOMPurify(dom.window as unknown as Window);
const sanitizedHtml = DOMPurify.sanitize(rawHtml, {
  ADD_ATTR: ['id', 'target', 'class', 'loading'],
});

// Post-process: group multi-image paragraphs into flex rows (via JSDOM)
const contentDom = new JSDOM(`<div id="rules-content">${sanitizedHtml}</div>`);
const contentEl = contentDom.window.document.getElementById('rules-content')!;
contentEl.querySelectorAll('p').forEach((p) => {
  const imgs = p.querySelectorAll('img');
  if (imgs.length < 2) return;
  const textOnly = p.cloneNode(true) as HTMLElement;
  textOnly.querySelectorAll('img, a').forEach((el) => el.remove());
  if (textOnly.textContent!.trim().length > 10) return;
  // Add flex classes
  ['flex', 'flex-wrap', 'gap-4', 'items-start', 'justify-center', 'my-6', 'not-prose'].forEach(
    (c) => p.classList.add(c),
  );
  imgs.forEach((img) => {
    img.classList.remove('mx-auto', 'block');
    img.classList.add('flex-shrink-0');
    img.classList.add(imgs.length === 2 ? 'max-w-[48%]' : 'max-w-[30%]');
  });
});
const preRenderedContent = contentEl.innerHTML;

// Count headers for verification
const headerMatches = preRenderedContent.match(/<h[23]\s/g) || [];
console.log(
  `   → Rendered ${headerMatches.length} sections (${
    (preRenderedContent.length / 1024).toFixed(1)
  } KB)`,
);

// ── 7. Generate search index JSON ────────────────────────────

console.log('🔍  Generating search index...');

interface SearchEntry {
  id: string;
  title: string;
  text: string;
}

const searchIndex: SearchEntry[] = [];
const allHeaders = contentEl.querySelectorAll('h2, h3');

allHeaders.forEach((header) => {
  const id = header.id || '';
  const title = header.textContent?.replace('#', '').trim() || '';
  if (!title) return;

  // Gather text between this header and the next
  let text = '';
  let sibling = header.nextElementSibling;
  while (sibling && !sibling.matches('h2, h3')) {
    text += ' ' + (sibling.textContent || '');
    sibling = sibling.nextElementSibling;
  }

  searchIndex.push({ id, title, text: text.trim() });
});

const searchIndexPath = join(DIST, 'js', 'search-index.json');
ensureDirSync(dirname(searchIndexPath));
Deno.writeTextFileSync(searchIndexPath, JSON.stringify(searchIndex));
const indexSizeKB = (JSON.stringify(searchIndex).length / 1024).toFixed(1);
console.log(`   → ${searchIndex.length} sections indexed (${indexSizeKB} KB)`);

// ── 8. Copy images/ (if exists) ─────────────────────────────

const imagesDir = join(ROOT, 'images');
if (copyDirSync(imagesDir, join(DIST, 'images'))) {
  let count = 0;
  for (const _entry of walkSync(imagesDir)) {
    count++;
  }
  console.log(`🖼️   Copied images/ (${count} files)`);
} else {
  console.log('⚠️   No images/ directory found — skipping');
}

// ── 9. Process index.html ────────────────────────────────────

console.log('🏗️   Processing index.html...');

let html = Deno.readTextFileSync(join(ROOT, 'index.html'));

// 9a. Replace dev CSS path with local built copy
html = html.replace(
  'href="../assets/css/output.css"',
  'href="css/styles.css"',
);

// 9b. Inject pre-rendered rules content into #rules-content div
html = html.replace(
  /(<div id="rules-content"[^>]*>)[\s\S]*?(<\/div>\s*<\/main>)/,
  `$1\n${preRenderedContent}\n            $2`,
);

// 9b. Replace CDN vendor scripts with local paths
html = html.replace(
  /<script src="https:\/\/cdn\.jsdelivr\.net\/npm\/marked\/marked\.min\.js"><\/script>/,
  '<script src="js/vendor/marked.umd.js"></script>',
);
html = html.replace(
  /<script src="https:\/\/cdn\.jsdelivr\.net\/npm\/dompurify@[\d.]+\/dist\/purify\.min\.js"><\/script>/,
  '<script src="js/vendor/purify.min.js"></script>',
);

Deno.writeTextFileSync(join(DIST, 'index.html'), html);
console.log('   → dist/index.html');

// ── Done ─────────────────────────────────────────────────────

interface DistFile {
  name: string;
  size: number;
}

const distFiles: DistFile[] = [];
for (const entry of walkSync(DIST, { includeDirs: false })) {
  const stat = Deno.statSync(entry.path);
  distFiles.push({ name: relative(DIST, entry.path), size: stat.size });
}

console.log('\n✅  Build complete! dist/ contents:\n');
let totalSize = 0;
for (const f of distFiles) {
  const sizeKB = (f.size / 1024).toFixed(1);
  totalSize += f.size;
  console.log(`   ${f.name.padEnd(40)} ${sizeKB.padStart(8)} KB`);
}
console.log(`${''.padEnd(52, '─')}`);
console.log(`   ${'Total'.padEnd(40)} ${(totalSize / 1024).toFixed(1).padStart(8)} KB`);
console.log(`\n🚀  Deploy the dist/ folder to any static host.\n`);
