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
          // floatPrecision 3 keeps the error tolerance tight enough that svgo
          // does NOT linearize curves (precision 2 turned them polygonal), so
          // the smooth strokes survive. (straightCurves:false would be more
          // direct but crashes svgo 3.3.3.)
          convertPathData: { floatPrecision: 3 },
          cleanupNumericValues: { floatPrecision: 3 },
          convertTransform: { floatPrecision: 3 },
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
