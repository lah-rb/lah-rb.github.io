/**
 * build-images.ts
 *
 * Generates one JPEG XL per card from the full-size masters in assets/images/.
 *
 * Single source of truth per card — no x1..x5 thumbnail tiers. The browser
 * decodes .jxl at full resolution via the in-browser WASM decode worker pool
 * (see assets/js/jxl-decode-worker.js); the grid downscales per request (?d=).
 *
 * Requires: cjxl (libjxl) and sips (macOS) on PATH.
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
}

main();
