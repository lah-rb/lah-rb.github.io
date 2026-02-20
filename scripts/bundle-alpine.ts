#!/usr/bin/env -S deno run -A

/**
 * bundle-alpine.ts — Bundles Alpine.js + plugins into a single minified file.
 *
 * Output: assets/js/alpine.bundle.min.js
 *
 * This replaces the 6 CDN <script> tags in _layouts/default.html with one
 * locally-hosted, minified bundle.
 */

import * as esbuild from 'esbuild';
import { dirname, join } from 'jsr:@std/path@1';
import { fromFileUrl } from 'jsr:@std/path@1/from-file-url';

const ROOT = dirname(dirname(fromFileUrl(import.meta.url)));
const ENTRY_POINT = join(ROOT, 'scripts', '_alpine-entry.ts');
const OUTPUT_FILE = join(ROOT, 'assets', 'js', 'alpine.bundle.min.js');

// Create a virtual entry point that imports Alpine + all plugins
const entryContent = `
import Alpine from 'alpinejs';
import persist from '@alpinejs/persist';
import intersect from '@alpinejs/intersect';
import focus from '@alpinejs/focus';
import anchor from '@alpinejs/anchor';
import collapse from '@alpinejs/collapse';

// Register plugins before Alpine starts
Alpine.plugin(persist);
Alpine.plugin(intersect);
Alpine.plugin(focus);
Alpine.plugin(anchor);
Alpine.plugin(collapse);

// Start Alpine (deferred to match CDN behavior so other scripts can register)
queueMicrotask(() => Alpine.start());

// Expose globally for debugging if needed
(window as any).Alpine = Alpine;
`;

// Write the virtual entry point
await Deno.writeTextFile(ENTRY_POINT, entryContent);

try {
  const result = await esbuild.build({
    entryPoints: [ENTRY_POINT],
    bundle: true,
    minify: true,
    format: 'iife',
    target: ['es2020'],
    outfile: OUTPUT_FILE,
    logLevel: 'info',
  });

  if (result.errors.length > 0) {
    console.error('❌ esbuild errors:', result.errors);
    Deno.exit(1);
  }

  const stat = await Deno.stat(OUTPUT_FILE);
  const sizeKB = (stat.size / 1024).toFixed(1);
  console.log(`✅ Alpine.js bundle: ${OUTPUT_FILE} (${sizeKB} KB)`);
} finally {
  // Clean up the virtual entry point
  try {
    await Deno.remove(ENTRY_POINT);
  } catch {
    // ignore
  }
  esbuild.stop();
}
