/**
 * yolo-inference.js — runs the model that finds QR codes in a camera frame,
 * with ONNX Runtime Web.
 *
 * The model is a YOLO model trained on the card photos. Which file it is,
 * and which file the runner is, the engine says (MODEL and RUNNER in
 * kipukas-server/src/routes/qr.rs). The runner is ONNX Runtime Web as it is
 * released, served by the site (scripts/fetch-onnxruntime.ts). The model
 * runs on WebGPU where the browser has it, and on WASM where not.
 *
 * Only the running is here. What goes into the model and what is made of
 * what comes out are the engine's (kipukas-server/src/scan.rs).
 *
 * The model and the runner are fetched when computer vision is first turned
 * on, and the service worker keeps both from then on (sw-src.js), so that it
 * works offline afterwards. Until both have been fetched it cannot:
 * initSession fails, and can be called again later.
 *
 * Usage (from the engine's worker):
 *   import { initSession, runDetection } from './yolo-inference.js';
 *   await initSession(model, runner); // the addresses the engine names
 *   const { data, places } = await runDetection(input);
 */

// The two files the runner loads from beside itself when it first makes a
// model ready, by the names its release gives them (FILES in
// scripts/fetch-onnxruntime.ts).
const PARTS = {
  mjs: 'ort-wasm-simd-threaded.jsep.mjs',
  wasm: 'ort-wasm-simd-threaded.jsep.wasm',
};

// The side of the square picture the model takes (MODEL_SIDE in scan.rs).
const SIDE = 640;

// A copy of the runner that has not failed, and how many copies have been
// taken.
let ort = null;
let copies = 0;
// The model ready to run, and the copy of the runner it was made with.
let session = null;
let made = null;
let backend = null;
// The address of the model the session was made from.
let using = null;

/**
 * Fetch the model and the runner, and make the model ready to run.
 * Tries WebGPU first for GPU acceleration, falls back to WASM.
 *
 * Rejects if the model or the runner cannot be fetched (no connection, and
 * neither kept from before). It can be called again.
 *
 * @param {string} modelUrl - the address of the .onnx model file
 * @param {string} runnerUrl - the address of the runner's script
 * @returns {Promise<string|null>} the backend in use ('webgpu' or 'wasm'),
 *   or null if the model was fetched and could be run on neither
 */
export async function initSession(modelUrl, runnerUrl) {
  if (session && using === modelUrl) return backend;
  session = null;
  using = null;

  // The model first: without it there is nothing to run, and nothing of the
  // runner has been started that a failure here could leave half done.
  const response = await fetch(modelUrl);
  if (!response.ok) throw new Error(`${modelUrl}: ${response.status}`);
  const model = new Uint8Array(await response.arrayBuffer());

  // A copy of the runner. One that could not start stays that way, and so
  // in some browsers does a script that could not be fetched (Firefox does
  // not ask again for an address an import of it failed on). So the try
  // after a failure takes another copy: the same files under addresses the
  // browser has not been given before. That goes for the runner's script
  // and for the script it imports from beside itself. Its third file is
  // fetched as data, which is asked for again as it is.
  if (!ort) {
    const copy = copies;
    copies += 1;
    const address = (file, again) => {
      const url = new URL(file, new URL(runnerUrl, self.location.href));
      if (again && copy > 0) url.searchParams.set('copy', copy);
      return url.href;
    };
    const taken = await import(address(runnerUrl, true));
    taken.env.wasm.wasmPaths = { mjs: address(PARTS.mjs, true), wasm: address(PARTS.wasm, false) };
    ort = taken;
  }
  const runner = ort;

  // Try WebGPU first (3-8× faster on mobile GPU), fall back to WASM/CPU.
  // User opts in via the CV toggle, so both backends are acceptable.
  for (const b of ['webgpu', 'wasm']) {
    try {
      runner.env.wasm.numThreads = 1; // Single thread in worker
      session = await runner.InferenceSession.create(model, {
        executionProviders: [b],
        graphOptimizationLevel: 'all',
      });
      made = runner;
      backend = b;
      using = modelUrl;
      return backend;
    } catch (err) {
      console.warn(`[yolo-inference] ${b} backend failed:`, err.message);
      session = null;
    }
  }
  // This copy of the runner may be why: it is not used again.
  ort = null;
  return null;
}

/**
 * Run the model on a frame.
 *
 * @param {Float32Array} input - the frame as the model takes it: three
 *   planes of SIDE by SIDE numbers (the engine's scan_model_input)
 * @returns {Promise<{data: Float32Array, places: number}>} the model's
 *   output, five rows of `places` numbers (for the engine's scan_found)
 */
export async function runDetection(input) {
  if (!session) throw new Error('[yolo-inference] Session not initialized');
  const feeds = { [session.inputNames[0]]: new made.Tensor('float32', input, [1, 3, SIDE, SIDE]) };
  const output = (await session.run(feeds))[session.outputNames[0]];
  return { data: output.data, places: output.dims[2] };
}

/**
 * Whether the model is ready to run.
 * @returns {boolean}
 */
export function isReady() {
  return session !== null;
}
