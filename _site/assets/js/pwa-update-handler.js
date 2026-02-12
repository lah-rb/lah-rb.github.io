/**
 * PWA Update Handler
 * 
 * This script handles service worker update notifications.
 * Include this in your HTML pages to notify users when new content is available.
 */

(function() {
  'use strict';

  // Configuration
  const config = {
    // Show debug messages in console
    debug: false,
    // Auto-reload when update is available (set to false to show manual reload button)
    autoReload: false,
    // Notification display duration in ms (0 = persistent)
    notificationDuration: 0
  };

  function log(...args) {
    if (config.debug) {
      console.log('[PWA Update]', ...args);
    }
  }

  // ============================================
  // UPDATE NOTIFICATION UI
  // ============================================

  function createUpdateNotification(reloadCallback) {
    // Check if notification already exists
    if (document.getElementById('pwa-update-notification')) {
      return;
    }

    // Create notification container
    const notification = document.createElement('div');
    notification.id = 'pwa-update-notification';
    notification.innerHTML = `
      <div class="pwa-update-content">
        <span class="pwa-update-message">🎉 New version available!</span>
        <button id="pwa-update-reload" class="pwa-update-button">Update Now</button>
        <button id="pwa-update-dismiss" class="pwa-update-button pwa-update-button-secondary">Later</button>
      </div>
    `;

    // Add styles
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
        animation: slideIn 0.3s ease-out;
        max-width: 400px;
      }
      
      @keyframes slideIn {
        from { transform: translateX(100%); opacity: 0; }
        to { transform: translateX(0); opacity: 1; }
      }
      
      @keyframes slideOut {
        from { transform: translateX(0); opacity: 1; }
        to { transform: translateX(100%); opacity: 0; }
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

    // Handle reload button
    document.getElementById('pwa-update-reload').addEventListener('click', () => {
      if (reloadCallback) {
        reloadCallback();
      }
    });

    // Handle dismiss button
    document.getElementById('pwa-update-dismiss').addEventListener('click', () => {
      notification.style.animation = 'slideOut 0.3s ease-in';
      setTimeout(() => {
        notification.remove();
        styles.remove();
      }, 300);
    });

    // Auto-hide if configured
    if (config.notificationDuration > 0) {
      setTimeout(() => {
        if (notification.parentNode) {
          notification.style.animation = 'slideOut 0.3s ease-in';
          setTimeout(() => {
            notification.remove();
            styles.remove();
          }, 300);
        }
      }, config.notificationDuration);
    }
  }

  // ============================================
  // SERVICE WORKER REGISTRATION & HANDLING
  // ============================================

  let refreshing = false;
  let newWorker = null;

  function handleUpdate(registration) {
    log('Update found, waiting for install...');
    
    registration.addEventListener('updatefound', () => {
      newWorker = registration.installing;
      
      newWorker.addEventListener('statechange', () => {
        if (newWorker.state === 'installed') {
          if (navigator.serviceWorker.controller) {
            // New update available
            log('New content available');
            
            if (config.autoReload) {
              // Auto-reload the page
              window.location.reload();
            } else {
              // Show notification to user
              createUpdateNotification(() => {
                newWorker.postMessage({ type: 'SKIP_WAITING' });
              });
            }
          } else {
            // First install - content cached for offline use
            log('Content cached for offline use');
          }
        }
      });
    });
  }

  // Listen for messages from service worker
  if ('serviceWorker' in navigator) {
    navigator.serviceWorker.addEventListener('message', (event) => {
      log('Message from SW:', event.data);
      
      if (event.data && event.data.type === 'SW_ACTIVATED') {
        // Service worker activated - reload to get new content
        if (!refreshing) {
          refreshing = true;
          window.location.reload();
        }
      }
    });

    // Register service worker
    window.addEventListener('load', () => {
      navigator.serviceWorker.register('/sw.js')
        .then((registration) => {
          log('SW registered:', registration.scope);
          
          // Check for updates immediately
          registration.update();
          
          // Handle updates
          handleUpdate(registration);
          
          // Check for updates periodically (every 30 minutes)
          setInterval(() => {
            registration.update();
          }, 30 * 60 * 1000);
        })
        .catch((error) => {
          console.error('SW registration failed:', error);
        });
    });

    // Handle controller change (new SW activated)
    navigator.serviceWorker.addEventListener('controllerchange', () => {
      if (!refreshing) {
        refreshing = true;
        window.location.reload();
      }
    });
  }

  // Expose config for external modification
  window.PWAUpdateConfig = config;
})();
