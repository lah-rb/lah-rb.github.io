#!/usr/bin/env -S deno run -A

/**
 * fetch-onnxruntime.ts — puts ONNX Runtime Web among the site's own files.
 *
 * ONNX Runtime Web runs the model that finds QR codes for the scanner
 * (assets/js/yolo-inference.js). The site serves it itself, as it does
 * Alpine and the rest, so that nothing is fetched from another origin.
 *
 * Output: assets/js-wasm/onnxruntime-web-<version>/ (not committed)
 *
 * The files are the release's own, unchanged: the package is fetched from
 * the npm registry once, kept in .cache/, and held to the hash recorded
 * below every time this runs. Only the files a browser fetches are taken
 * out of it (FILES): the runner, and the two it loads from beside itself.
 *
 * To move to another release:
 *   1. VERSION and INTEGRITY below. The hash is the registry's own for the
 *      release, `dist.integrity` in
 *      https://registry.npmjs.org/onnxruntime-web/<version>
 *   2. RUNNER in kipukas-server/src/routes/qr.rs, which names the folder.
 *      This script stops if the two disagree.
 *   3. The version in _about/open_stack.html and CONTRIBUTING.md.
 * The folder carries the version because the service worker keeps these
 * files by their address and does not ask for an address again (sw-src.js).
 */

import { encodeBase64 } from 'jsr:@std/encoding@1/base64';
import { dirname, join } from 'jsr:@std/path@1';
import { fromFileUrl } from 'jsr:@std/path@1/from-file-url';

const VERSION = '1.21.0';
const INTEGRITY =
  'sha512-adzOe+7uI7lKz6pQNbAsLMQd2Fq5Jhmoxd8LZjJr8m3KvbFyiYyRxRiC57/XXD+jb18voppjeGAjoZmskXG+7A==';

// What a browser fetches: the runner (WebGPU and WASM in one script), and
// the two files it loads when it first makes a model ready. Their names are
// the release's (PARTS in assets/js/yolo-inference.js names the two).
const FILES = [
  'ort.all.min.mjs',
  'ort-wasm-simd-threaded.jsep.mjs',
  'ort-wasm-simd-threaded.jsep.wasm',
];

const ROOT = dirname(dirname(fromFileUrl(import.meta.url)));
const PACKAGE = `https://registry.npmjs.org/onnxruntime-web/-/onnxruntime-web-${VERSION}.tgz`;
const KEPT = join(ROOT, '.cache', `onnxruntime-web-${VERSION}.tgz`);
const SERVED = join(ROOT, 'assets', 'js-wasm');
const FOLDER = `onnxruntime-web-${VERSION}`;
const ENGINE = join(ROOT, 'kipukas-server', 'src', 'routes', 'qr.rs');

function stop(why: string): never {
  console.error(`❌ ${why}`);
  Deno.exit(1);
}

/** The package's hash, as the npm registry writes one. */
async function integrity(bytes: Uint8Array<ArrayBuffer>): Promise<string> {
  return `sha512-${encodeBase64(await crypto.subtle.digest('SHA-512', bytes))}`;
}

/** The package: the copy kept from before, or fetched now and kept. */
async function release(): Promise<Uint8Array<ArrayBuffer>> {
  try {
    const kept = await Deno.readFile(KEPT);
    if (await integrity(kept) === INTEGRITY) return kept;
    console.warn(`The copy kept at ${KEPT} is not the release: fetching it again.`);
  } catch (e) {
    if (!(e instanceof Deno.errors.NotFound)) throw e;
  }
  console.log(`Fetching ${PACKAGE}`);
  const response = await fetch(PACKAGE);
  if (!response.ok) stop(`${PACKAGE}: ${response.status}`);
  const fetched = new Uint8Array(await response.arrayBuffer());
  const found = await integrity(fetched);
  if (found !== INTEGRITY) {
    stop(
      `What was fetched is not the release recorded here.\n   expected ${INTEGRITY}\n   found    ${found}`,
    );
  }
  await Deno.mkdir(dirname(KEPT), { recursive: true });
  await Deno.writeFile(KEPT, fetched);
  return fetched;
}

// The engine names the runner's address to its worker. A release changed
// here and not there would leave the scanner asking for a folder that is
// not served.
const named = `"/assets/js-wasm/${FOLDER}/${FILES[0]}"`;
if (!(await Deno.readTextFile(ENGINE)).includes(named)) {
  stop(
    `kipukas-server/src/routes/qr.rs does not name ${named} (RUNNER). It and VERSION here go together.`,
  );
}

await release();

// Taken out into a folder beside the one served, and put in its place whole.
const out = join(SERVED, FOLDER);
const fresh = `${out}.new`;
await Deno.remove(fresh, { recursive: true }).catch(() => {});
await Deno.mkdir(fresh, { recursive: true });
const { code } = await new Deno.Command('tar', {
  // -m: the files are dated now, not as the package dates them (1985).
  args: [
    '-xzmf',
    KEPT,
    '-C',
    fresh,
    '--strip-components',
    '2',
    ...FILES.map((file) => `package/dist/${file}`),
  ],
  stdout: 'inherit',
  stderr: 'inherit',
}).output();
if (code !== 0) stop(`tar could not take the files out of ${KEPT}`);

// Only this release is served: an earlier one's folder goes.
for await (const entry of Deno.readDir(SERVED)) {
  if (
    entry.isDirectory && entry.name.startsWith('onnxruntime-web-') && entry.name !== `${FOLDER}.new`
  ) {
    await Deno.remove(join(SERVED, entry.name), { recursive: true });
  }
}
await Deno.rename(fresh, out);

for (const file of FILES) {
  const { size } = await Deno.stat(join(out, file));
  console.log(`   ${file}  ${(size / 1024).toFixed(0)} KB`);
}
console.log(`✅ ONNX Runtime Web ${VERSION}: assets/js-wasm/${FOLDER}/`);
