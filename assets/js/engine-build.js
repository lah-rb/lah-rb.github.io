/**
 * engine-build.js — which build of the engine this browser can run.
 *
 * The engine is built twice (deno task build:wasm): with WebAssembly's SIMD
 * instructions, which make decoding the cards' art about twice as fast, and
 * without them, for a browser that does not have them (Safari before 16.4,
 * so every iPhone that stops at iOS 15). A browser cannot load a module that
 * holds instructions it does not know, so which build to fetch is settled
 * before either is fetched.
 *
 * Usage (from a module worker):
 *   import init from '../js-wasm/kipukas-server-pkg/kipukas_server.js';
 *   import { engineBuild } from './engine-build.js';
 *   await init(engineBuild());
 */

// The smallest module that uses SIMD: one function, which gives a 128-bit
// value. A browser without SIMD says it is not a valid module.
// deno-fmt-ignore
const USES_SIMD = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, // "\0asm", version 1
  1, 5, 1, 96, 0, 1, 123, // a type: no arguments, one v128 result
  3, 2, 1, 0, // one function of that type
  10, 10, 1, 8, 0, 65, 0, 253, 15, 253, 98, 11, // i32.const 0, i8x16.splat, i8x16.popcnt
]);

/**
 * Whether this browser has WebAssembly's SIMD instructions.
 *
 * @returns {boolean}
 */
export function hasSimd() {
  try {
    return WebAssembly.validate(USES_SIMD);
  } catch (_) {
    return false;
  }
}

/**
 * What to give the engine's `init`: nothing, for the build with SIMD (the
 * one `init` fetches when not told otherwise), or the address of the build
 * without.
 *
 * @returns {{module_or_path: URL}|undefined}
 */
export function engineBuild() {
  if (hasSimd()) return undefined;
  return {
    module_or_path: new URL(
      '../js-wasm/kipukas-server-pkg/kipukas_server_plain_bg.wasm',
      import.meta.url,
    ),
  };
}
