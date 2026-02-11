/**
 * chat.js — Kippa rules assistant chat widget.
 * Reuses WebSocket/GraphQL streaming from the original chat.html.
 * Exposes: KipukasChat
 */

const KipukasChat = (() => {
    const GRAPHQL_HTTP_URL = 'https://kippa.kipukas.us/graphql';
    const GRAPHQL_WS_URL = 'wss://kippa.kipukas.us/graphql';

    let ws = null;
    let isWsConnected = false;
    let isStreaming = false;
    let subscriptionId = 0;
    let wsReconnectAttempts = 0;
    const MAX_RECONNECT_ATTEMPTS = 5;
    let chatOpen = false;

    // DOM refs
    let toggleBtn, panel, iconOpen, iconClose, messages, form, input, sendBtn, statusDot;

    function parseMarkdown(text) {
        if (!text) return '';
        const rawHtml = marked.parse(text, { breaks: true });
        return DOMPurify.sanitize(rawHtml);
    }

    function addMessage(text, isUser = false, streaming = false) {
        const wrapper = document.createElement('div');
        wrapper.className = `flex ${isUser ? 'justify-end' : 'justify-start'}`;

        const bubble = document.createElement('div');
        bubble.className = `chat-bubble rounded-xl px-3 py-2 ${
            isUser
                ? 'bg-sky-600 text-white rounded-br-sm'
                : 'bg-sky-800 text-sky-100 rounded-bl-sm'
        }${streaming ? ' streaming-cursor' : ''}`;

        const contentDiv = document.createElement('div');
        contentDiv.className = 'markdown-content';

        if (isUser) {
            contentDiv.textContent = text;
        } else {
            contentDiv.innerHTML = parseMarkdown(text);
        }

        bubble.appendChild(contentDiv);
        wrapper.appendChild(bubble);
        messages.appendChild(wrapper);
        messages.scrollTop = messages.scrollHeight;

        return contentDiv;
    }

    function updateStreamingMessage(contentDiv, text) {
        contentDiv.innerHTML = parseMarkdown(text);
        messages.scrollTop = messages.scrollHeight;
    }

    function setInputEnabled(enabled) {
        input.disabled = !enabled;
        sendBtn.disabled = !enabled;
        if (enabled) {
            input.placeholder = 'Ask Kippa...';
        } else {
            input.placeholder = 'Connecting...';
        }
    }

    // WebSocket connection
    function initWebSocket() {
        if (ws?.readyState === WebSocket.OPEN || ws?.readyState === WebSocket.CONNECTING) return;

        ws = new WebSocket(GRAPHQL_WS_URL, 'graphql-transport-ws');

        ws.onopen = () => {
            ws.send(JSON.stringify({ type: 'connection_init' }));
        };

        ws.onmessage = (event) => {
            const msg = JSON.parse(event.data);
            if (msg.type === 'connection_ack') {
                isWsConnected = true;
                wsReconnectAttempts = 0;
                statusDot.className = 'w-2 h-2 rounded-full bg-green-500';
                statusDot.title = 'Connected';
                setInputEnabled(true);
                // Welcome message
                if (messages.children.length === 0) {
                    addMessage("Hi! I'm Kippa. Ask me anything about the Kipukas rules.");
                }
            } else if (msg.type === 'connection_error') {
                isWsConnected = false;
                statusDot.className = 'w-2 h-2 rounded-full bg-red-500';
                statusDot.title = 'Disconnected';
            }
        };

        ws.onerror = () => {
            isWsConnected = false;
            statusDot.className = 'w-2 h-2 rounded-full bg-red-500';
            statusDot.title = 'Error';
        };

        ws.onclose = () => {
            isWsConnected = false;
            statusDot.className = 'w-2 h-2 rounded-full bg-gray-500';
            statusDot.title = 'Disconnected';
            setInputEnabled(false);

            if (wsReconnectAttempts < MAX_RECONNECT_ATTEMPTS) {
                wsReconnectAttempts++;
                const delay = Math.min(1000 * Math.pow(2, wsReconnectAttempts - 1), 30000);
                setTimeout(initWebSocket, delay);
            }
        };
    }

    function streamCompletion(prompt) {
        return new Promise((resolve, reject) => {
            if (!isWsConnected || ws?.readyState !== WebSocket.OPEN) {
                reject(new Error('WebSocket not connected'));
                return;
            }

            const currentSubId = (++subscriptionId).toString();
            let accumulatedText = '';
            let contentDiv = null;
            let bubble = null;
            let isCompleteReceived = false;
            const originalOnMessage = ws.onmessage;

            ws.onmessage = (event) => {
                const msg = JSON.parse(event.data);

                if (msg.id === currentSubId) {
                    switch (msg.type) {
                        case 'next':
                            const chunk = msg.payload?.data?.streamCompletion;
                            if (chunk) {
                                accumulatedText += chunk.text;
                                if (!contentDiv) {
                                    contentDiv = addMessage(accumulatedText, false, true);
                                    bubble = contentDiv.parentElement;
                                } else {
                                    updateStreamingMessage(contentDiv, accumulatedText);
                                }
                                if (chunk.isComplete) {
                                    isCompleteReceived = true;
                                    if (bubble) bubble.classList.remove('streaming-cursor');
                                    ws.send(JSON.stringify({ id: currentSubId, type: 'complete' }));
                                    ws.onmessage = originalOnMessage;
                                    resolve({ text: accumulatedText });
                                }
                            }
                            break;
                        case 'error':
                            ws.onmessage = originalOnMessage;
                            reject(new Error(msg.payload?.message || 'Subscription error'));
                            break;
                    }
                } else {
                    originalOnMessage(event);
                }
            };

            ws.send(JSON.stringify({
                id: currentSubId,
                type: 'subscribe',
                payload: {
                    query: `subscription { streamCompletion(request: { prompt: ${JSON.stringify(prompt)} }) { text isComplete } }`
                }
            }));

            // Timeout
            setTimeout(() => {
                if (!isCompleteReceived) {
                    ws.onmessage = originalOnMessage;
                    ws.send(JSON.stringify({ id: currentSubId, type: 'complete' }));
                    if (bubble) bubble.classList.remove('streaming-cursor');
                    resolve({ text: accumulatedText });
                }
            }, 120000);
        });
    }

    async function sendHTTP(prompt) {
        const response = await fetch(GRAPHQL_HTTP_URL, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                query: `mutation { createCompletion(request: { prompt: ${JSON.stringify(prompt)} }) { text tokensGenerated } }`
            })
        });
        const data = await response.json();
        return data.data?.createCompletion;
    }

    async function handleSubmit(e) {
        e.preventDefault();
        const prompt = input.value.trim();
        if (!prompt || isStreaming) return;

        addMessage(prompt, true);
        input.value = '';
        input.focus();

        isStreaming = true;
        sendBtn.disabled = true;
        sendBtn.textContent = '...';

        try {
            try {
                await streamCompletion(prompt);
            } catch {
                const result = await sendHTTP(prompt);
                if (result?.text) addMessage(result.text, false);
            }
        } catch {
            addMessage('Sorry, something went wrong. Please try again.', false);
        } finally {
            isStreaming = false;
            sendBtn.disabled = false;
            sendBtn.textContent = 'Send';
        }
    }

    function toggleChat() {
        chatOpen = !chatOpen;
        panel.classList.toggle('hidden', !chatOpen);
        // Use flex when open, clear inline style when closed
        if (chatOpen) {
            panel.style.display = 'flex';
        } else {
            panel.style.display = '';
        }
        iconOpen.classList.toggle('hidden', chatOpen);
        iconClose.classList.toggle('hidden', !chatOpen);

        // Init WebSocket on first open
        if (chatOpen && !ws) {
            initWebSocket();
        }
    }

    function init() {
        toggleBtn = document.getElementById('chat-toggle');
        panel = document.getElementById('chat-panel');
        iconOpen = document.getElementById('chat-icon-open');
        iconClose = document.getElementById('chat-icon-close');
        messages = document.getElementById('chat-messages');
        form = document.getElementById('chat-form');
        input = document.getElementById('chat-input');
        sendBtn = document.getElementById('chat-send');
        statusDot = document.getElementById('chat-status');

        toggleBtn.addEventListener('click', toggleChat);
        form.addEventListener('submit', handleSubmit);

        // Cleanup on page unload
        window.addEventListener('beforeunload', () => {
            if (ws) ws.close();
        });
    }

    return { init };
})();
