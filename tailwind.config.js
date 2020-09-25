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
    extend: { backgroundImage: theme => ({
      'hero-pattern': "url('/assets/images/topography.svg')",
      'footer-texture': "url('/img/footer-texture.png')"})
    },
  },
  variants: {
    backgroundColor: ['responsive', 'focus', 'hover', 'active'],
    boxShadow: ['responsive', 'focus', 'hover', 'active']
  },
  plugins: [],
}
