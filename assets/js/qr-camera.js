/**
 * qr-camera.js — the camera of the QR scanner.
 *
 * The scanner itself is an engine-owned view (kipukas-view.js) of
 * /api/qr/status: the engine holds whether it is open, draws it, and takes
 * every tap on its controls (kipukas-server/src/routes/qr.rs). What only a
 * page can do is left to this file:
 *
 *   - run the camera for as long as the scanner's video is drawn, and stop it
 *     when it is not, or when the page is left (browser getUserMedia API);
 *   - hand frames to the engine's worker, which finds and reads the codes in
 *     them and posts what a code says to the engine (kipukas-worker.js). A
 *     frame is the part of the camera's picture that the scanner shows, and
 *     no more: what the player aims at is what is read. How
 *     often is the engine's to say (data-qr-every on the scanner); the next
 *     frame is taken only when the worker is done with the last;
 *   - change which camera is in use, of those the device has (the button the
 *     engine marks data-qr-switch), and tell the engine which is running and
 *     the size of its picture. The engine shows the picture mirrored or not
 *     by which way that camera faces. It keeps the camera the player changed
 *     to, and writes it on the scanner for this file to ask the browser for
 *     the next time;
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
   * The part of the camera's picture that the video shows, in the picture's
   * own pixels: left, top, width, height. The engine draws the video to
   * fill its place (object-fit: cover), so a picture of another shape than
   * that place is shown with its middle in view and the rest cut off, by
   * the same amount on either side. Null until both have a size.
   */
  function shownPart(video) {
    const across = video.videoWidth;
    const down = video.videoHeight;
    const placeAcross = video.clientWidth;
    const placeDown = video.clientHeight;
    if (!across || !down || !placeAcross || !placeDown) return null;
    // How much the picture is enlarged to fill the place.
    const scale = Math.max(placeAcross / across, placeDown / down);
    const width = placeAcross / scale;
    const height = placeDown / scale;
    return [(across - width) / 2, (down - height) / 2, width, height];
  }

  /**
   * Take a frame and hand it to the worker: the part of the camera's
   * picture that the scanner shows, drawn to the size of the canvas frames
   * are read through. The worker says when it is done with it (QR_DONE,
   * below), and the next is taken then, or when the engine's time between
   * frames is up if that is later.
   */
  function takeFrame() {
    if (!scanning) return;
    const video = scannerVideo();
    const worker = globalThis.kipukasWorker;
    const part = video && shownPart(video);
    if (!part || !worker || !frameCtx || video.readyState !== video.HAVE_ENOUGH_DATA) {
      // No picture yet.
      takeFrameIn(every());
      return;
    }

    let imageData;
    try {
      frameCtx.drawImage(video, ...part, 0, 0, frameCanvas.width, frameCanvas.height);
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
    video.onloadedmetadata = () => tellCamera();
    video.onresize = () => tellCamera();
    takeFrameIn(0);
  }

  /**
   * The camera the engine has kept as the player's choice, as it has
   * written it on the scanner: its id, and the way it faces. Both empty
   * before any camera has run.
   */
  function remembered() {
    const scanner = document.querySelector('[data-qr-camera]');
    const said = (name) => (scanner && scanner.getAttribute(name)) || '';
    return { id: said('data-camera-id'), facing: said('data-camera-facing') };
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
   * `chosen`: the player has just changed to this camera, which the engine
   * keeps as their choice. Resolves when the engine has drawn the scanner
   * for it.
   */
  function tellCamera(chosen) {
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
    if (chosen === true) said.set('chosen', 'true');
    return globalThis.kipukasViews.send('/api/qr/status', said.toString());
  }

  /**
   * Start a camera and begin reading frames from it: `chosen` if the player
   * has just changed camera, else the one the engine has kept, else one
   * that faces the player.
   *
   * A camera is asked for by its id only where the browser shows, before
   * any camera runs, that it still has one of that id. Where it does not
   * (an iPhone names no camera until one has been allowed on the page), it
   * is asked only for one that faces the same way, and chooses which
   * itself: an id it does not know is not offered to it, and an iPhone's
   * rear camera changes lens by itself as the card comes near.
   */
  async function start(chosen) {
    if (scanning || starting) return;
    starting = true;

    try {
      const want = chosen || remembered();
      await enumerateDevices();
      const listed = want.id && videoDevices.some((device) => device.deviceId === want.id);
      const video = { focusMode: 'continuous' };
      if (listed) video.deviceId = { ideal: want.id };
      else video.facingMode = want.facing || 'user';

      const mediaStream = await navigator.mediaDevices.getUserMedia({ video, audio: false });

      // The scanner may have been closed while the camera was being asked for.
      if (!scannerVideo()) {
        mediaStream.getTracks().forEach((track) => track.stop());
        return;
      }
      stream = mediaStream;
      running = described(mediaStream);
      // The browser names its cameras once one has been allowed: for the
      // button that changes camera.
      enumerateDevices();
      // Told before the picture is shown, so that it is shown the right
      // way round from its first frame.
      await tellCamera(!!chosen);
      const target = scannerVideo();
      if (!target) {
        stop();
        return;
      }
      scanning = true;
      show(target, mediaStream);
      console.log(
        `[qr-camera] Camera started (${
          running.label || 'unnamed'
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
      next = { id: device.deviceId, facing: '' };
    } else {
      const away = (running || remembered()).facing === 'environment';
      next = { id: '', facing: away ? 'user' : 'environment' };
    }
    console.log(`[qr-camera] Switching camera: ${next.id ? 'the next' : next.facing}`);

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

  // A page that is left may be kept by the browser, to be brought back whole
  // on Back. Its camera is not kept with it, so it is stopped here: the page
  // does not come back holding a picture that has ended. The engine closes
  // the scanner of a page that comes back (kipukas-server/src/store.rs).
  globalThis.addEventListener('pagehide', () => {
    if (scanning) stop();
  });

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
