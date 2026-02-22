/**
 * qr-camera.js — Minimal camera + QR scan loop for the HTMX-driven QR scanner.
 *
 * Replaces the old Alpine-driven qr_scanner.js. All UI state management is
 * handled by HTMX + WASM routes (/api/qr/status, /api/qr/found). This module
 * only manages:
 *   - Camera start/stop (browser getUserMedia API)
 *   - Frame capture loop (canvas → pixel data → Web Worker)
 *   - Listening for QR_FOUND results from the worker
 *
 * The worker (kipukas-worker.js) handles ZXing decode + WASM HTML formatting.
 * Results are swapped into the DOM via htmx.ajax() for architectural consistency.
 *
 * Exposed globally as window.kipukasQR for use by HTMX-returned HTML fragments.
 */

(() => {
  'use strict';

  let scanning = false;
  let scannerOpen = false;
  let scanInterval = null;
  let stream = null;
  let videoDevices = [];
  let currentDeviceIndex = 0;
  let currentFacingMode = 'user';

  /**
   * Enumerate available video input devices.
   * Called on startup to populate videoDevices list.
   */
  async function enumerateDevices() {
    try {
      const devices = await navigator.mediaDevices.enumerateDevices();
      videoDevices = devices.filter((device) => device.kind === 'videoinput');
      console.log(`[qr-camera] Found ${videoDevices.length} video devices`);
    } catch (err) {
      console.error('[qr-camera] Failed to enumerate devices:', err);
      videoDevices = [];
    }
  }

  /**
   * Start the camera and begin the QR scanning loop.
   * Called by <script> tags in WASM-returned scanning UI HTML.
   */
  function start() {
    if (scanning) return;

    const video = document.getElementById('video');
    const canvas = document.getElementById('canvas');
    if (!video || !canvas) {
      console.error('[qr-camera] Missing #video or #canvas element');
      return;
    }

    const ctx = canvas.getContext('2d', { willReadFrequently: true, alpha: false });

    // Enumerate devices first, then start camera
    enumerateDevices().then(() => {
      const constraints = {
        video: {
          facingMode: currentFacingMode,
          focusMode: 'continuous',
        },
        audio: false,
      };

      // If we have enumerated devices and a valid index, use deviceId
      if (videoDevices.length > 0 && currentDeviceIndex < videoDevices.length) {
        constraints.video = {
          deviceId: { exact: videoDevices[currentDeviceIndex].deviceId },
          focusMode: 'continuous',
        };
      }

      navigator.mediaDevices
        .getUserMedia(constraints)
        .then((mediaStream) => {
          stream = mediaStream;
          video.srcObject = stream;
          video.setAttribute('playsinline', 'true');
          video.play();
          scanning = true;
          scannerOpen = true;

          // Start periodic frame capture → worker decode loop
          scanInterval = setInterval(() => {
            if (!scanning || video.readyState !== video.HAVE_ENOUGH_DATA) return;

            ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
            const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);

            // Send raw RGBA pixels to Web Worker for ZXing decode.
            // Transfer the buffer for zero-copy performance.
            const worker = globalThis.kipukasWorker;
            if (worker) {
              worker.postMessage(
                {
                  type: 'QR_FRAME',
                  pixels: imageData.data.buffer,
                  width: canvas.width,
                  height: canvas.height,
                },
                [imageData.data.buffer], // Transfer ownership (zero-copy)
              );
            }
          }, 500); // 2 fps — sufficient for QR scanning, gentle on resources

          console.log('[qr-camera] Camera started, scanning at 2 fps');
        })
        .catch((err) => {
          console.error('[qr-camera] Camera access error:', err);
          // Show error via HTMX → WASM route
          if (typeof htmx !== 'undefined') {
            htmx.ajax(
              'GET',
              `/api/qr/status?action=error&msg=${
                encodeURIComponent(err.message || 'Camera access denied')
              }`,
              { target: '#qr-container', swap: 'innerHTML' },
            );
          }
          scannerOpen = false;
        });
    });
  }

  /**
   * Switch to the next available camera.
   * Cycles through enumerated video devices.
   */
  function switchCamera() {
    if (videoDevices.length <= 1) {
      console.log('[qr-camera] No alternative cameras available');
      // Toggle between front/back facing mode as fallback
      currentFacingMode = currentFacingMode === 'user' ? 'environment' : 'user';
      console.log(`[qr-camera] Switching to ${currentFacingMode} camera`);
    } else {
      // Move to next device in list
      currentDeviceIndex = (currentDeviceIndex + 1) % videoDevices.length;
      console.log(`[qr-camera] Switching to device ${currentDeviceIndex}: ${videoDevices[currentDeviceIndex].label || 'Unknown'}`);
    }

    // Stop current stream and restart with new constraints
    if (stream) {
      stream.getTracks().forEach((track) => track.stop());
      stream = null;
    }
    scanning = false;
    if (scanInterval) {
      clearInterval(scanInterval);
      scanInterval = null;
    }

    // Restart camera with new device
    start();
  }

  /**
   * Stop the camera and scanning loop.
   * Called by close button onclick and by QR_FOUND handler.
   */
  function stop() {
    scanning = false;
    scannerOpen = false;

    if (scanInterval) {
      clearInterval(scanInterval);
      scanInterval = null;
    }

    if (stream) {
      stream.getTracks().forEach((track) => track.stop());
      stream = null;
    }

    const video = document.getElementById('video');
    if (video) {
      video.srcObject = null;
    }

    console.log('[qr-camera] Camera stopped');
  }

  /**
   * Toggle the QR scanner open/closed.
   * Called by the QR scanner toolbar button.
   */
  function toggle() {
    if (scannerOpen) {
      stop();
      if (typeof htmx !== 'undefined') {
        htmx.ajax('GET', '/api/qr/status?action=close', {
          target: '#qr-container',
          swap: 'innerHTML',
        });
      }
    } else {
      const privacy = localStorage.getItem('qr-privacy-accepted') === 'true';
      if (typeof htmx !== 'undefined') {
        htmx.ajax('GET', `/api/qr/status?action=open&privacy=${privacy}`, {
          target: '#qr-container',
          swap: 'innerHTML',
        });
      }
      // Note: scannerOpen is set to true inside start() after camera is acquired.
      // If privacy modal is shown, start() is called by the WASM-returned HTML
      // after the user accepts.
    }
  }

  // ── Listen for QR_FOUND from the Web Worker ────────────────────────

  function setupWorkerListener() {
    const worker = globalThis.kipukasWorker;
    if (!worker) {
      // Worker not yet available (kipukas-api.js may not have loaded yet).
      // Retry after a short delay.
      setTimeout(setupWorkerListener, 100);
      return;
    }

    worker.addEventListener('message', (event) => {
      if (event.data?.type === 'QR_FOUND') {
        // Stop scanning immediately
        stop();

        // Inject the WASM-generated HTML (contains redirect script)
        const target = document.getElementById('qr-result');
        if (target) {
          target.innerHTML = event.data.html;
          // Process any HTMX attributes or script tags in the new HTML
          if (typeof htmx !== 'undefined') htmx.process(target);
          // Execute inline scripts (htmx.process doesn't run <script> tags)
          target.querySelectorAll('script').forEach((oldScript) => {
            const newScript = document.createElement('script');
            newScript.textContent = oldScript.textContent;
            oldScript.parentNode.replaceChild(newScript, oldScript);
          });
        }
      }
    });
  }

  // Initialize worker listener when DOM is ready
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', setupWorkerListener);
  } else {
    setupWorkerListener();
  }

  // ── Public API ─────────────────────────────────────────────────────

  globalThis.kipukasQR = { start, stop, toggle, switchCamera };
})();
