/**
 * PWA Update Handler
 *
 * Registers the service worker and manages the update-then-refresh flow:
 *
 *   1. Browser detects a new sw.js → installs it in the background.
 *   2. New SW enters the "waiting" state (it does NOT skipWaiting automatically).
 *   3. This script shows a toast notification: "New version available!".
 *   4. User clicks "Update Now" → we postMessage SKIP_WAITING to the waiting SW.
 *   5. The waiting SW calls self.skipWaiting() → becomes active.
 *   6. The "controllerchange" event fires → we reload the page once.
 *
 * Include this script in your HTML pages. Do NOT add a second inline
 * navigator.serviceWorker.register() — this file handles it.
 */

(function () {
  'use strict';

  // ============================================
  // CONFIGURATION (override via window.PWAUpdateConfig before this script loads)
  // ============================================
  const defaults = {
    debug: false,
    autoReload: false, // true = skip the toast and reload immediately
    notificationDuration: 0, // ms before auto-dismiss (0 = persistent)
    updateInterval: 30 * 60 * 1000, // how often to poll for SW updates (30 min)
  };

  const config = Object.assign({}, defaults, globalThis.PWAUpdateConfig || {});

  function log(...args) {
    if (config.debug) console.log('[PWA Update]', ...args);
  }

  // ============================================
  // UPDATE NOTIFICATION UI
  // ============================================

  function createUpdateNotification(onUpdate) {
    if (document.getElementById('pwa-update-notification')) return;

    const notification = document.createElement('div');
    notification.id = 'pwa-update-notification';
    notification.innerHTML = `
      <div class="pwa-update-content">
        <span class="pwa-update-message">🎉 New version available!</span>
        <button id="pwa-update-reload" class="pwa-update-button">Update Now</button>
        <button id="pwa-update-dismiss" class="pwa-update-button pwa-update-button-secondary">Later</button>
      </div>
    `;

    const styles = document.createElement('style');
    styles.textContent = `
      #pwa-update-notification {
        position: fixed;
        bottom: 20px;
        right: 20px;
        background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
        color: white;
        padding: 16px 24px;
        border-radius: 12px;
        box-shadow: 0 10px 40px rgba(0,0,0,0.3);
        z-index: 10000;
        font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
        animation: pwaSlideIn 0.3s ease-out;
        max-width: 400px;
      }
      @keyframes pwaSlideIn {
        from { transform: translateX(100%); opacity: 0; }
        to   { transform: translateX(0);    opacity: 1; }
      }
      @keyframes pwaSlideOut {
        from { transform: translateX(0);    opacity: 1; }
        to   { transform: translateX(100%); opacity: 0; }
      }
      .pwa-update-content {
        display: flex;
        flex-direction: column;
        gap: 12px;
      }
      .pwa-update-message {
        font-size: 16px;
        font-weight: 600;
      }
      .pwa-update-button {
        background: white;
        color: #667eea;
        border: none;
        padding: 10px 20px;
        border-radius: 8px;
        font-size: 14px;
        font-weight: 600;
        cursor: pointer;
        transition: all 0.2s;
      }
      .pwa-update-button:hover {
        transform: translateY(-2px);
        box-shadow: 0 4px 12px rgba(0,0,0,0.2);
      }
      .pwa-update-button-secondary {
        background: rgba(255,255,255,0.2);
        color: white;
      }
      .pwa-update-button-secondary:hover {
        background: rgba(255,255,255,0.3);
      }
      @media (max-width: 480px) {
        #pwa-update-notification {
          left: 10px;
          right: 10px;
          bottom: 10px;
          max-width: none;
        }
      }
    `;

    document.head.appendChild(styles);
    document.body.appendChild(notification);

    function dismiss() {
      notification.style.animation = 'pwaSlideOut 0.3s ease-in forwards';
      setTimeout(() => {
        notification.remove();
        styles.remove();
      }, 300);
    }

    document.getElementById('pwa-update-reload').addEventListener('click', () => {
      onUpdate();
    });

    document.getElementById('pwa-update-dismiss').addEventListener('click', dismiss);

    if (config.notificationDuration > 0) {
      setTimeout(() => {
        if (notification.parentNode) dismiss();
      }, config.notificationDuration);
    }
  }

  // ============================================
  // SERVICE WORKER LIFECYCLE
  // ============================================

  if (!('serviceWorker' in navigator)) return;

  // Guard: only one reload per controllerchange
  let refreshing = false;

  /**
   * When a new SW finishes installing and enters "waiting", decide what to do.
   */
  function onNewSWWaiting(waitingSW) {
    log('New service worker is waiting to activate');

    if (config.autoReload) {
      // Skip the toast — tell the SW to activate immediately
      waitingSW.postMessage({ type: 'SKIP_WAITING' });
      return;
    }

    // Show the update toast. When the user clicks "Update Now" we tell
    // the waiting SW to skipWaiting, which triggers controllerchange → reload.
    createUpdateNotification(() => {
      log('User accepted update — sending SKIP_WAITING');
      waitingSW.postMessage({ type: 'SKIP_WAITING' });
    });
  }

  /**
   * Watch a registration for a newly installing SW and track its state.
   */
  function listenForWaitingSW(registration) {
    // If there is already a SW waiting (e.g. user dismissed the toast earlier)
    if (registration.waiting) {
      onNewSWWaiting(registration.waiting);
      return;
    }

    registration.addEventListener('updatefound', () => {
      const newSW = registration.installing;
      if (!newSW) return;

      log('New service worker installing…');

      newSW.addEventListener('statechange', () => {
        if (newSW.state === 'installed' && navigator.serviceWorker.controller) {
          // A new SW is installed while an old one still controls the page.
          onNewSWWaiting(newSW);
        } else if (newSW.state === 'installed') {
          log('Content cached for first-time offline use');
        }
      });
    });
  }

  // ============================================
  // REGISTRATION
  // ============================================

  self.addEventListener('load', () => {
    navigator.serviceWorker
      .register('/sw.js')
      .then((registration) => {
        log('SW registered, scope:', registration.scope);

        // Immediately check if there's already a waiting worker
        // (can happen if the user navigated away before accepting the update)
        listenForWaitingSW(registration);

        // Periodically poll for a new SW
        setInterval(() => {
          registration.update();
          log('Checking for SW update…');
        }, config.updateInterval);
      })
      .catch((err) => {
        console.error('SW registration failed:', err);
      });
  });

  // ============================================
  // CONTROLLER CHANGE → RELOAD
  // ============================================
  // Fires when a new SW takes over (after skipWaiting succeeds).
  navigator.serviceWorker.addEventListener('controllerchange', () => {
    if (refreshing) return;
    refreshing = true;
    log('New service worker activated — reloading page');
    globalThis.location.reload();
  });
})();
