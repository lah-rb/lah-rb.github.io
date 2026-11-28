/* tslint:disable */
/* eslint-disable */

/**
 * Decode a JXL file to a 24-bit BMP buffer, downscaled so its longest side is
 * at most `max_dim` pixels (0 = no downscale).
 *
 * Returns the BMP bytes as a `Uint8Array` to JS. Errors become a thrown
 * `JsError` so the Service Worker can fall back / surface a broken image.
 */
export function decode_jxl(bytes: Uint8Array, max_dim: number): Uint8Array;

/**
 * Answer a request, shaped like an HTTP one.
 *
 * Called from JavaScript (the engine's worker) via wasm-bindgen.
 *
 * # Arguments
 * * `method` — `GET` or `POST`
 * * `path`   — the route (e.g., "/api/type-matchup")
 * * `query`  — the query string, with its `?` (e.g., "?part=result"), or empty
 * * `body`   — a POST's body: form fields, or JSON on the routes between the
 *   engine and what holds it. Empty for a GET.
 *
 * # Returns
 * For a view's route, the HTML to patch into the page. Otherwise a short
 * reply, or JSON (`/api/handover`).
 */
export function handle_request(method: string, path: string, query: string, body: string): string;

/**
 * What the model gave for the frame in hand: `output` is its five rows of
 * `places` numbers (see [`boxes`]). From here on the reader is offered the
 * boxes and not the whole frame. Gives the boxes back for the page to draw
 * over the camera's picture: five numbers a box (left, top, width, height,
 * how sure), in the frame's pixels.
 */
export function scan_found(output: Float32Array, places: number): Float32Array;

/**
 * A camera frame to look for a code in: four bytes a pixel. It takes the
 * place of the frame before it. False, and nothing is in hand, if the
 * bytes are not a picture of that size.
 */
export function scan_frame(rgba: Uint8Array, width: number, height: number): boolean;

/**
 * The height of the picture [`scan_next`] gave last.
 */
export function scan_height(): number;

/**
 * The frame in hand as the model takes it: see [`model_input`].
 */
export function scan_model_input(): Float32Array;

/**
 * The next picture of the frame in hand for the reader, four bytes a
 * pixel, or nothing when there are no more: see [`Scan::next`]. Its size
 * is [`scan_width`] by [`scan_height`].
 */
export function scan_next(): Uint8Array | undefined;

/**
 * The width of the picture [`scan_next`] gave last.
 */
export function scan_width(): number;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly handle_request: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number];
    readonly scan_frame: (a: number, b: number, c: number, d: number) => number;
    readonly scan_model_input: () => [number, number];
    readonly scan_found: (a: number, b: number, c: number) => [number, number];
    readonly scan_next: () => [number, number];
    readonly scan_width: () => number;
    readonly scan_height: () => number;
    readonly decode_jxl: (a: number, b: number, c: number) => [number, number, number, number];
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __externref_table_dealloc: (a: number) => void;
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
