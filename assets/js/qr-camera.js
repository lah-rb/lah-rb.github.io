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
 *     them and posts what a code says to the engine (kipukas-worker.js). How
 *     often is the engine's to say (data-qr-every on the scanner); the next
 *     frame is taken only when the worker is done with the last;
 *   - change which camera is in use, of those the device has (the button the
 *     engine marks data-qr-switch), and tell the engine which is running and
 *     the size of its picture. The engine keeps that: it writes the camera on
 *     the scanner for this file to ask the browser for the next time, and
 *     shows the picture mirrored or not by which way the camera faces;
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
  let stream = null;
  // The canvas frames are read through, and its context.
  let frameCanvas = null;
  let frameCtx = null;
  // The wait until the next frame is taken, and when the last one was.
  let frameTimer = null;
  let frameTakenAt = 0;
  // How often a frame is taken if the engine has not said (milliseconds).
  const EVERY_UNSAID = 500;
  // A worker that has not said it is done with a frame after this long is
  // handed another (milliseconds). It lets go any that arrive while it is
  // busy, so this only matters if a frame's answer was lost.
  const UNANSWERED = 2000;
  let videoDevices = [];
  // The camera that is running, as the browser describes it: its id, its
  // name, which way it faces, and the size its picture was said to be.
  let running = null;
  // Where in the browser's list of cameras the last change of camera went,
  // for a browser that does not say which of them is running. Until there
  // has been a change, that is taken to be the first.
  let changedTo = 0;

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

  /**
   * The least time from the taking of one frame to the taking of the next,
   * in milliseconds, as the engine says on the scanner it has drawn.
   */
  function every() {
    const scanner = document.querySelector('[data-qr-camera]');
    const said = Number(scanner && scanner.getAttribute('data-qr-every'));
    return said > 0 ? said : EVERY_UNSAID;
  }

  /** Take the next frame after `wait` milliseconds, in place of any wait running. */
  function takeFrameIn(wait) {
    clearTimeout(frameTimer);
    frameTimer = setTimeout(takeFrame, wait);
  }

  /**
   * Take a frame from the camera's picture and hand it to the worker. The
   * worker says when it is done with it (QR_DONE, below), and the next is
   * taken then, or when the engine's time between frames is up if that is
   * later.
   */
  function takeFrame() {
    if (!scanning) return;
    const video = scannerVideo();
    const worker = globalThis.kipukasWorker;
    if (!video || !worker || !frameCtx || video.readyState !== video.HAVE_ENOUGH_DATA) {
      // No picture yet.
      takeFrameIn(every());
      return;
    }

    let imageData;
    try {
      frameCtx.drawImage(video, 0, 0, frameCanvas.width, frameCanvas.height);
      imageData = frameCtx.getImageData(0, 0, frameCanvas.width, frameCanvas.height);
    } catch (_) {
      // The picture could not be drawn this time (a browser may refuse a
      // video between two of its frames). The next try is not lost with it.
      takeFrameIn(every());
      return;
    }
    frameTakenAt = performance.now();
    // The pixels are handed over, not copied.
    worker.postMessage(
      {
        type: 'QR_FRAME',
        pixels: imageData.data.buffer,
        width: frameCanvas.width,
        height: frameCanvas.height,
      },
      [imageData.data.buffer],
    );
    takeFrameIn(UNANSWERED);
  }

  /** Show the camera's picture in the scanner's video and start reading frames. */
  function show(video, mediaStream) {
    frameCanvas = video.closest('[data-qr-camera]').querySelector('#canvas');
    frameCtx = frameCanvas.getContext('2d', { willReadFrequently: true, alpha: false });
    video.srcObject = mediaStream;
    video.setAttribute('playsinline', 'true');
    // A picture that is replaced before it starts (the camera was switched)
    // gives up its turn to the next: nothing to report.
    const playing = video.play();
    if (playing) playing.catch(() => {});
    // The size of the picture, when the video has it and when it changes.
    video.onloadedmetadata = tellCamera;
    video.onresize = tellCamera;
    takeFrameIn(0);
  }

  /**
   * The camera the engine remembers the scanner last ran, as it has written
   * it on the scanner: the one to ask the browser for. All empty before any
   * camera has run.
   */
  function remembered() {
    const scanner = document.querySelector('[data-qr-camera]');
    const said = (name) => (scanner && scanner.getAttribute(name)) || '';
    return {
      id: said('data-camera-id'),
      label: said('data-camera-label'),
      facing: said('data-camera-facing'),
    };
  }

  /** What the browser says of the camera behind a stream. */
  function described(mediaStream) {
    const track = mediaStream.getVideoTracks()[0];
    const settings = track && track.getSettings ? track.getSettings() : {};
    return {
      id: settings.deviceId || '',
      label: (track && track.label) || '',
      facing: settings.facingMode || '',
      width: settings.width || 0,
      height: settings.height || 0,
    };
  }

  /**
   * Tell the engine which camera is running and the size of its picture: the
   * video's own once it has one (a phone turned on its side changes it).
   * Resolves when the engine has drawn the scanner for it.
   */
  function tellCamera() {
    if (!running) return Promise.resolve();
    const video = scannerVideo();
    const showing = video && video.srcObject === stream && video.videoWidth > 0;
    const said = new URLSearchParams({
      action: 'camera',
      id: running.id,
      label: running.label,
      facing: running.facing,
      width: showing ? video.videoWidth : running.width,
      height: showing ? video.videoHeight : running.height,
    });
    return globalThis.kipukasViews.send('/api/qr/status', said.toString());
  }

  /**
   * Start a camera and begin reading frames from it: `chosen` if the player
   * has just changed camera, else the one the engine remembers, else one
   * that faces the player.
   */
  async function start(chosen) {
    if (scanning || starting) return;
    starting = true;

    try {
      let want = chosen || remembered();
      // Whether the camera that starts may still be exchanged for the one
      // wanted (see below). Once.
      let exchange = !chosen;
      let mediaStream;
      let got;
      for (;;) {
        await enumerateDevices();
        // The browser's id for the camera wanted: the one remembered or,
        // where the browser has given the camera a new id since, that of
        // the camera with its name.
        const known = videoDevices.find((device) => want.id && device.deviceId === want.id) ||
          videoDevices.find((device) => want.label && device.label === want.label);
        const id = (known && known.deviceId) || want.id;
        // Asked for as preferences, so that a camera that is gone, or an
        // id the browser no longer knows, still starts one.
        const video = { focusMode: 'continuous' };
        if (id) video.deviceId = { ideal: id };
        if (want.facing) video.facingMode = { ideal: want.facing };
        else if (!id) video.facingMode = 'user';

        mediaStream = await navigator.mediaDevices.getUserMedia({ video, audio: false });
        got = described(mediaStream);
        // The browser names its cameras once one has been allowed.
        await enumerateDevices();

        // A browser that gives its cameras new ids on each visit, and
        // names none until one runs, could only be asked for a camera
        // facing the same way. If that started another than the one
        // remembered by name, start that one in its place.
        const meant = exchange && want.label && got.label !== want.label &&
          videoDevices.find((device) =>
            device.label === want.label && device.deviceId && device.deviceId !== got.id
          );
        if (!meant) break;
        mediaStream.getTracks().forEach((track) => track.stop());
        want = { id: meant.deviceId, label: meant.label, facing: want.facing };
        exchange = false;
      }

      // The scanner may have been closed while the camera was being asked for.
      if (!scannerVideo()) {
        mediaStream.getTracks().forEach((track) => track.stop());
        return;
      }
      stream = mediaStream;
      running = got;
      // Told before the picture is shown, so that it is shown the right
      // way round from its first frame.
      await tellCamera();
      const target = scannerVideo();
      if (!target) {
        stop();
        return;
      }
      scanning = true;
      show(target, mediaStream);
      console.log(
        `[qr-camera] Camera started (${
          got.label || 'unnamed'
        }), a frame every ${every()} ms at most`,
      );
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
    clearTimeout(frameTimer);
    frameTimer = null;

    if (stream) {
      stream.getTracks().forEach((track) => track.stop());
      stream = null;
    }
    running = null;

    clearBboxOverlay();
    console.log('[qr-camera] Camera stopped');
  }

  /**
   * Change to the next of the device's cameras, in the browser's order, and
   * round to the first again. Where the browser names no cameras to choose
   * between, ask for one that faces the other way.
   */
  function switchCamera() {
    if (starting) return;
    const named = videoDevices.filter((device) => device.deviceId);
    let next;
    if (named.length > 1) {
      // The one after the camera that is running: known by its id, or its
      // name, or failing both by where the last change went.
      const now = running || {};
      let at = named.findIndex((device) => now.id && device.deviceId === now.id);
      if (at < 0) at = named.findIndex((device) => now.label && device.label === now.label);
      if (at < 0) at = changedTo;
      changedTo = (at + 1) % named.length;
      const device = named[changedTo];
      next = { id: device.deviceId, label: device.label, facing: '' };
    } else {
      const away = (running || remembered()).facing === 'environment';
      next = { id: '', label: '', facing: away ? 'user' : 'environment' };
    }
    console.log(`[qr-camera] Switching camera: ${next.label || next.facing}`);

    stop();
    start(next);
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

  // What the worker says of the frames: that it is done with one, and what
  // it found in it, which is drawn over the picture.
  function setupWorkerListener() {
    const worker = globalThis.kipukasWorker;
    if (!worker) {
      // Worker not yet available (kipukas-api.js may not have loaded yet).
      // Retry after a short delay.
      setTimeout(setupWorkerListener, 100);
      return;
    }

    worker.addEventListener('message', (event) => {
      if (!scanning) return;
      if (event.data?.type === 'QR_DONE') {
        // The worker is done with the last frame: the next is taken as
        // soon as the engine's time between frames allows.
        takeFrameIn(Math.max(0, every() - (performance.now() - frameTakenAt)));
        return;
      }
      if (event.data?.type !== 'QR_BBOX') return;
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
