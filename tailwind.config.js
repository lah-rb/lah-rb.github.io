module.exports = {
  future: {
    removeDeprecatedGapUtilities: true,
    purgeLayersByDefault: true,
  },
  purge: [],
  theme: {
    extend: {},
  },
  variants: {
    backgroundColor: ['responsive', 'focus', 'hover', 'active'],
    boxShadow: ['responsive', 'focus', 'hover', 'active']
  },
  plugins: [],
}
