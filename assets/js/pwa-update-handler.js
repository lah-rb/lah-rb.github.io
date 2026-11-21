(function(){"use strict";const n=Object.assign({},{debug:!1,autoReload:!1,notificationDuration:0,updateInterval:18e5},globalThis.PWAUpdateConfig||{});function a(...e){n.debug&&console.log("[PWA Update]",...e)}function d(e){if(document.getElementById("pwa-update-notification"))return;const t=document.createElement("div");t.id="pwa-update-notification",t.innerHTML=`
      <div class="pwa-update-content">
        <span class="pwa-update-message">\u{1F389} New version available!</span>
        <button id="pwa-update-reload" class="pwa-update-button">Update Now</button>
        <button id="pwa-update-dismiss" class="pwa-update-button pwa-update-button-secondary">Later</button>
      </div>
    `;const o=document.createElement("style");o.textContent=`
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
    `,document.head.appendChild(o),document.body.appendChild(t);function s(){t.style.animation="pwaSlideOut 0.3s ease-in forwards",setTimeout(()=>{t.remove(),o.remove()},300)}document.getElementById("pwa-update-reload").addEventListener("click",()=>{e()}),document.getElementById("pwa-update-dismiss").addEventListener("click",s),n.notificationDuration>0&&setTimeout(()=>{t.parentNode&&s()},n.notificationDuration)}if(!("serviceWorker"in navigator))return;let i=!1;function r(e){if(a("New service worker is waiting to activate"),n.autoReload){e.postMessage({type:"SKIP_WAITING"});return}d(()=>{a("User accepted update \u2014 sending SKIP_WAITING"),e.postMessage({type:"SKIP_WAITING"})})}function p(e){if(e.waiting){r(e.waiting);return}e.addEventListener("updatefound",()=>{const t=e.installing;t&&(a("New service worker installing\u2026"),t.addEventListener("statechange",()=>{t.state==="installed"&&navigator.serviceWorker.controller?r(t):t.state==="installed"&&a("Content cached for first-time offline use")}))})}self.addEventListener("load",()=>{navigator.serviceWorker.register("/sw.js").then(e=>{a("SW registered, scope:",e.scope),p(e),setInterval(()=>{e.update(),a("Checking for SW update\u2026")},n.updateInterval)}).catch(e=>{console.error("SW registration failed:",e)})}),navigator.serviceWorker.addEventListener("controllerchange",()=>{i||(i=!0,a("New service worker activated \u2014 reloading page"),globalThis.location.reload())})})();
