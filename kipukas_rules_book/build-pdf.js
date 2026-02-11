#!/usr/bin/env node

/**
 * build-pdf.js — Generates a print-ready PDF from rules.md via WeasyPrint.
 *
 * Steps:
 *   1. Read and pre-process rules.md (page break markers)
 *   2. Render markdown → HTML with custom renderer (PDF image classes)
 *   3. Post-process multi-image paragraphs
 *   4. Wrap in standalone HTML document with pdf.css
 *   5. Write temp HTML file
 *   6. Run WeasyPrint → kipukas_rules.pdf
 *   7. Clean up temp file
 *
 * Usage:
 *   node build-pdf.js                  → outputs kipukas_rules.pdf
 *   node build-pdf.js --output dist/   → outputs dist/kipukas_rules.pdf
 */

const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const { Marked } = require('marked');
const { JSDOM } = require('jsdom');

const ROOT = __dirname;

// Parse --output flag
let outputDir = ROOT;
const outputIdx = process.argv.indexOf('--output');
if (outputIdx !== -1 && process.argv[outputIdx + 1]) {
    outputDir = path.resolve(ROOT, process.argv[outputIdx + 1]);
}
const OUTPUT_PDF = path.join(outputDir, 'kipukas_rules.pdf');

// ── 1. Read and preprocess rules.md ──────────────────────────

console.log('📖  Reading rules.md...');
let mdText = fs.readFileSync(path.join(ROOT, 'rules.md'), 'utf-8');

// Convert page break markers
mdText = mdText.replace(/<!--\s*pagebreak\s*-->/gi, '<div class="page-break"></div>');

// ── 2. Image classification for PDF ─────────────────────────
// Maps image filenames → PDF CSS classes (not Tailwind classes)

const IMAGE_CLASSES = [
    // QR code thumbnails
    { test: /_qr\./i, classes: 'qr-thumb' },
    // Small game tokens & pieces
    { test: /\/(capital|basecamp|soul_token|d6|diel_die|d20)\./i, classes: 'img-token' },
    // Geography markers
    { test: /\/(A_frame|log_cabin|plain_house|modern_home|boat|tunnel|dock)\./i, classes: 'img-geo' },
    // Signature / initials
    { test: /\/initials\./i, classes: 'img-tiny' },
    // Book cover
    { test: /\/front_cover/i, classes: 'img-large' },
    // Card art
    { test: /\/(hilbert_king_of_avian_frogs|myrthvither_raven)(_back)?\./i, classes: 'img-large' },
    // KEAL means tracker / UI elements
    { test: /\/keal_means_clean/i, classes: 'qr-thumb' },
    { test: /\/keal_means_/i, classes: 'img-screenshot' },
    // QR scanner UI screenshots
    { test: /\/(QR_modal|QR_scanner)\./i, classes: 'img-screenshot' },
    // Fists tool screenshots
    { test: /\/fists_closed/i, classes: 'img-tiny' },
    { test: /\/fists_open/i, classes: 'img-token' },
    // Recipe lookup
    { test: /\/recipe_lookup/i, classes: 'img-screenshot' },
    // Diagrams
    { test: /\/(motives|placement_courtesy|12_slot_rec_mat)/i, classes: 'img-diagram' },
    // Map examples (smaller)
    { test: /\/map_top_example/i, classes: 'img-screenshot' },
    // Large map views
    { test: /\/(map_perspective|map_top)\./i, classes: 'img-large' },
    // Brawl diagrams
    { test: /\/brawl_sequence/i, classes: 'img-diagram' },
    // Moving downed souls
    { test: /\/moving_downed_souls/i, classes: 'img-diagram' },
    // Marked damage tracker
    { test: /\/marked_damage_tracker/i, classes: 'img-screenshot' },
    // Scanning card
    { test: /\/scanning_card/i, classes: 'img-screenshot' },
];

const DEFAULT_IMG_CLASS = '';

function classifyImage(src) {
    for (const rule of IMAGE_CLASSES) {
        if (rule.test.test(src)) return rule.classes;
    }
    return DEFAULT_IMG_CLASS;
}

// ── 3. Custom marked renderer ────────────────────────────────

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
        return `<h${depth} id="${id}">${cleanText}</h${depth}>\n`;
    },
    image({ href, title, text }) {
        const cls = classifyImage(href || '');
        const alt = (text || '').replace(/"/g, '&quot;');
        const titleAttr = title ? ` title="${title.replace(/"/g, '&quot;')}"` : '';
        const classAttr = cls ? ` class="${cls}"` : '';
        // Keep relative path here — we'll resolve to file:// AFTER DOMPurify
        return `<img src="${href}" alt="${alt}"${classAttr}${titleAttr}>`;
    }
};

const markedInstance = new Marked({ renderer: customRenderer, breaks: true, gfm: true });
const rawHtml = markedInstance.parse(mdText);

console.log('🔨  Rendering markdown → HTML...');

// Sanitize with DOMPurify
const dom = new JSDOM('');
const DOMPurify = require('dompurify')(dom.window);
const sanitizedHtml = DOMPurify.sanitize(rawHtml, {
    ADD_ATTR: ['id', 'target', 'class', 'loading'],
});

// ── 4. Post-process: group multi-image paragraphs ────────────

const contentDom = new JSDOM(`<div id="content">${sanitizedHtml}</div>`);
const contentEl = contentDom.window.document.getElementById('content');

// Resolve all image src to absolute file:// paths for WeasyPrint
// (Done AFTER DOMPurify which would strip file:// URLs)
contentEl.querySelectorAll('img').forEach(img => {
    const src = img.getAttribute('src');
    if (src && !src.startsWith('http') && !src.startsWith('file://') && !src.startsWith('data:')) {
        const absPath = path.resolve(ROOT, src.replace(/^\.\//, ''));
        img.setAttribute('src', 'file://' + absPath);
    }
});

// Group multi-image paragraphs into flex rows
contentEl.querySelectorAll('p').forEach(p => {
    const imgs = p.querySelectorAll('img');
    if (imgs.length < 2) return;

    // Check that it's predominantly images
    const textOnly = p.cloneNode(true);
    textOnly.querySelectorAll('img, a').forEach(el => el.remove());
    if (textOnly.textContent.trim().length > 10) return;

    // Mark as image row for PDF CSS
    p.classList.add('image-row');
});

const renderedContent = contentEl.innerHTML;

// ── 5. Build standalone HTML document ────────────────────────

const cssPath = path.resolve(ROOT, 'css', 'pdf.css');

const htmlDocument = `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <link rel="stylesheet" href="file://${cssPath}">
</head>
<body>
${renderedContent}
</body>
</html>`;

// Write temp HTML
const tempHtml = path.join(ROOT, '.pdf-temp.html');
fs.writeFileSync(tempHtml, htmlDocument, 'utf-8');

const contentSize = (renderedContent.length / 1024).toFixed(1);
const h2Count = (renderedContent.match(/<h2\s/g) || []).length;
console.log(`   → ${h2Count} sections, ${contentSize} KB HTML`);

// ── 6. Run WeasyPrint ────────────────────────────────────────

console.log('🖨️   Generating PDF with WeasyPrint...');

// Ensure output directory exists
fs.mkdirSync(outputDir, { recursive: true });

// Find weasyprint binary
const home = process.env.HOME || '/Users/lah-rb';
const weasyPaths = [
    path.join(home, '.local', 'bin', 'weasyprint'),
    '/opt/homebrew/bin/weasyprint',
    'weasyprint',
];

let weasyBin = 'weasyprint';
for (const p of weasyPaths) {
    try {
        execSync(`"${p}" --version 2>/dev/null`, { stdio: 'pipe' });
        weasyBin = p;
        break;
    } catch (e) {
        continue;
    }
}

try {
    execSync(`"${weasyBin}" "${tempHtml}" "${OUTPUT_PDF}"`, {
        cwd: ROOT,
        stdio: 'inherit',
    });
    console.log(`\n✅  PDF generated: ${path.relative(ROOT, OUTPUT_PDF)}`);

    const pdfStat = fs.statSync(OUTPUT_PDF);
    const sizeMB = (pdfStat.size / (1024 * 1024)).toFixed(1);
    console.log(`   → ${sizeMB} MB\n`);
} catch (err) {
    console.error('❌  WeasyPrint failed:', err.message);
    process.exit(1);
} finally {
    // ── 7. Clean up temp file ────────────────────────────────
    if (fs.existsSync(tempHtml)) {
        fs.unlinkSync(tempHtml);
    }
}
