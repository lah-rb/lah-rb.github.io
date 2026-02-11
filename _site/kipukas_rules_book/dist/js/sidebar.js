/**
 * sidebar.js — Builds TOC from headers, scroll-spy, toggle open/close.
 * Exposes: KipukasSidebar
 */

const KipukasSidebar = (() => {
    let tocLinks = [];
    let headerElements = [];
    let observer = null;

    function buildTOC(headers) {
        const toc = document.getElementById('toc');
        toc.innerHTML = '';
        tocLinks = [];
        headerElements = [];

        headers.forEach(h => {
            const a = document.createElement('a');
            a.href = `#${h.id}`;
            a.textContent = h.text;
            a.className = `toc-link${h.level === 3 ? ' toc-sub' : ''}`;
            a.dataset.targetId = h.id;

            a.addEventListener('click', (e) => {
                e.preventDefault();
                scrollContentTo(h.id);
                history.replaceState(null, '', `#${h.id}`);
                // Close sidebar on mobile
                if (window.innerWidth < 1024) {
                    closeSidebar();
                }
            });

            toc.appendChild(a);
            tocLinks.push(a);
            headerElements.push(h.element);
        });

        initScrollSpy();
    }

    function initScrollSpy() {
        if (observer) observer.disconnect();

        const scrollContainer = document.getElementById('content-scroll');

        // Use IntersectionObserver relative to scroll container
        observer = new IntersectionObserver((entries) => {
            entries.forEach(entry => {
                if (entry.isIntersecting) {
                    setActive(entry.target.id);
                }
            });
        }, {
            root: scrollContainer,
            rootMargin: '-10% 0px -80% 0px',
            threshold: 0,
        });

        headerElements.forEach(el => observer.observe(el));
    }

    function setActive(id) {
        tocLinks.forEach(link => {
            if (link.dataset.targetId === id) {
                link.classList.add('active');
                // Scroll TOC to active item
                link.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
            } else {
                link.classList.remove('active');
            }
        });
    }

    // Sidebar toggle logic
    function openSidebar() {
        const sidebar = document.getElementById('sidebar');
        const overlay = document.getElementById('sidebar-overlay');
        sidebar.classList.remove('-translate-x-full');
        overlay.classList.remove('hidden');
    }

    function closeSidebar() {
        const sidebar = document.getElementById('sidebar');
        const overlay = document.getElementById('sidebar-overlay');
        sidebar.classList.add('-translate-x-full');
        overlay.classList.add('hidden');
    }

    function toggleSidebar() {
        const sidebar = document.getElementById('sidebar');
        if (sidebar.classList.contains('-translate-x-full')) {
            openSidebar();
        } else {
            // On desktop, we want to actually hide it
            if (window.innerWidth >= 1024) {
                sidebar.classList.add('-translate-x-full');
                sidebar.classList.remove('lg:translate-x-0');
                sidebar.classList.add('lg:-translate-x-full');
            } else {
                closeSidebar();
            }
        }
    }

    function initToggle() {
        const toggleBtn = document.getElementById('sidebar-toggle');
        const closeBtn = document.getElementById('sidebar-close');
        const overlay = document.getElementById('sidebar-overlay');

        toggleBtn.addEventListener('click', () => {
            const sidebar = document.getElementById('sidebar');
            if (window.innerWidth >= 1024) {
                // Desktop toggle
                if (sidebar.classList.contains('lg:-translate-x-full')) {
                    sidebar.classList.remove('lg:-translate-x-full');
                    sidebar.classList.add('lg:translate-x-0');
                } else {
                    sidebar.classList.add('lg:-translate-x-full');
                    sidebar.classList.remove('lg:translate-x-0');
                }
            } else {
                // Mobile toggle
                if (sidebar.classList.contains('-translate-x-full')) {
                    openSidebar();
                } else {
                    closeSidebar();
                }
            }
        });

        closeBtn.addEventListener('click', closeSidebar);
        overlay.addEventListener('click', closeSidebar);
    }

    return { buildTOC, initToggle };
})();
