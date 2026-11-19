/**
 * qr-camera.js — the camera of the QR scanner.
 *
 * The scanner itself is an engine-owned view (kipukas-view.js) of
 * /api/qr/status: the engine holds whether it is open, draws it, and takes
 * every tap on its controls (kipukas-server/src/routes/qr.rs). What only a
 * page can do is left to this file:
 *
 *   - run the camera for as long as the scanner's video is drawn, and stop it
 *     when it is not (browser getUserMedia API);
 *   - hand frames to the engine's worker, which finds and reads the codes in
 *     them and posts what a code says to the engine (kipukas-worker.js);
 *   - change which camera is in use, a matter of the device and not of the
 *     scanner (the button the engine marks data-qr-switch);
 *   - draw the worker's findings over the picture (the YOLO bounding boxes).
 *
 * It is loaded when the scanner's toolbar button is first pressed.
 */

(() => {
  'use strict';

  // The camera is running, and frames are being handed to the worker.
  let scanning = false;
  // The camera has been asked for and has not answered yet.
  let starting = false;
  // The camera could not be started for the scanner now drawn. It is not
  // asked for again until the scanner has gone and come back.
  let failed = false;
  let scanInterval = null;
  let stream = null;
  let videoDevices = [];
  let currentDeviceIndex = 0;
  let currentFacingMode = 'user';

  // Cleared a moment after the worker last reported what it found.
  let bboxFadeTimer = null;

  /** The scanner's video, while the engine has it drawn. */
  function scannerVideo() {
    return document.querySelector('[data-qr-camera] video');
  }

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

    if (!data || !data.boxes || data.boxes.length === 0) return;

    const { boxes, yoloMs, frameW, frameH, decoded } = data;

    // Scale from frame coordinates to overlay canvas coordinates
    const scaleX = overlay.width / frameW;
    const scaleY = overlay.height / frameH;

    // Five numbers a box: left, top, width, height, and how sure the model
    // is of it (kipukas-server/src/scan.rs).
    for (let at = 0; at + 4 < boxes.length; at += 5) {
      const x = boxes[at] * scaleX;
      const y = boxes[at + 1] * scaleY;
      const w = boxes[at + 2] * scaleX;
      const h = boxes[at + 3] * scaleY;
      const conf = (boxes[at + 4] * 100).toFixed(0);

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

  /** Show the camera's picture in the scanner's video and start reading frames. */
  function show(video, mediaStream) {
    const canvas = video.closest('[data-qr-camera]').querySelector('#canvas');
    const ctx = canvas.getContext('2d', { willReadFrequently: true, alpha: false });
    video.srcObject = mediaStream;
    video.setAttribute('playsinline', 'true');
    // A picture that is replaced before it starts (the camera was switched)
    // gives up its turn to the next: nothing to report.
    const playing = video.play();
    if (playing) playing.catch(() => {});

    clearInterval(scanInterval);
    scanInterval = setInterval(() => {
      if (!scanning || video.readyState !== video.HAVE_ENOUGH_DATA) return;

      ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
      const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);

      // Send raw RGBA pixels to the worker for YOLO+ZXing decode.
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
  }

  /** Start the camera and begin reading frames from it. */
  async function start() {
    if (scanning || starting) return;
    starting = true;

    try {
      await enumerateDevices();

      // Build constraints - use deviceId only if we have a valid non-empty ID
      const deviceId = videoDevices[currentDeviceIndex]?.deviceId;
      const hasValidDeviceId = deviceId && deviceId.trim() !== '';
      const video = hasValidDeviceId
        ? { deviceId: { ideal: deviceId }, focusMode: 'continuous' }
        : { facingMode: currentFacingMode, focusMode: 'continuous' };

      const mediaStream = await navigator.mediaDevices.getUserMedia({ video, audio: false });

      // The scanner may have been closed while the camera was being asked for.
      const target = scannerVideo();
      if (!target) {
        mediaStream.getTracks().forEach((track) => track.stop());
        return;
      }
      stream = mediaStream;
      scanning = true;
      show(target, mediaStream);

      // Re-enumerate devices now that we have permission to get real device IDs
      enumerateDevices();
      console.log('[qr-camera] Camera started, scanning at 2 fps');
    } catch (err) {
      console.error('[qr-camera] Camera access error:', err);
      failed = true;
      // The engine says so in the scanner's place.
      globalThis.kipukasViews.send(
        '/api/qr/status',
        'action=error&msg=' + encodeURIComponent(err.message || 'Camera access denied'),
      );
    } finally {
      starting = false;
      // What the engine has drawn may have changed while the camera started.
      follow();
    }
  }

  /** Stop the camera and stop reading frames. */
  function stop() {
    scanning = false;

    if (scanInterval) {
      clearInterval(scanInterval);
      scanInterval = null;
    }

    if (stream) {
      stream.getTracks().forEach((track) => track.stop());
      stream = null;
    }

    clearBboxOverlay();
    console.log('[qr-camera] Camera stopped');
  }

  /**
   * Switch to the next available camera.
   * Cycles through enumerated video devices.
   */
  function switchCamera() {
    if (starting) return;
    if (videoDevices.length <= 1) {
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

    // Stop the current stream and start again with the new camera
    stop();
    start();
  }

  /** Clear the bounding box overlay canvas. */
  function clearBboxOverlay() {
    const overlay = document.getElementById('bbox-overlay');
    if (overlay) {
      const ctx = overlay.getContext('2d');
      ctx.clearRect(0, 0, overlay.width, overlay.height);
    }
    if (bboxFadeTimer) {
      clearTimeout(bboxFadeTimer);
      bboxFadeTimer = null;
    }
  }

  /**
   * Have the camera follow what the engine has drawn: running while the
   * scanner's video is on the page, stopped when it is not.
   */
  function follow() {
    const video = scannerVideo();
    if (!video) {
      failed = false;
      if (scanning) stop();
    } else if (failed || starting) {
      // Nothing to do until the camera answers, or the scanner goes.
    } else if (!scanning) {
      start();
    } else if (video.srcObject !== stream) {
      // The video was drawn anew: give it the picture again.
      show(video, stream);
    }
  }

  // The engine draws the scanner into its view; watch that for changes.
  new MutationObserver(follow).observe(
    document.getElementById('qr-container') || document.body,
    { childList: true, subtree: true },
  );
  follow();

  // The button that changes camera.
  document.addEventListener('click', (event) => {
    if (event.target.closest && event.target.closest('[data-qr-switch]')) switchCamera();
  });

  // What the worker found in the last frame, drawn over the picture.
  function setupWorkerListener() {
    const worker = globalThis.kipukasWorker;
    if (!worker) {
      // Worker not yet available (kipukas-api.js may not have loaded yet).
      // Retry after a short delay.
      setTimeout(setupWorkerListener, 100);
      return;
    }

    worker.addEventListener('message', (event) => {
      if (event.data?.type !== 'QR_BBOX' || !scanning) return;
      drawBboxOverlay(event.data);

      // Auto-fade boxes after 1.5s if no new data arrives
      if (bboxFadeTimer) clearTimeout(bboxFadeTimer);
      bboxFadeTimer = setTimeout(() => {
        drawBboxOverlay(null);
      }, 1500);
    });
  }

  setupWorkerListener();
})();
