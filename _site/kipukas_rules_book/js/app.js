/**
 * app.js — Orchestrates all modules: markdown loading, sidebar, search, chat, print.
 *
 * Each module init is wrapped in try/catch so that if a privacy-focused browser's
 * content blocker prevents a script from loading (e.g. Cromite, IronFox filter
 * lists), the remaining features still work instead of cascading into total failure.
 */

/**
 * Safely scroll to a target element within #content-scroll only,
 * preventing the outer page / header from being pushed off-screen.
 */
function scrollContentTo(targetId) {
    const target = document.getElementById(targetId);
    if (!target) return;
    const scrollContainer = document.getElementById('content-scroll');
    const containerRect = scrollContainer.getBoundingClientRect();
    const targetRect = target.getBoundingClientRect();
    const offset = targetRect.top - containerRect.top + scrollContainer.scrollTop;
    scrollContainer.scrollTo({ top: offset, behavior: 'smooth' });

    // Brief highlight
    target.style.background = '#fef3c7';
    target.style.transition = 'background 0.3s';
    setTimeout(() => { target.style.background = ''; }, 2000);
}

// Safety net: if anything accidentally scrolls the outer page, snap it back
// so the header is always visible.
window.addEventListener('scroll', () => {
    if (window.scrollY !== 0) {
        window.scrollTo(0, 0);
    }
}, { passive: false });

(async function init() {
    // 1. Initialize sidebar toggle behavior immediately
    try {
        if (typeof KipukasSidebar !== 'undefined') {
            KipukasSidebar.initToggle();
        }
    } catch (e) {
        console.warn('[Kipukas] Sidebar toggle init failed:', e);
    }

    // 2. Initialize assistant widget
    try {
        if (typeof KipukasKippa !== 'undefined') {
            KipukasKippa.init();
        }
    } catch (e) {
        console.warn('[Kipukas] Assistant widget init failed:', e);
    }

    // 3. Initialize search event handlers
    try {
        if (typeof KipukasSearch !== 'undefined') {
            KipukasSearch.init();
        }
    } catch (e) {
        console.warn('[Kipukas] Search init failed:', e);
    }

    // 4. Load and render markdown content
    let headers = [];
    try {
        if (typeof KipukasMarkdown !== 'undefined') {
            headers = await KipukasMarkdown.loadAndRenderMarkdown();
        }
    } catch (e) {
        console.warn('[Kipukas] Markdown loading failed:', e);
    }

    // 5. Build TOC from parsed headers
    try {
        if (headers.length > 0 && typeof KipukasSidebar !== 'undefined') {
            KipukasSidebar.buildTOC(headers);
        }
    } catch (e) {
        console.warn('[Kipukas] TOC build failed:', e);
    }

    // 6. Index content for search
    try {
        if (typeof KipukasSearch !== 'undefined') {
            KipukasSearch.indexContent();
        }
    } catch (e) {
        console.warn('[Kipukas] Search indexing failed:', e);
    }

    // 7. Print / Export to PDF — check for pre-built PDF at load time
    try {
        const printBtn = document.getElementById('print-btn');
        if (printBtn) {
            let pdfAvailable = false;

            // Probe for the PDF in the background (non-blocking)
            fetch('kipukas_rules.pdf', { method: 'HEAD' })
                .then(r => { pdfAvailable = r.ok; })
                .catch(() => { pdfAvailable = false; });

            printBtn.addEventListener('click', () => {
                if (pdfAvailable) {
                    // PDF exists — trigger download (synchronous user gesture)
                    const a = document.createElement('a');
                    a.href = 'kipukas_rules.pdf';
                    a.download = 'kipukas_rules.pdf';
                    a.style.display = 'none';
                    document.body.appendChild(a);
                    a.click();
                    setTimeout(() => document.body.removeChild(a), 100);
                } else {
                    // No PDF available — fallback to browser print
                    window.print();
                }
            });
        }
    } catch (e) {
        console.warn('[Kipukas] Print button init failed:', e);
    }

    // 8. Handle internal anchor clicks within rendered content
    try {
        const contentEl = document.getElementById('content');
        if (contentEl) {
            contentEl.addEventListener('click', (e) => {
                const link = e.target.closest('a[href^="#"]');
                if (link) {
                    e.preventDefault();
                    const targetId = link.getAttribute('href').slice(1);
                    scrollContentTo(targetId);
                    history.replaceState(null, '', `#${targetId}`);
                }
            });
        }
    } catch (e) {
        console.warn('[Kipukas] Anchor click handler failed:', e);
    }

    // 9. Handle browser back/forward with hash changes
    window.addEventListener('hashchange', () => {
        const hash = window.location.hash.slice(1);
        if (hash) {
            scrollContentTo(hash);
        }
    });
})();
