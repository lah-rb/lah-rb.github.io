/**
 * app.js — Orchestrates all modules: markdown loading, sidebar, search, chat, print.
 */

(async function init() {
    // 1. Initialize sidebar toggle behavior immediately
    KipukasSidebar.initToggle();

    // 2. Initialize chat widget
    KipukasChat.init();

    // 3. Initialize search event handlers
    KipukasSearch.init();

    // 4. Load and render markdown content
    const headers = await KipukasMarkdown.loadAndRenderMarkdown();

    // 5. Build TOC from parsed headers
    if (headers.length > 0) {
        KipukasSidebar.buildTOC(headers);
    }

    // 6. Index content for search
    KipukasSearch.indexContent();

    // 7. Print / Export to PDF — check for pre-built PDF at load time
    const printBtn = document.getElementById('print-btn');
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

    // 8. Handle internal anchor clicks within rendered content
    document.getElementById('content').addEventListener('click', (e) => {
        const link = e.target.closest('a[href^="#"]');
        if (link) {
            e.preventDefault();
            const targetId = link.getAttribute('href').slice(1);
            const target = document.getElementById(targetId);
            if (target) {
                target.scrollIntoView({ behavior: 'smooth', block: 'start' });
                history.replaceState(null, '', `#${targetId}`);
                // Brief highlight
                target.style.background = '#fef3c7';
                target.style.transition = 'background 0.3s';
                setTimeout(() => { target.style.background = ''; }, 2000);
            }
        }
    });

    // 9. Handle browser back/forward with hash changes
    window.addEventListener('hashchange', () => {
        const hash = window.location.hash.slice(1);
        if (hash) {
            const target = document.getElementById(hash);
            if (target) {
                target.scrollIntoView({ behavior: 'smooth', block: 'start' });
            }
        }
    });
})();
