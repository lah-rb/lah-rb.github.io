module.exports = {
  plugins: [
    require('tailwindcss'),
    require('autoprefixer'),
    process.env.NODE_ENV === 'production' && require('@fullhuman/postcss-purgecss')({
      content: [
        './404.html',
        './index.html',
        './_layouts/**.html',
        './_posts/**.html',
        './_site/**.html'
      ],
      defaultExtractor: content.match(/[A-Za-z0-9-_:/]+/g) || []
    })
  ]
}
