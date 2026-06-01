// svgo config for optimizing oversized vector UI SVGs (e.g. the toolbar logo).
// Path-heavy files shrink most from coordinate-precision reduction; keep viewBox
// so the <img> still scales correctly.
export default {
  multipass: true,
  plugins: [
    {
      name: 'preset-default',
      params: {
        overrides: {
          // Coordinate-precision reduction is the safe, high-impact lever.
          // 2 decimals keeps glyph curves intact (precision 1 distorted text).
          convertPathData: { floatPrecision: 2 },
          cleanupNumericValues: { floatPrecision: 2 },
          convertTransform: { floatPrecision: 2 },
          // Never drop viewBox — the toolbar sizes via CSS and relies on it.
          removeViewBox: false,
          // The wordmark is outlined text with evenodd fills; merging glyph
          // paths shatters them, so keep paths separate.
          mergePaths: false,
        },
      },
    },
  ],
};
