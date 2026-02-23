(function(){"use strict";const n=Object.assign({},{debug:!1,autoReload:!1,notificationDuration:0,updateInterval:18e5},globalThis.PWAUpdateConfig||{});function a(...t){n.debug&&console.log("[PWA Update]",...t)}function s(t){if(document.getElementById("pwa-update-notification"))return;const e=document.createElement("div");e.id="pwa-update-notification",e.innerHTML=`
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
    `,document.head.appendChild(o),document.body.appendChild(e);function d(){e.style.animation="pwaSlideOut 0.3s ease-in forwards",setTimeout(()=>{e.remove(),o.remove()},300)}document.getElementById("pwa-update-reload").addEventListener("click",()=>{t()}),document.getElementById("pwa-update-dismiss").addEventListener("click",d),n.notificationDuration>0&&setTimeout(()=>{e.parentNode&&d()},n.notificationDuration)}if(!("serviceWorker"in navigator))return;let i=!1;function r(t){if(a("New service worker is waiting to activate"),n.autoReload){t.postMessage({type:"SKIP_WAITING"});return}s(()=>{a("User accepted update \u2014 sending SKIP_WAITING"),t.postMessage({type:"SKIP_WAITING"})})}function p(t){if(t.waiting){r(t.waiting);return}t.addEventListener("updatefound",()=>{const e=t.installing;e&&(a("New service worker installing\u2026"),e.addEventListener("statechange",()=>{e.state==="installed"&&navigator.serviceWorker.controller?r(e):e.state==="installed"&&a("Content cached for first-time offline use")}))})}self.addEventListener("load",()=>{navigator.serviceWorker.register("/sw.js").then(t=>{a("SW registered, scope:",t.scope),p(t),setInterval(()=>{t.update(),a("Checking for SW update\u2026")},n.updateInterval)}).catch(t=>{console.error("SW registration failed:",t)})}),navigator.serviceWorker.addEventListener("controllerchange",()=>{i||(i=!0,a("New service worker activated \u2014 reloading page"),globalThis.location.reload())})})();
