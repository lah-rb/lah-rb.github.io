/**
 * search.js — Full-text search through rendered content with results dropdown.
 * Exposes: KipukasSearch
 */

const KipukasSearch = (() => {
    let debounceTimer = null;
    let sections = []; // {id, title, text}

    function indexContent() {
        sections = [];
        const content = document.getElementById('content');
        const allHeaders = content.querySelectorAll('h2, h3');

        allHeaders.forEach((header, idx) => {
            const id = header.id || '';
            const title = header.textContent.replace('#', '').trim();
            if (!title) return;

            // Gather text between this header and the next
            let text = '';
            let sibling = header.nextElementSibling;
            while (sibling && !sibling.matches('h2, h3')) {
                text += ' ' + sibling.textContent;
                sibling = sibling.nextElementSibling;
            }

            sections.push({ id, title, text: text.trim() });
        });
    }

    function search(query) {
        if (!query || query.length < 2) return [];

        const terms = query.toLowerCase().split(/\s+/).filter(t => t.length > 1);
        const results = [];

        sections.forEach(section => {
            const haystack = (section.title + ' ' + section.text).toLowerCase();
            const allMatch = terms.every(term => haystack.includes(term));
            if (!allMatch) return;

            // Find a snippet around the first match
            const fullText = section.title + ' ' + section.text;
            const lowerFull = fullText.toLowerCase();
            const firstIdx = lowerFull.indexOf(terms[0]);
            const snippetStart = Math.max(0, firstIdx - 40);
            const snippetEnd = Math.min(fullText.length, firstIdx + terms[0].length + 80);
            let snippet = (snippetStart > 0 ? '...' : '') +
                fullText.slice(snippetStart, snippetEnd) +
                (snippetEnd < fullText.length ? '...' : '');

            // Highlight terms in snippet
            terms.forEach(term => {
                const regex = new RegExp(`(${escapeRegex(term)})`, 'gi');
                snippet = snippet.replace(regex, '<mark class="search-highlight">$1</mark>');
            });

            results.push({
                id: section.id,
                title: section.title,
                snippet,
            });
        });

        return results.slice(0, 15);
    }

    function escapeRegex(str) {
        return str.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    }

    function renderResults(results, container) {
        if (results.length === 0) {
            container.innerHTML = `
                <div class="p-4 text-center text-slate-500 text-sm">
                    No results found
                </div>`;
            container.classList.remove('hidden');
            return;
        }

        container.innerHTML = results.map(r => `
            <a href="#${r.id}" class="block px-4 py-3 hover:bg-sky-50 border-b border-sky-100 last:border-0 transition-colors search-result-item" data-target-id="${r.id}">
                <div class="font-semibold text-sky-900 text-sm">${r.title}</div>
                <div class="text-xs text-slate-600 mt-1 leading-relaxed">${r.snippet}</div>
            </a>
        `).join('');
        container.classList.remove('hidden');

        // Attach click handlers
        container.querySelectorAll('.search-result-item').forEach(item => {
            item.addEventListener('click', (e) => {
                e.preventDefault();
                const targetId = item.dataset.targetId;
                const target = document.getElementById(targetId);
                if (target) {
                    target.scrollIntoView({ behavior: 'smooth', block: 'start' });
                    history.replaceState(null, '', `#${targetId}`);
                    // Brief highlight
                    target.style.background = '#fef3c7';
                    setTimeout(() => { target.style.background = ''; }, 2000);
                }
                hideResults();
                document.getElementById('search-input').value = '';
                document.getElementById('search-clear').classList.add('hidden');
            });
        });
    }

    function hideResults() {
        document.getElementById('search-results').classList.add('hidden');
    }

    function init() {
        const input = document.getElementById('search-input');
        const resultsEl = document.getElementById('search-results');
        const clearBtn = document.getElementById('search-clear');

        input.addEventListener('input', () => {
            const q = input.value.trim();
            clearBtn.classList.toggle('hidden', !q);

            clearTimeout(debounceTimer);
            debounceTimer = setTimeout(() => {
                if (q.length < 2) {
                    hideResults();
                    return;
                }
                const results = search(q);
                renderResults(results, resultsEl);
            }, 200);
        });

        input.addEventListener('keydown', (e) => {
            if (e.key === 'Escape') {
                input.value = '';
                clearBtn.classList.add('hidden');
                hideResults();
                input.blur();
            }
        });

        clearBtn.addEventListener('click', () => {
            input.value = '';
            clearBtn.classList.add('hidden');
            hideResults();
            input.focus();
        });

        // Close results when clicking outside
        document.addEventListener('click', (e) => {
            if (!document.getElementById('search-wrapper').contains(e.target)) {
                hideResults();
            }
        });

        // Keyboard shortcut: Ctrl/Cmd + K to focus search
        document.addEventListener('keydown', (e) => {
            if ((e.ctrlKey || e.metaKey) && e.key === 'k') {
                e.preventDefault();
                input.focus();
                input.select();
            }
        });
    }

    return { indexContent, init };
})();
