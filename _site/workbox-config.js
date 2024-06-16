module.exports = {
	globDirectory: '_site/',
	globPatterns: [
		'**/*.{html,png,css,webp,js,wasm,svg,yml,ico,pdf,json,webmanifest}'
	],
	swDest: './sw.js',
	ignoreURLParametersMatching: [
		/^utm_/,
		/^fbclid$/
	],
	runtimeCaching: [{
		urlPattern: /https:\/\/www.kipukas.cards/,
		handler: 'NetworkFirst'
		}],
};