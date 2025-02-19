/** @type {import('tailwindcss').Config} */

const defaultTheme = require('tailwindcss/defaultTheme')

module.exports = {
  content: [
    './*.{html,js,yml}',
    './_layouts/*.{html,js,yml}',
    './_posts/*.{html,js,yml}',
    './_data/*.{html,js,yml}',
    './_includes/*.{html,js,yml}',
    './sections/*.{html,js,yml}',
    './game_rules/*.{html,js,yml}',
  ],
  safelist: [
    'grid-cols-3',
    'grid-cols-4',
    'grid-cols-5',
    'grid-cols-6',
    'grid-cols-7',
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
      },
      screens: {
        'xs': { 'max':'450px' },
        'sm': '450px',
        'md': '768px', // md and past are default values
        'lg': '1024px',
        'xl': '1280px',
        '2xl': '1536px'
      },
    },
  },
  variants: {
    backgroundColor: ['responsive', 'focus', 'hover', 'active'],
    boxShadow: ['responsive', 'focus', 'hover', 'active']
  },
  plugins: [
    require('@tailwindcss/typography'),
    require('@tailwindcss/forms')
  ],
}
