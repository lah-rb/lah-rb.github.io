/**
 * qr-camera.js — Minimal camera + QR scan loop for the HTMX-driven QR scanner.
 *
 * Replaces the old Alpine-driven qr_scanner.js. All UI state management is
 * handled by HTMX + WASM routes (/api/qr/status, /api/qr/found). This module
 * only manages:
 *   - Camera start/stop (browser getUserMedia API)
 *   - Frame capture loop (canvas → pixel data → Web Worker)
 *   - Listening for QR_FOUND results from the worker
 *   - Drawing YOLO bounding box overlays for visual debugging
 *
 * The worker (kipukas-worker.js) handles YOLO+ZXing decode + WASM HTML formatting.
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

  // Bbox overlay state (updated by QR_BBOX messages from worker)
  let lastBboxData = null;
  let bboxFadeTimer = null;

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
   * Draw YOLO bounding boxes on the overlay canvas.
   * Green = detected, Red outline = detected but decode failed, Bright green = decoded.
   *
   * @param {Object} data - QR_BBOX message data from worker
   */
  function drawBboxOverlay(data) {
    const overlay = document.getElementById('bbox-overlay');
    if (!overlay) return;

    const ctx = overlay.getContext('2d');
    ctx.clearRect(0, 0, overlay.width, overlay.height);

    if (!data || !data.detections || data.detections.length === 0) return;

    const { detections, yoloMs, frameW, frameH, decoded } = data;

    // Scale from frame coordinates to overlay canvas coordinates
    const scaleX = overlay.width / frameW;
    const scaleY = overlay.height / frameH;

    for (const det of detections) {
      const x = det.x * scaleX;
      const y = det.y * scaleY;
      const w = det.w * scaleX;
      const h = det.h * scaleY;
      const conf = (det.confidence * 100).toFixed(0);

      // Box color: bright green if decoded, amber if detection only
      ctx.strokeStyle = decoded ? '#22c55e' : '#f59e0b';
      ctx.lineWidth = 3;
      ctx.strokeRect(x, y, w, h);

      // Corner accents for visual clarity
      const cornerLen = Math.min(w, h) * 0.2;
      ctx.lineWidth = 4;
      ctx.strokeStyle = decoded ? '#22c55e' : '#f59e0b';
      // Top-left
      ctx.beginPath();
      ctx.moveTo(x, y + cornerLen);
      ctx.lineTo(x, y);
      ctx.lineTo(x + cornerLen, y);
      ctx.stroke();
      // Top-right
      ctx.beginPath();
      ctx.moveTo(x + w - cornerLen, y);
      ctx.lineTo(x + w, y);
      ctx.lineTo(x + w, y + cornerLen);
      ctx.stroke();
      // Bottom-right
      ctx.beginPath();
      ctx.moveTo(x + w, y + h - cornerLen);
      ctx.lineTo(x + w, y + h);
      ctx.lineTo(x + w - cornerLen, y + h);
      ctx.stroke();
      // Bottom-left
      ctx.beginPath();
      ctx.moveTo(x + cornerLen, y + h);
      ctx.lineTo(x, y + h);
      ctx.lineTo(x, y + h - cornerLen);
      ctx.stroke();

      // Label background
      const label = decoded ? `✓ ${conf}%` : `${conf}%`;
      ctx.font = 'bold 14px monospace';
      const textW = ctx.measureText(label).width;
      ctx.fillStyle = decoded ? 'rgba(34, 197, 94, 0.85)' : 'rgba(245, 158, 11, 0.85)';
      ctx.fillRect(x, y - 20, textW + 8, 20);

      // Label text
      ctx.fillStyle = '#000';
      ctx.fillText(label, x + 4, y - 5);
    }

    // YOLO timing in bottom-left
    ctx.font = '12px monospace';
    ctx.fillStyle = 'rgba(0, 0, 0, 0.6)';
    ctx.fillRect(4, overlay.height - 22, 90, 18);
    ctx.fillStyle = '#22c55e';
    ctx.fillText(`YOLO: ${yoloMs}ms`, 8, overlay.height - 8);
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
      // Build constraints - use deviceId only if we have a valid non-empty ID
      const deviceId = videoDevices[currentDeviceIndex]?.deviceId;
      const hasValidDeviceId = deviceId && deviceId.trim() !== '';

      const constraints = {
        video: {
          facingMode: hasValidDeviceId ? undefined : currentFacingMode,
          deviceId: hasValidDeviceId ? { ideal: deviceId } : undefined,
          focusMode: 'continuous',
        },
        audio: false,
      };

      // Clean up undefined values to avoid constraint issues
      if (!constraints.video.facingMode) delete constraints.video.facingMode;
      if (!constraints.video.deviceId) delete constraints.video.deviceId;

      navigator.mediaDevices
        .getUserMedia(constraints)
        .then((mediaStream) => {
          stream = mediaStream;
          video.srcObject = stream;
          video.setAttribute('playsinline', 'true');
          video.play();
          scanning = true;
          scannerOpen = true;

          // Re-enumerate devices now that we have permission to get real device IDs
          enumerateDevices().then(() => {
            console.log('[qr-camera] Re-enumerated devices after permission grant');
          });

          // Start periodic frame capture → worker decode loop
          scanInterval = setInterval(() => {
            if (!scanning || video.readyState !== video.HAVE_ENOUGH_DATA) return;

            ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
            const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);

            // Send raw RGBA pixels to Web Worker for YOLO+ZXing decode.
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
      console.log(
        `[qr-camera] Switching to device ${currentDeviceIndex}: ${
          videoDevices[currentDeviceIndex].label || 'Unknown'
        }`,
      );
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

    // Clear bbox overlay
    clearBboxOverlay();

    // Restart camera with new device
    start();
  }

  /**
   * Clear the bounding box overlay canvas.
   */
  function clearBboxOverlay() {
    const overlay = document.getElementById('bbox-overlay');
    if (overlay) {
      const ctx = overlay.getContext('2d');
      ctx.clearRect(0, 0, overlay.width, overlay.height);
    }
    lastBboxData = null;
    if (bboxFadeTimer) {
      clearTimeout(bboxFadeTimer);
      bboxFadeTimer = null;
    }
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

    clearBboxOverlay();

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

  // ── Listen for QR_FOUND and QR_BBOX from the Web Worker ───────────

  function setupWorkerListener() {
    const worker = globalThis.kipukasWorker;
    if (!worker) {
      // Worker not yet available (kipukas-api.js may not have loaded yet).
      // Retry after a short delay.
      setTimeout(setupWorkerListener, 100);
      return;
    }

    worker.addEventListener('message', (event) => {
      // ── Bounding box overlay ──
      if (event.data?.type === 'QR_BBOX') {
        if (!scanning) return; // Don't draw if scanner closed

        lastBboxData = event.data;
        drawBboxOverlay(event.data);

        // Auto-fade boxes after 1.5s if no new data arrives
        if (bboxFadeTimer) clearTimeout(bboxFadeTimer);
        bboxFadeTimer = setTimeout(() => {
          drawBboxOverlay(null);
        }, 1500);
        return;
      }

      // ── QR found result ──
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
