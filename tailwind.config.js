/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    './*.{html,js,yml}',
    './_layouts/*.{html,js,yml}',
    './_posts/*.{html,js,yml}',
    './_data/*.{html,js,yml}',
    './_includes/*.{html,js,yml}',
    './sections/*.{html,js,yml}',
  ],
  theme: {
    extend: {
      backgroundImage: theme => ({
      'hero-pattern': "url('/assets/utility_images/topography.svg')"
    }),
      colors: {
        'kip-red': '#9c2828',
        'kip-goldenrod': '#b87a19',
        'kip-drk-goldenrod': '#7A5015',
        'kip-drk-sienna': '#341c17'
      },
      strokeWidth: {
        '5': '5px',
      }
    },
  },
  variants: {
    backgroundColor: ['responsive', 'focus', 'hover', 'active'],
    boxShadow: ['responsive', 'focus', 'hover', 'active']
  },
  plugins: [
    require('@tailwindcss/typography'),
  ],
}
