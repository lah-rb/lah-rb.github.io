// Records what the scanner asks of the browser and what it is given, on the page it is put into.
// It changes nothing the page does: every wrapped call is passed through as it was.
// What is recorded is kept in the page (and carried to the tab's next page in sessionStorage)
// until the recorder on the Mac takes it with __kpkDiag.drain().
(() => {
  if (window.__kpkDiag) return 'already';
  const CARRY = '__kpk_diag_carry';
  const entries = [];
  try {
    entries.push(...JSON.parse(sessionStorage.getItem(CARRY) || '[]'));
  } catch (_) { /* nothing carried */ }
  const short = (id) => (id ? String(id).slice(0, 6) + '~' + String(id).length : '');
  const carry = () => {
    try {
      if (entries.length) sessionStorage.setItem(CARRY, JSON.stringify(entries.slice(-300)));
      else sessionStorage.removeItem(CARRY);
    } catch (_) { /* no storage */ }
  };
  const note = (what, data) => {
    entries.push({
      at: new Date().toISOString().slice(11, 23),
      page: location.pathname + location.hash,
      what,
      data,
    });
    carry();
  };
  const shortCamera = (text) => {
    try {
      const camera = JSON.parse(text);
      if (camera && typeof camera === 'object' && 'id' in camera) camera.id = short(camera.id);
      return camera;
    } catch (_) {
      return text;
    }
  };
  const storedNow = () => {
    const out = {};
    for (
      const key of [
        'kipukas-scanner-camera',
        'kipukas-scanner-light',
        'kipukas-cv-enabled',
        'qr-privacy-accepted',
      ]
    ) {
      let value = null;
      try {
        value = localStorage.getItem(key);
      } catch (_) { /* no storage */ }
      out[key] = key === 'kipukas-scanner-camera' && value ? shortCamera(value) : value;
    }
    return out;
  };
  const drawnNow = () => {
    const scanner = document.querySelector('[data-qr-camera]');
    if (!scanner) return null;
    const video = scanner.querySelector('video');
    const track = video && video.srcObject && video.srcObject.getVideoTracks()[0];
    return {
      wantedId: short(scanner.getAttribute('data-camera-id')),
      wantedFacing: scanner.getAttribute('data-camera-facing'),
      hasFacingAttribute: scanner.hasAttribute('data-camera-facing'),
      mirrored: scanner.hasAttribute('data-mirrored'),
      showing: track ? `${track.label} (${track.readyState}${track.muted ? ', muted' : ''})` : null,
    };
  };
  const describeTrack = (track) => {
    const settings = track.getSettings ? track.getSettings() : {};
    let can = {};
    try {
      can = track.getCapabilities ? track.getCapabilities() : {};
    } catch (_) { /* not offered */ }
    return {
      label: track.label,
      id: short(settings.deviceId),
      facing: settings.facingMode === undefined ? '(not said)' : settings.facingMode,
      size: `${settings.width}x${settings.height}`,
      frameRate: settings.frameRate,
      zoom: settings.zoom,
      canFace: can.facingMode,
    };
  };

  // ── The page as it starts ──
  const standalone = matchMedia('(display-mode: standalone)').matches ||
    navigator.standalone === true;
  note('page', {
    href: location.href,
    standalone,
    visible: document.visibilityState,
    stored: storedNow(),
    scanner: drawnNow(),
  });
  (async () => {
    const text = async (path) => {
      try {
        return await (await fetch(path)).text();
      } catch (_) {
        return '';
      }
    };
    const [camera, api, worker] = await Promise.all([
      text('/assets/js/qr-camera.js'),
      text('/assets/js/kipukas-api.js'),
      text('/assets/js/kipukas-worker.js'),
    ]);
    let permission = '';
    try {
      permission = (await navigator.permissions.query({ name: 'camera' })).state;
    } catch (e) {
      permission = 'cannot be asked: ' + e.name;
    }
    let registration = null;
    try {
      registration = await navigator.serviceWorker.getRegistration();
    } catch (_) { /* none */ }
    note('build', {
      cameraScript: camera.length,
      cameraScriptKeepsChoice: /data-camera-facing/.test(camera) && /chosen/.test(camera),
      cameraScriptTakesShownPart: /clientWidth/.test(camera),
      apiScript: api.length,
      apiReadsCameraAtStart: /kipukas-scanner-camera/.test(api),
      workerScript: worker.length,
      workerSaysDone: /QR_DONE/.test(worker),
      permission,
      serviceWorker: registration
        ? {
          controller: !!navigator.serviceWorker.controller,
          waiting: !!registration.waiting,
          installing: !!registration.installing,
        }
        : null,
    });
  })();

  // ── What the browser is asked for cameras, and what it gives ──
  const media = navigator.mediaDevices;
  if (media) {
    const list = media.enumerateDevices.bind(media);
    media.enumerateDevices = async (...args) => {
      const devices = await list(...args);
      note(
        'cameras listed',
        devices.filter((d) => d.kind === 'videoinput').map((d) =>
          `${d.label || '(unnamed)'} ${short(d.deviceId) || '(no id)'}`
        ),
      );
      return devices;
    };
    const ask = media.getUserMedia.bind(media);
    media.getUserMedia = async (constraints) => {
      const video = constraints && constraints.video;
      const asked = JSON.parse(JSON.stringify(video === undefined ? null : video));
      if (asked && asked.deviceId) {
        const id = asked.deviceId;
        asked.deviceId = typeof id === 'string'
          ? short(id)
          : Object.fromEntries(Object.entries(id).map(([k, v]) => [k, short(v)]));
      }
      note('camera asked for', asked);
      const began = performance.now();
      try {
        const stream = await ask(constraints);
        const track = stream.getVideoTracks()[0];
        note('camera given', {
          after: Math.round(performance.now() - began) + ' ms',
          ...describeTrack(track),
        });
        for (const event of ['ended', 'mute', 'unmute']) {
          track.addEventListener(
            event,
            () => note('camera track ' + event, { label: track.label }),
          );
        }
        const end = track.stop.bind(track);
        track.stop = () => {
          note('camera stopped by the page', { label: track.label });
          return end();
        };
        return stream;
      } catch (error) {
        note('camera refused', {
          after: Math.round(performance.now() - began) + ' ms',
          name: error.name,
          message: error.message,
        });
        throw error;
      }
    };
    media.addEventListener('devicechange', () => note('cameras changed', null));
  }

  // ── What the page tells the engine of the scanner, and what the engine draws for it ──
  const wrapSend = () => {
    const views = globalThis.kipukasViews;
    if (!views || views.__kpkWrapped) return !!views;
    const send = views.send.bind(views);
    views.send = (path, body) => {
      const result = send(path, body);
      if (path === '/api/qr/status') {
        const said = Object.fromEntries(new URLSearchParams(body || ''));
        if (said.id) said.id = short(said.id);
        note('engine told', said);
        Promise.resolve(result).then(
          () =>
            note('engine drew', {
              scanner: drawnNow(),
              stored: storedNow()['kipukas-scanner-camera'],
            }),
          () => {},
        );
      }
      return result;
    };
    views.__kpkWrapped = true;
    return true;
  };
  if (!wrapSend()) {
    const waiting = setInterval(() => {
      if (wrapSend()) clearInterval(waiting);
    }, 50);
  }

  // ── What is stored ──
  const set = Storage.prototype.setItem;
  Storage.prototype.setItem = function (key, value) {
    if (this === localStorage && /scanner|qr-privacy|cv-enabled/.test(key)) {
      note('stored', { key, value: key === 'kipukas-scanner-camera' ? shortCamera(value) : value });
    }
    return set.call(this, key, value);
  };
  const remove = Storage.prototype.removeItem;
  Storage.prototype.removeItem = function (key) {
    if (this === localStorage && /scanner|qr-privacy|cv-enabled/.test(key)) {
      note('removed from storage', { key });
    }
    return remove.call(this, key);
  };

  // ── The scanner coming and going, and the player's taps ──
  let was = null;
  const watch = () => {
    const now = JSON.stringify(drawnNow());
    if (now !== was) {
      was = now;
      note('scanner drawn', JSON.parse(now));
    }
  };
  new MutationObserver(watch).observe(document, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ['data-camera-id', 'data-camera-facing', 'data-mirrored'],
  });
  document.addEventListener('click', (event) => {
    const target = event.target;
    if (!target || !target.closest) return;
    if (target.closest('[data-qr-switch]')) note('tap: switch camera', null);
    else if (target.closest('button[aria-label="Toggle QR Scanner"]')) note('tap: QR button', null);
    else if (target.closest('[data-engine-body="action=close"]')) note('tap: close scanner', null);
  }, true);

  // ── The page being hidden, shown, left, or brought back from the browser's page cache ──
  document.addEventListener('visibilitychange', () => note('visibility', document.visibilityState));
  addEventListener('pagehide', (event) => note('page left', { keptInCache: event.persisted }));
  addEventListener('pageshow', (event) => {
    if (event.persisted) {
      note('page brought back from cache', { stored: storedNow(), scanner: drawnNow() });
    }
  });

  // ── What the scanner's own script says ──
  for (const level of ['log', 'error', 'warn']) {
    const original = console[level].bind(console);
    console[level] = (...args) => {
      if (typeof args[0] === 'string' && args[0].includes('[qr-camera]')) {
        note(
          'script says',
          args.map((a) => (a instanceof Error ? `${a.name}: ${a.message}` : String(a))).join(' ')
            .replace(/\s+/g, ' '),
        );
      }
      return original(...args);
    };
  }

  window.__kpkDiag = {
    drain() {
      const out = entries.splice(0);
      carry();
      return JSON.stringify(out);
    },
  };
  return 'installed';
})();
