#!/usr/bin/env -S deno run -A

/**
 * minify-site.ts — Post-build minification of JS files in _site/.
 *
 * Runs after `build:sw` (the final build step) to minify all unminified
 * JavaScript in the built site. Uses esbuild's transform() API (text-in →
 * text-out) so import paths, importScripts(), and module worker imports
 * are left untouched — no bundling, no import resolution.
 *
 * Skips:
 *   - Files already minified (*.min.js)
 *   - Vendored Workbox libraries (workbox/)
 *   - Vendored game_rules libraries (game_rules/js/vendor/)
 *   - Emscripten / wasm-pack generated code (js-wasm/zxing_reader.js, kipukas-server-pkg/)
 *   - Non-JS files with .js extension (demoScript.js is an HTML fragment)
 *
 * Output: overwrites each file in-place inside _site/ and prints a size report.
 */

import * as esbuild from 'esbuild';
import { dirname, join, relative } from 'jsr:@std/path@1';
import { fromFileUrl } from 'jsr:@std/path@1/from-file-url';

const ROOT = dirname(dirname(fromFileUrl(import.meta.url)));
const SITE_DIR = join(ROOT, '_site');

/** Patterns (tested against path relative to _site/) to skip. */
const SKIP_PATTERNS = [
  /\.min\.js$/, // Already minified
  /\/workbox\//, // Vendored Workbox libraries
  /\/vendor\//, // Vendored game_rules libraries
  /\/kipukas-server-pkg\//, // wasm-pack generated glue code
  /\/zxing_reader\.js$/, // Emscripten-generated (106 KB, already optimized)
  /\/demoScript\.js$/, // HTML fragment with <script> wrapper, not valid JS
];

/** Recursively collect all .js files under a directory. */
async function collectJsFiles(dir: string): Promise<string[]> {
  const files: string[] = [];
  for await (const entry of Deno.readDir(dir)) {
    const fullPath = join(dir, entry.name);
    if (entry.isDirectory) {
      files.push(...await collectJsFiles(fullPath));
    } else if (entry.isFile && entry.name.endsWith('.js')) {
      files.push(fullPath);
    }
  }
  return files;
}

// ── Main ───────────────────────────────────────────────────────────

try {
  // Verify _site/ exists
  await Deno.stat(SITE_DIR);
} catch {
  console.error('❌ _site/ directory not found. Run the Jekyll build first.');
  Deno.exit(1);
}

const allFiles = await collectJsFiles(SITE_DIR);

// Filter to only files that need minification
const targets = allFiles.filter((f) => {
  const rel = relative(SITE_DIR, f);
  return !SKIP_PATTERNS.some((pat) => pat.test(rel));
});

if (targets.length === 0) {
  console.log('No JS files to minify.');
  esbuild.stop();
  Deno.exit(0);
}

console.log(`\nMinifying ${targets.length} JS files in _site/...\n`);

let totalBefore = 0;
let totalAfter = 0;
let skipped = 0;

for (const filePath of targets) {
  const rel = relative(SITE_DIR, filePath);
  const source = await Deno.readTextFile(filePath);
  const beforeSize = new TextEncoder().encode(source).byteLength;

  try {
    const result = await esbuild.transform(source, {
      minify: true,
      // Preserve legal comments (licenses) at the top of files
      legalComments: 'inline',
    });

    if (result.warnings.length > 0) {
      for (const w of result.warnings) {
        console.warn(`  ⚠  ${rel}: ${w.text}`);
      }
    }

    await Deno.writeTextFile(filePath, result.code);
    const afterSize = new TextEncoder().encode(result.code).byteLength;

    totalBefore += beforeSize;
    totalAfter += afterSize;

    const saved = beforeSize - afterSize;
    const pct = beforeSize > 0 ? ((saved / beforeSize) * 100).toFixed(1) : '0.0';
    console.log(
      `  ${rel.padEnd(45)} ${fmtKB(beforeSize)} → ${fmtKB(afterSize)}  (−${pct}%)`,
    );
  } catch (err: unknown) {
    skipped++;
    const msg = err instanceof Error ? err.message.split('\n')[0] : String(err);
    console.warn(`  ⚠  ${rel}: skipped (parse error: ${msg})`);
  }
}

const totalSaved = totalBefore - totalAfter;
const totalPct = totalBefore > 0 ? ((totalSaved / totalBefore) * 100).toFixed(1) : '0.0';
console.log(
  `\n✅ Total: ${fmtKB(totalBefore)} → ${fmtKB(totalAfter)}  (−${totalPct}%, saved ${
    fmtKB(totalSaved)
  })\n`,
);

esbuild.stop();

// ── Helpers ────────────────────────────────────────────────────────

function fmtKB(bytes: number): string {
  return (bytes / 1024).toFixed(1).padStart(7) + ' KB';
}
