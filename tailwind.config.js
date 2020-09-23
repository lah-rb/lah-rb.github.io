module.exports = {
  future: {
    removeDeprecatedGapUtilities: true,
    purgeLayersByDefault: true,
  },
  purge: [
    './404.html',
    './index.html',
    './_layouts/**.html',
    './_posts/**.html',
    './_site/**.html',
  ],
  theme: {
    extend: {},
  },
  variants: {
    backgroundColor: ['responsive', 'focus', 'hover', 'active'],
    boxShadow: ['responsive', 'focus', 'hover', 'active']
  },
  plugins: [],
}
