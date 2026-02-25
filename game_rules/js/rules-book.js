/**
 * rules-book.js — Alpine.js component for the Kipukas Rules Book.
 *
 * Replaces: app.js, sidebar.js, search.js, chat.js, markdown.js
 * Pattern 11: Self-contained Alpine component with Tailwind utilities.
 *
 * Markdown rendering is handled at build time (build.ts pre-renders into HTML).
 * This component handles: TOC/sidebar, scroll-spy, search, chat (Kippa), print.
 */

document.addEventListener('alpine:init', () => {
  Alpine.data('rulesBook', () => ({
    // ── Sidebar / TOC ─────────────────────────────────────────
    sidebarOpen: globalThis.innerWidth >= 1024,
    headers: [],
    activeId: '',
    _observer: null,

    // ── Search ────────────────────────────────────────────────
    searchQuery: '',
    searchResults: [],
    searchIndex: [],
    showSearchResults: false,
    _searchDebounce: null,

    // ── Chat (Kippa) ──────────────────────────────────────────
    chatOpen: false,
    messages: [],
    chatInput: '',
    chatConnected: false,
    chatStreaming: false,
    _ws: null,
    _subscriptionId: 0,
    _wsReconnectAttempts: 0,
    _maxReconnectAttempts: 5,

    // ── Print ─────────────────────────────────────────────────
    pdfAvailable: false,

    // ══════════════════════════════════════════════════════════
    // Init
    // ══════════════════════════════════════════════════════════

    init() {
      // Collect headers from pre-rendered content
      this.collectHeaders();

      // Set up scroll-spy via IntersectionObserver
      this.initScrollSpy();

      // Load search index (generated at build time)
      this.loadSearchIndex();

      // Probe for pre-built PDF
      fetch('kipukas_rules.pdf', { method: 'HEAD' })
        .then((r) => {
          this.pdfAvailable = r.ok;
        })
        .catch(() => {
          this.pdfAvailable = false;
        });

      // Handle initial hash
      if (globalThis.location.hash) {
        const id = globalThis.location.hash.slice(1);
        setTimeout(() => this.scrollTo(id), 150);
      }

      // Prevent outer page scroll — only #content-scroll should scroll
      globalThis.addEventListener(
        'scroll',
        () => {
          if (globalThis.scrollY !== 0) globalThis.scrollTo(0, 0);
        },
        { passive: false },
      );

      // Watch chat messages for auto-scroll
      this.$watch('messages', () => {
        this.$nextTick(() => {
          const el = this.$refs.chatMessages;
          if (el) el.scrollTop = el.scrollHeight;
        });
      });
    },

    // ══════════════════════════════════════════════════════════
    // Sidebar / TOC
    // ══════════════════════════════════════════════════════════

    collectHeaders() {
      const content = document.getElementById('rules-content');
      if (!content) return;
      const els = content.querySelectorAll('h2, h3');
      this.headers = [];
      els.forEach((el) => {
        if (el.id && el.textContent.replace('#', '').trim()) {
          this.headers.push({
            id: el.id,
            text: el.textContent.replace('#', '').trim(),
            level: el.tagName === 'H2' ? 2 : 3,
          });
        }
      });
    },

    initScrollSpy() {
      if (this._observer) this._observer.disconnect();

      const scrollContainer = document.getElementById('content-scroll');
      if (!scrollContainer) return;

      this._observer = new IntersectionObserver(
        (entries) => {
          entries.forEach((entry) => {
            if (entry.isIntersecting) {
              this.activeId = entry.target.id;
            }
          });
        },
        {
          root: scrollContainer,
          rootMargin: '-10% 0px -80% 0px',
          threshold: 0,
        },
      );

      // Observe all header elements
      const content = document.getElementById('rules-content');
      if (content) {
        content.querySelectorAll('h2, h3').forEach((el) => {
          if (el.id) this._observer.observe(el);
        });
      }
    },

    toggleSidebar() {
      const sidebar = document.getElementById('sidebar');
      if (globalThis.innerWidth >= 1024) {
        // Desktop toggle
        if (sidebar.classList.contains('lg:-translate-x-full')) {
          sidebar.classList.remove('lg:-translate-x-full');
          sidebar.classList.add('lg:translate-x-0');
        } else {
          sidebar.classList.add('lg:-translate-x-full');
          sidebar.classList.remove('lg:translate-x-0');
        }
      } else {
        this.sidebarOpen = !this.sidebarOpen;
      }
    },

    closeSidebar() {
      this.sidebarOpen = false;
    },

    scrollTo(targetId) {
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
      setTimeout(() => {
        target.style.background = '';
      }, 2000);
    },

    tocClick(id) {
      this.scrollTo(id);
      history.replaceState(null, '', `#${id}`);
      // Close sidebar on mobile
      if (globalThis.innerWidth < 1024) this.closeSidebar();
    },

    // ══════════════════════════════════════════════════════════
    // Search
    // ══════════════════════════════════════════════════════════

    loadSearchIndex() {
      fetch('js/search-index.json')
        .then((r) => r.json())
        .then((data) => {
          this.searchIndex = data;
        })
        .catch(() => {
          // Fallback: build index from DOM (dev mode without build)
          this.buildIndexFromDOM();
        });
    },

    buildIndexFromDOM() {
      const content = document.getElementById('rules-content');
      if (!content) return;
      const allHeaders = content.querySelectorAll('h2, h3');
      this.searchIndex = [];
      allHeaders.forEach((header) => {
        const id = header.id || '';
        const title = header.textContent.replace('#', '').trim();
        if (!title) return;
        let text = '';
        let sibling = header.nextElementSibling;
        while (sibling && !sibling.matches('h2, h3')) {
          text += ' ' + sibling.textContent;
          sibling = sibling.nextElementSibling;
        }
        this.searchIndex.push({ id, title, text: text.trim() });
      });
    },

    onSearchInput() {
      clearTimeout(this._searchDebounce);
      this._searchDebounce = setTimeout(() => this.performSearch(), 200);
    },

    performSearch() {
      const q = this.searchQuery.trim();
      if (q.length < 2) {
        this.searchResults = [];
        this.showSearchResults = false;
        return;
      }

      const terms = q.toLowerCase().split(/\s+/).filter((t) => t.length > 1);
      const results = [];

      this.searchIndex.forEach((section) => {
        const haystack = (section.title + ' ' + section.text).toLowerCase();
        const allMatch = terms.every((term) => haystack.includes(term));
        if (!allMatch) return;

        // Build snippet around first match
        const fullText = section.title + ' ' + section.text;
        const lowerFull = fullText.toLowerCase();
        const firstIdx = lowerFull.indexOf(terms[0]);
        const snippetStart = Math.max(0, firstIdx - 40);
        const snippetEnd = Math.min(fullText.length, firstIdx + terms[0].length + 80);
        let snippet = (snippetStart > 0 ? '...' : '') +
          fullText.slice(snippetStart, snippetEnd) +
          (snippetEnd < fullText.length ? '...' : '');

        // Highlight terms
        terms.forEach((term) => {
          const regex = new RegExp(`(${term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')})`, 'gi');
          snippet = snippet.replace(regex, '<mark class="search-highlight">$1</mark>');
        });

        results.push({ id: section.id, title: section.title, snippet });
      });

      this.searchResults = results.slice(0, 15);
      this.showSearchResults = true;
    },

    selectSearchResult(id) {
      this.scrollTo(id);
      history.replaceState(null, '', `#${id}`);
      this.searchQuery = '';
      this.searchResults = [];
      this.showSearchResults = false;
    },

    clearSearch() {
      this.searchQuery = '';
      this.searchResults = [];
      this.showSearchResults = false;
    },

    // ══════════════════════════════════════════════════════════
    // Chat (Kippa)
    // ══════════════════════════════════════════════════════════

    toggleChat() {
      this.chatOpen = !this.chatOpen;
      if (this.chatOpen && !this._ws) {
        this.initWebSocket();
      }
    },

    initWebSocket() {
      if (!navigator.onLine) {
        this.messages.push({
          text:
            'Kippa is unavailable offline. Search the rules or browse the table of contents instead!',
          isUser: false,
          streaming: false,
        });
        return;
      }

      const wsUrl = 'wss://kippa.kipukas.us/graphql';
      if (
        this._ws?.readyState === WebSocket.OPEN || this._ws?.readyState === WebSocket.CONNECTING
      ) {
        return;
      }

      this._ws = new WebSocket(wsUrl, 'graphql-transport-ws');

      this._ws.onopen = () => {
        this._ws.send(JSON.stringify({ type: 'connection_init' }));
      };

      this._ws.onmessage = (event) => {
        const msg = JSON.parse(event.data);
        if (msg.type === 'connection_ack') {
          this.chatConnected = true;
          this._wsReconnectAttempts = 0;
          if (this.messages.length === 0) {
            this.messages.push({
              text: "Hi! I'm Kippa. Ask me anything about the Kipukas rules.",
              isUser: false,
              streaming: false,
            });
          }
        } else if (msg.type === 'connection_error') {
          this.chatConnected = false;
        }
      };

      this._ws.onerror = () => {
        this.chatConnected = false;
      };

      this._ws.onclose = () => {
        this.chatConnected = false;
        if (this._wsReconnectAttempts < this._maxReconnectAttempts) {
          this._wsReconnectAttempts++;
          const delay = Math.min(1000 * Math.pow(2, this._wsReconnectAttempts - 1), 30000);
          setTimeout(() => this.initWebSocket(), delay);
        }
      };
    },

    sendMessage() {
      const prompt = this.chatInput.trim();
      if (!prompt || this.chatStreaming) return;

      this.messages.push({ text: prompt, isUser: true, streaming: false });
      this.chatInput = '';
      this.chatStreaming = true;

      this._streamCompletion(prompt)
        .catch(() => {
          // Fallback to HTTP
          return this._sendHTTP(prompt);
        })
        .catch(() => {
          this.messages.push({
            text: 'Sorry, something went wrong. Please try again.',
            isUser: false,
            streaming: false,
          });
        })
        .finally(() => {
          this.chatStreaming = false;
        });
    },

    _streamCompletion(prompt) {
      return new Promise((resolve, reject) => {
        if (!this.chatConnected || this._ws?.readyState !== WebSocket.OPEN) {
          reject(new Error('WebSocket not connected'));
          return;
        }

        const currentSubId = (++this._subscriptionId).toString();
        let accumulatedText = '';
        let msgIndex = -1;
        let isCompleteReceived = false;
        const originalOnMessage = this._ws.onmessage;

        this._ws.onmessage = (event) => {
          const msg = JSON.parse(event.data);
          if (msg.id === currentSubId) {
            switch (msg.type) {
              case 'next': {
                const chunk = msg.payload?.data?.streamCompletion;
                if (chunk) {
                  accumulatedText += chunk.text;
                  if (msgIndex === -1) {
                    this.messages.push({
                      text: accumulatedText,
                      isUser: false,
                      streaming: true,
                    });
                    msgIndex = this.messages.length - 1;
                  } else {
                    this.messages[msgIndex].text = accumulatedText;
                  }
                  if (chunk.isComplete) {
                    isCompleteReceived = true;
                    if (msgIndex >= 0) this.messages[msgIndex].streaming = false;
                    this._ws.send(JSON.stringify({ id: currentSubId, type: 'complete' }));
                    this._ws.onmessage = originalOnMessage;
                    resolve({ text: accumulatedText });
                  }
                }
                break;
              }
              case 'error':
                this._ws.onmessage = originalOnMessage;
                reject(new Error(msg.payload?.message || 'Subscription error'));
                break;
            }
          } else {
            originalOnMessage(event);
          }
        };

        this._ws.send(JSON.stringify({
          id: currentSubId,
          type: 'subscribe',
          payload: {
            query: `subscription { streamCompletion(request: { prompt: ${
              JSON.stringify(prompt)
            } }) { text isComplete } }`,
          },
        }));

        // Timeout after 2 minutes
        setTimeout(() => {
          if (!isCompleteReceived) {
            this._ws.onmessage = originalOnMessage;
            this._ws.send(JSON.stringify({ id: currentSubId, type: 'complete' }));
            if (msgIndex >= 0) this.messages[msgIndex].streaming = false;
            resolve({ text: accumulatedText });
          }
        }, 120000);
      });
    },

    _sendHTTP(prompt) {
      return fetch('https://kippa.kipukas.us/graphql', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          query: `mutation { createCompletion(request: { prompt: ${
            JSON.stringify(prompt)
          } }) { text tokensGenerated } }`,
        }),
      })
        .then((r) => r.json())
        .then((data) => {
          const result = data.data?.createCompletion;
          if (result?.text) {
            this.messages.push({ text: result.text, isUser: false, streaming: false });
          }
        });
    },

    /**
     * Parse markdown in chat bubble text for display.
     * Uses marked + DOMPurify which are loaded as vendor scripts.
     */
    renderChatMarkdown(text) {
      if (!text) return '';
      if (typeof marked !== 'undefined' && typeof DOMPurify !== 'undefined') {
        const raw = marked.parse(text, { breaks: true });
        return DOMPurify.sanitize(raw);
      }
      // Fallback: escape HTML and convert newlines
      return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/\n/g, '<br>');
    },

    // ══════════════════════════════════════════════════════════
    // Print / PDF
    // ══════════════════════════════════════════════════════════

    handlePrint() {
      if (this.pdfAvailable) {
        const a = document.createElement('a');
        a.href = 'kipukas_rules.pdf';
        a.download = 'kipukas_rules.pdf';
        a.style.display = 'none';
        document.body.appendChild(a);
        a.click();
        setTimeout(() => document.body.removeChild(a), 100);
      } else {
        globalThis.print();
      }
    },

    // ══════════════════════════════════════════════════════════
    // Hash navigation
    // ══════════════════════════════════════════════════════════

    handleHash() {
      const hash = globalThis.location.hash.slice(1);
      if (hash) this.scrollTo(hash);
    },

    handleContentClick(e) {
      const link = e.target.closest('a[href^="#"]');
      if (link) {
        e.preventDefault();
        const targetId = link.getAttribute('href').slice(1);
        this.scrollTo(targetId);
        history.replaceState(null, '', `#${targetId}`);
      }
    },
  }));
});
