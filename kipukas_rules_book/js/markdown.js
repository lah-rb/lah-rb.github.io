/**
 * markdown.js — Loads rules.md, parses it with marked.js, handles {#id} custom anchors.
 * Exposes: loadAndRenderMarkdown()
 */

const KipukasMarkdown = (() => {

    // ── Image classification by filename → Tailwind classes ──────────
    const IMAGE_CLASSES = [
        // QR code thumbnails (inside card-type links)
        { test: /_qr\./i, classes: 'w-20 h-20 object-cover rounded shadow-sm inline-block' },
        // Small game tokens & pieces
        { test: /\/(capital|basecamp|soul_token|d6|diel_die|d20)\./i, classes: 'w-64 h-auto rounded-lg shadow-md mx-auto block' },
        // Geography markers
        { test: /\/(A_frame|log_cabin|plain_house|modern_home|boat|tunnel|dock)\./i, classes: 'w-36 h-auto rounded-lg shadow-md mx-auto block' },
        // Signature / initials
        { test: /\/initials\./i, classes: 'w-16 h-auto !shadow-none' },
        // Book cover
        { test: /\/front_cover/i, classes: 'w-96 h-auto rounded-xl shadow-lg mx-auto block justify-center' },
        // Card art (character / species illustrations, including _back variants)
        { test: /\/(hilbert_king_of_avian_frogs|myrthvither_raven)(_back)?\./i, classes: 'w-96 h-auto rounded-xl shadow-lg' },
        // KEAL means tracker / UI elements
        { test: /\/keal_means_/i, classes: 'max-w-sm h-auto rounded-lg shadow-md' },
        // QR scanner UI screenshots
        { test: /\/(QR_modal|QR_scanner)\./i, classes: 'max-w-sm h-auto rounded-lg shadow-md' },
        // Fists tool screenshots
        { test: /\/fists_/i, classes: 'max-w-sm h-auto rounded-lg shadow-md' },
        // Recipe lookup screenshot
        { test: /\/recipe_lookup/i, classes: 'max-w-sm h-auto rounded-lg shadow-md' },
        // Diagrams (motives, placement)
        { test: /\/(motives|placement_courtesy|12_slot_rec_mat)/i, classes: 'max-w-md h-auto rounded-lg shadow mx-auto block' },
        // Map examples used in scenario walkthrough (smaller)
        { test: /\/map_top_example/i, classes: 'max-w-sm h-auto rounded-xl shadow-lg' },
        // Large map views
        { test: /\/(map_perspective|map_top)\./i, classes: 'max-w-lg h-auto rounded-xl shadow-lg mx-auto block' },
    ];

    // Default classes when no pattern matches
    const DEFAULT_IMG_CLASSES = 'max-w-md h-auto rounded-lg shadow-md';

    function classifyImage(src) {
        for (const rule of IMAGE_CLASSES) {
            if (rule.test.test(src)) return rule.classes;
        }
        return DEFAULT_IMG_CLASSES;
    }

    // ── Post-render: group adjacent images into flex rows ────────────
    function postProcessImages(contentEl) {
        contentEl.querySelectorAll('p').forEach(p => {
            const imgs = p.querySelectorAll('img');
            if (imgs.length < 2) return;

            // Check that the paragraph is predominantly images (allow links wrapping images)
            const textOnly = p.cloneNode(true);
            textOnly.querySelectorAll('img, a').forEach(el => el.remove());
            if (textOnly.textContent.trim().length > 10) return;

            // Convert to flex container
            p.classList.add('flex', 'flex-wrap', 'gap-4', 'items-start', 'justify-center', 'my-6', 'not-prose');

            // Cap each child image so they share the row
            imgs.forEach(img => {
                // Remove mx-auto / block since parent is flex now
                img.classList.remove('mx-auto', 'block');
                // Add flex-friendly sizing
                img.classList.add('flex-shrink-0');
                // For side-by-side, constrain width based on count
                const maxW = imgs.length === 2 ? 'max-w-[48%]' : 'max-w-[30%]';
                img.classList.add(maxW);
            });
        });
    }

    // Custom renderer to handle {#id} syntax in headers
    function createRenderer() {
        const renderer = new marked.Renderer();
        const headerRegex = /\{#([^}]+)\}\s*$/;

        function renderHeader(text, level) {
            const match = text.match(headerRegex);
            let id, cleanText;

            if (match) {
                id = match[1];
                cleanText = text.replace(headerRegex, '').trim();
            } else {
                // Generate slug from text
                id = text.toLowerCase()
                    .replace(/<[^>]*>/g, '')
                    .replace(/[^\w\s-]/g, '')
                    .replace(/\s+/g, '_')
                    .replace(/-+/g, '_')
                    .trim();
                cleanText = text;
            }

            // Skip empty or placeholder sections
            if (!cleanText) return '';

            const anchor = `<a class="header-anchor" href="#${id}" title="Link to this section">#</a>`;
            return `<h${level} id="${id}">${cleanText}${anchor}</h${level}>\n`;
        }

        renderer.heading = function(data) {
            // marked v12+ passes an object, older passes (text, level, raw)
            if (typeof data === 'object' && data.text !== undefined) {
                return renderHeader(data.text, data.depth);
            }
            return renderHeader(data, arguments[1]);
        };

        // Image renderer — classify by filename and inject Tailwind classes
        renderer.image = function(data) {
            let href, title, text;
            if (typeof data === 'object' && data.href !== undefined) {
                href = data.href; title = data.title; text = data.text;
            } else {
                href = data; title = arguments[1]; text = arguments[2];
            }
            const classes = classifyImage(href || '');
            const alt = (text || '').replace(/"/g, '&quot;');
            const titleAttr = title ? ` title="${title.replace(/"/g, '&quot;')}"` : '';
            return `<img src="${href}" alt="${alt}" class="${classes}"${titleAttr} loading="lazy">`;
        };

        return renderer;
    }

    function collectHeaders(contentEl) {
        const headers = [];
        contentEl.querySelectorAll('h2, h3').forEach(el => {
            if (el.id && el.textContent.replace('#', '').trim()) {
                headers.push({
                    id: el.id,
                    text: el.textContent.replace('#', '').trim(),
                    level: el.tagName === 'H2' ? 2 : 3,
                    element: el,
                });
            }
        });
        return headers;
    }

    function handleInitialHash() {
        if (window.location.hash) {
            const target = document.getElementById(window.location.hash.slice(1));
            if (target) {
                setTimeout(() => target.scrollIntoView({ behavior: 'smooth' }), 100);
            }
        }
    }

    async function loadAndRenderMarkdown() {
        const contentEl = document.getElementById('content');
        const loadingEl = document.getElementById('loading');

        // Check if content was pre-rendered at build time
        const preRendered = contentEl.querySelectorAll('h2, h3');
        if (preRendered.length > 0) {
            // Content already injected by build step — just collect headers
            if (loadingEl) loadingEl.remove();
            postProcessImages(contentEl);
            const headers = collectHeaders(contentEl);
            handleInitialHash();
            return headers;
        }

        // Dev mode: fetch and render rules.md at runtime
        try {
            const response = await fetch('rules.md');
            if (!response.ok) throw new Error(`Failed to load rules.md: ${response.status}`);
            let mdText = await response.text();

            // Convert manual page break markers to HTML divs
            mdText = mdText.replace(/<!--\s*pagebreak\s*-->/gi, '<div class="page-break"></div>');

            // Configure marked
            marked.setOptions({
                renderer: createRenderer(),
                breaks: true,
                gfm: true,
            });

            // Parse and sanitize
            const rawHtml = marked.parse(mdText);
            const cleanHtml = DOMPurify.sanitize(rawHtml, {
                ADD_ATTR: ['id', 'target', 'class', 'loading'],
            });

            // Remove loading state and insert content
            if (loadingEl) loadingEl.remove();
            contentEl.innerHTML = cleanHtml;

            // Group adjacent images into flex rows
            postProcessImages(contentEl);

            const headers = collectHeaders(contentEl);
            handleInitialHash();
            return headers;
        } catch (err) {
            console.error('Error loading markdown:', err);
            if (loadingEl) {
                loadingEl.innerHTML = `
                    <div class="text-center text-red-600">
                        <p class="text-lg font-semibold">Failed to load rules</p>
                        <p class="text-sm mt-2">${err.message}</p>
                        <button onclick="location.reload()" class="mt-4 px-4 py-2 bg-sky-600 text-white rounded-lg hover:bg-sky-700">Retry</button>
                    </div>`;
            }
            return [];
        }
    }

    return { loadAndRenderMarkdown };
})();
