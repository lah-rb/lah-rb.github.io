/* tslint:disable */
/* eslint-disable */

/**
 * Decode a QR code from raw RGBA pixel data using multi-strategy rqrr cascade.
 *
 * Called from kipukas-worker.js on each camera frame.
 * Returns decoded text or empty string if no QR found.
 */
export function decode_qr_frame(rgba: Uint8Array, width: number, height: number): string;

/**
 * Process an HTTP-like request and return an HTML fragment.
 *
 * Called from JavaScript (Web Worker) via wasm-bindgen.
 *
 * # Arguments
 * * `method` — HTTP method (e.g., "GET", "POST")
 * * `path`   — URL path (e.g., "/api/type-matchup")
 * * `query`  — Query string (e.g., "?atk[]=Brutal&def[]=Avian")
 * * `body`   — Request body (e.g., POST form data). Empty string for GET requests.
 *
 * # Returns
 * An HTML string fragment suitable for HTMX to swap into the DOM.
 */
export function handle_request(method: string, path: string, query: string, body: string): string;

/**
 * Reset the frame accumulator (call when scanner closes).
 */
export function reset_qr_frames(): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly handle_request: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number];
    readonly decode_qr_frame: (a: number, b: number, c: number, d: number) => [number, number];
    readonly reset_qr_frames: () => void;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
