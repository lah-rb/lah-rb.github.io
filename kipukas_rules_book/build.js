#!/usr/bin/env node

/**
 * build.js — Produces a fully self-contained static dist/ folder.
 *
 * Steps:
 *   1. Clean dist/
 *   2. Run Tailwind CLI → dist/css/styles.css
 *   3. Copy vendor JS (marked, DOMPurify) → dist/js/vendor/
 *   4. Copy app JS → dist/js/
 *   5. Copy print CSS → dist/css/
 *   6. Pre-render rules.md → HTML and inject into index.html
 *   7. Copy images/ → dist/images/ (if exists)
 *   8. Process index.html → dist/index.html (rewrite CDN refs, inject content)
 */

const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const { Marked } = require('marked');
const { JSDOM } = require('jsdom');

const ROOT = __dirname;
const DIST = path.join(ROOT, 'dist');

// ── Helpers ──────────────────────────────────────────────────

function clean(dir) {
    if (fs.existsSync(dir)) {
        fs.rmSync(dir, { recursive: true, force: true });
    }
    fs.mkdirSync(dir, { recursive: true });
}

function copyFileSync(src, dest) {
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.copyFileSync(src, dest);
}

function copyDirSync(src, dest) {
    if (!fs.existsSync(src)) return false;
    fs.mkdirSync(dest, { recursive: true });
    for (const entry of fs.readdirSync(src, { withFileTypes: true })) {
        const srcPath = path.join(src, entry.name);
        const destPath = path.join(dest, entry.name);
        if (entry.isDirectory()) {
            copyDirSync(srcPath, destPath);
        } else {
            fs.copyFileSync(srcPath, destPath);
        }
    }
    return true;
}

// ── 1. Clean dist/ ──────────────────────────────────────────

console.log('🧹  Cleaning dist/...');
clean(DIST);

// ── 2. Run Tailwind CLI ─────────────────────────────────────

console.log('🎨  Building Tailwind CSS...');
const tailwindBin = path.join(ROOT, 'node_modules', '.bin', 'tailwindcss');
const inputCSS = path.join(ROOT, 'src', 'input.css');
const outputCSS = path.join(DIST, 'css', 'styles.css');

fs.mkdirSync(path.dirname(outputCSS), { recursive: true });

execSync(`"${tailwindBin}" -i "${inputCSS}" -o "${outputCSS}" --minify`, {
    cwd: ROOT,
    stdio: 'inherit',
});

console.log(`   → ${path.relative(ROOT, outputCSS)}`);

// ── 3. Copy vendor JS ───────────────────────────────────────

console.log('📦  Copying vendor JS...');

const vendors = [
    {
        src: path.join(ROOT, 'node_modules', 'marked', 'lib', 'marked.umd.js'),
        dest: path.join(DIST, 'js', 'vendor', 'marked.umd.js'),
    },
    {
        src: path.join(ROOT, 'node_modules', 'dompurify', 'dist', 'purify.min.js'),
        dest: path.join(DIST, 'js', 'vendor', 'purify.min.js'),
    },
];

for (const v of vendors) {
    copyFileSync(v.src, v.dest);
    console.log(`   → ${path.relative(ROOT, v.dest)}`);
}

// ── 4. Copy app JS ──────────────────────────────────────────

console.log('📄  Copying app JS...');

const jsFiles = ['markdown.js', 'sidebar.js', 'search.js', 'kippa.js', 'app.js'];
for (const file of jsFiles) {
    copyFileSync(path.join(ROOT, 'js', file), path.join(DIST, 'js', file));
    console.log(`   → dist/js/${file}`);
}

// ── 5. Copy print CSS ───────────────────────────────────────

console.log('🖨️   Copying print CSS...');
copyFileSync(
    path.join(ROOT, 'css', 'print.css'),
    path.join(DIST, 'css', 'print.css')
);
console.log('   → dist/css/print.css');

// ── 6. Pre-render rules.md → HTML ───────────────────────────

console.log('📖  Pre-rendering rules.md → HTML...');

let mdText = fs.readFileSync(path.join(ROOT, 'rules.md'), 'utf-8');

// Convert manual page break markers to HTML divs
mdText = mdText.replace(/<!--\s*pagebreak\s*-->/gi, '<div class="page-break"></div>');

// ── Image classification (mirrors js/markdown.js) ────────────
const IMAGE_CLASSES = [
    { test: /_qr\./i, classes: 'w-20 h-20 object-cover rounded shadow-sm inline-block' },
    { test: /\/(capital|basecamp|soul_token|d6|diel_die|d20)\./i, classes: 'w-64 h-auto rounded-lg shadow-md mx-auto block' },
    { test: /\/(A_frame|log_cabin|plain_house|modern_home|boat|tunnel|dock)\./i, classes: 'w-36 h-auto rounded-lg shadow-md mx-auto block' },
    { test: /\/initials\./i, classes: 'w-16 h-auto !shadow-none' },
    { test: /\/front_cover/i, classes: 'w-96 h-auto rounded-xl shadow-lg mx-auto block' },
    { test: /\/(hilbert_king_of_avian_frogs|myrthvither_raven)(_back)?\./i, classes: 'w-96 h-auto rounded-xl shadow-lg' },
    { test: /\/keal_means_/i, classes: 'max-w-sm h-auto rounded-lg shadow-md' },
    { test: /\/(QR_modal|QR_scanner)\./i, classes: 'max-w-sm h-auto rounded-lg shadow-md' },
    { test: /\/fists_/i, classes: 'max-w-sm h-auto rounded-lg shadow-md' },
    { test: /\/recipe_lookup/i, classes: 'max-w-sm h-auto rounded-lg shadow-md' },
    { test: /\/(motives|placement_courtesy|12_slot_rec_mat)/i, classes: 'max-w-md h-auto rounded-lg shadow mx-auto block' },
    { test: /\/map_top_example/i, classes: 'max-w-sm h-auto rounded-xl shadow-lg' },
    { test: /\/(map_perspective|map_top)\./i, classes: 'max-w-lg h-auto rounded-xl shadow-lg mx-auto block' },
];
const DEFAULT_IMG_CLASSES = 'max-w-md h-auto rounded-lg shadow-md';

function classifyImage(src) {
    for (const rule of IMAGE_CLASSES) {
        if (rule.test.test(src)) return rule.classes;
    }
    return DEFAULT_IMG_CLASSES;
}

// Same custom renderer as js/markdown.js — handles {#id} header anchors + image classes
const headerRegex = /\{#([^}]+)\}\s*$/;
const customRenderer = {
    heading({ text, depth }) {
        const match = text.match(headerRegex);
        let id, cleanText;
        if (match) {
            id = match[1];
            cleanText = text.replace(headerRegex, '').trim();
        } else {
            id = text.toLowerCase()
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
    image({ href, title, text }) {
        const classes = classifyImage(href || '');
        const alt = (text || '').replace(/"/g, '&quot;');
        const titleAttr = title ? ` title="${title.replace(/"/g, '&quot;')}"` : '';
        return `<img src="${href}" alt="${alt}" class="${classes}"${titleAttr} loading="lazy">`;
    }
};

const markedInstance = new Marked({ renderer: customRenderer, breaks: true, gfm: true });
const rawHtml = markedInstance.parse(mdText);

// Sanitize with DOMPurify via jsdom (allow class + loading attrs for Tailwind image styling)
const dom = new JSDOM('');
const DOMPurify = require('dompurify')(dom.window);
const sanitizedHtml = DOMPurify.sanitize(rawHtml, { ADD_ATTR: ['id', 'target', 'class', 'loading'] });

// Post-process: group multi-image paragraphs into flex rows (via JSDOM)
const contentDom = new JSDOM(`<div id="content">${sanitizedHtml}</div>`);
const contentEl = contentDom.window.document.getElementById('content');
contentEl.querySelectorAll('p').forEach(p => {
    const imgs = p.querySelectorAll('img');
    if (imgs.length < 2) return;
    const textOnly = p.cloneNode(true);
    textOnly.querySelectorAll('img, a').forEach(el => el.remove());
    if (textOnly.textContent.trim().length > 10) return;
    // Add flex classes
    ['flex', 'flex-wrap', 'gap-4', 'items-start', 'justify-center', 'my-6', 'not-prose'].forEach(c => p.classList.add(c));
    imgs.forEach(img => {
        img.classList.remove('mx-auto', 'block');
        img.classList.add('flex-shrink-0');
        img.classList.add(imgs.length === 2 ? 'max-w-[48%]' : 'max-w-[30%]');
    });
});
const preRenderedContent = contentEl.innerHTML;

// Count headers for verification
const headerMatches = preRenderedContent.match(/<h[23]\s/g) || [];
console.log(`   → Rendered ${headerMatches.length} sections (${(preRenderedContent.length / 1024).toFixed(1)} KB)`);

// ── 7. Copy images/ (if exists) ─────────────────────────────

const imagesDir = path.join(ROOT, 'images');
if (copyDirSync(imagesDir, path.join(DIST, 'images'))) {
    const count = fs.readdirSync(imagesDir, { recursive: true }).length;
    console.log(`🖼️   Copied images/ (${count} files)`);
} else {
    console.log('⚠️   No images/ directory found — skipping');
}

// ── 8. Process index.html ────────────────────────────────────

console.log('🏗️   Processing index.html...');

let html = fs.readFileSync(path.join(ROOT, 'index.html'), 'utf-8');

// 8a. Remove the Tailwind CDN script + inline config block
//     Match: <script src="https://cdn.tailwindcss.com"></script> ... </script>
//     This covers the CDN script and the following inline config script.
html = html.replace(
    /<script src="https:\/\/cdn\.tailwindcss\.com"><\/script>\s*<script>\s*tailwind\.config\s*=\s*\{[\s\S]*?\}\s*<\/script>/,
    '<!-- Tailwind CSS (built) -->\n    <link rel="stylesheet" href="css/styles.css">'
);

// 8b. Remove the entire <style> block (now in Tailwind output)
html = html.replace(
    /\n\s*<style>[\s\S]*?<\/style>/,
    ''
);

// 8c. Inject pre-rendered rules content into #content div
//     Replace the loading spinner with the rendered HTML.
//     Anchor to </main> to avoid ambiguity with nested </div> tags.
html = html.replace(
    /(<div id="content"[^>]*>)[\s\S]*?(<\/div>\s*<\/main>)/,
    `$1\n${preRenderedContent}\n            $2`
);

// 8d. Replace CDN vendor scripts with local paths
html = html.replace(
    /<script src="https:\/\/cdn\.jsdelivr\.net\/npm\/marked\/marked\.min\.js"><\/script>/,
    '<script src="js/vendor/marked.umd.js"></script>'
);
html = html.replace(
    /<script src="https:\/\/cdn\.jsdelivr\.net\/npm\/dompurify@[\d.]+\/dist\/purify\.min\.js"><\/script>/,
    '<script src="js/vendor/purify.min.js"></script>'
);

fs.writeFileSync(path.join(DIST, 'index.html'), html, 'utf-8');
console.log('   → dist/index.html');

// ── Done ─────────────────────────────────────────────────────

const distFiles = [];
function walkDir(dir, prefix = '') {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
        if (entry.isDirectory()) {
            walkDir(path.join(dir, entry.name), rel);
        } else {
            const stat = fs.statSync(path.join(dir, entry.name));
            distFiles.push({ name: rel, size: stat.size });
        }
    }
}
walkDir(DIST);

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
