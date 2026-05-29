/**
 * build-images.ts
 *
 * Generates one progressive JPEG XL per card from the full-size masters in
 * assets/images/, then runs the native jxl_probe to record each file's
 * DC-prefix length (the bytes the Service Worker Range-fetches for grid
 * previews) into assets/images/jxl-manifest.json.
 *
 * Single source of truth per card — no x1..x5 thumbnail tiers. The browser
 * decodes .jxl via the in-browser WASM worker (see assets/js/kipukas-worker.js).
 *
 * Requires: cjxl (libjxl) and sips (macOS) on PATH; cargo for the probe.
 * Run: deno task build:images
 */

import { walk } from 'jsr:@std/fs@1/walk';
import { join } from 'jsr:@std/path@1';

const IMAGES_DIR = join(Deno.cwd(), 'assets', 'images');
const QUALITY = '90';

async function run(cmd: string, args: string[]): Promise<string> {
  const { code, stdout, stderr } = await new Deno.Command(cmd, {
    args,
    stdout: 'piped',
    stderr: 'piped',
  }).output();
  if (code !== 0) {
    throw new Error(`${cmd} ${args.join(' ')} failed:\n${new TextDecoder().decode(stderr)}`);
  }
  return new TextDecoder().decode(stdout);
}

async function main() {
  // Masters are the full-size .webp in assets/images/ (top level only).
  const masters: string[] = [];
  for await (const entry of walk(IMAGES_DIR, { exts: ['.webp'], maxDepth: 1 })) {
    if (entry.isFile) masters.push(entry.path);
  }
  if (masters.length === 0) {
    console.warn('[build-images] no .webp masters found — nothing to convert');
  }

  const jxlPaths: string[] = [];
  for (const src of masters) {
    const base = src.replace(/\.webp$/i, '');
    const jxl = `${base}.jxl`;
    const tmpPng = `${base}.tmp.png`;
    // webp → png (cjxl input) → progressive jxl. group_order=1 + -p put the
    // DC/low-res image at the front so a tiny prefix yields a preview.
    await run('sips', ['-s', 'format', 'png', src, '--out', tmpPng]);
    await run('cjxl', [tmpPng, jxl, '-q', QUALITY, '-p', '--group_order=1']);
    await Deno.remove(tmpPng);
    jxlPaths.push(jxl);
  }
  console.log(`[build-images] encoded ${jxlPaths.length} .jxl from masters`);

  // Probe DC-prefix length for every file (shared decode logic with the worker).
  const json = await run('cargo', [
    'run',
    '--release',
    '--quiet',
    '--manifest-path',
    join(Deno.cwd(), 'kipukas-server', 'Cargo.toml'),
    '--bin',
    'jxl_probe',
    '--',
    ...jxlPaths,
  ]);
  const manifestPath = join(IMAGES_DIR, 'jxl-manifest.json');
  await Deno.writeTextFile(manifestPath, json.trim() + '\n');
  console.log(`[build-images] wrote DC-prefix manifest → ${manifestPath}`);
}

main();
