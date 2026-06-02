/**
 * bench-jxl.ts — headless A/B for the JXL decode WASM (kipukas-server).
 *
 * Times decode_jxl(bytes, maxDim) over N iterations on a sample card image,
 * using the matching generated glue + bytes. Run before vs after enabling
 * +simd128 (each with its own freshly-built glue/wasm).
 *
 *   deno run -A scripts/bench-jxl.ts [wasmPath] [imgPath]
 *
 * Defaults to the current built wasm and assets/images/404.jxl.
 * Note: the glue (kipukas_server.js) and the wasm must come from the SAME
 * build, so bench right after each build rather than mixing.
 */
import init, { decode_jxl } from '../assets/js-wasm/kipukas-server-pkg/kipukas_server.js';

const wasmPath = Deno.args[0] ?? 'assets/js-wasm/kipukas-server-pkg/kipukas_server_bg.wasm';
const imgPath = Deno.args[1] ?? 'assets/images/404.jxl';

const wasmBytes = await Deno.readFile(wasmPath);
try {
  await init({ module_or_path: wasmBytes });
} catch {
  await init(wasmBytes);
}

const img = await Deno.readFile(imgPath);

function bench(maxDim: number, iters: number) {
  for (let i = 0; i < 3; i++) decode_jxl(img, maxDim); // warmup
  const times: number[] = [];
  for (let i = 0; i < iters; i++) {
    const t0 = performance.now();
    const out = decode_jxl(img, maxDim);
    times.push(performance.now() - t0);
    if (!out || out.length === 0) throw new Error('empty decode');
  }
  times.sort((a, b) => a - b);
  return { median: times[Math.floor(times.length / 2)], min: times[0], iters };
}

console.log(`wasm: ${wasmPath} (${(wasmBytes.length / 1024).toFixed(1)} KB)`);
console.log(`img:  ${imgPath} (${(img.length / 1024).toFixed(1)} KB)`);
for (const maxDim of [512, 4096]) {
  const r = bench(maxDim, 20);
  console.log(`maxDim=${maxDim}: median=${r.median.toFixed(2)}ms  min=${r.min.toFixed(2)}ms  (n=${r.iters})`);
}
