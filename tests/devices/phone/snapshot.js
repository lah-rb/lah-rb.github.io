// For evaluate.py: what the page has stored for the scanner, what Safari says
// of its cameras before any has been allowed on the page, and whether the
// scanner is open.
const short = (id) => (id ? id.slice(0, 6) + '~' + id.length : '');
const stored = {};
for (
  const key of [
    'kipukas-scanner-camera',
    'kipukas-scanner-light',
    'kipukas-cv-enabled',
    'qr-privacy-accepted',
    'kipukas-scanner-status',
  ]
) {
  stored[key] = localStorage.getItem(key);
}
try {
  const camera = JSON.parse(stored['kipukas-scanner-camera']);
  if (camera && camera.id) camera.id = short(camera.id);
  stored['kipukas-scanner-camera'] = camera;
} catch (_) { /* as it is stored */ }

let permission = '';
try {
  permission = (await navigator.permissions.query({ name: 'camera' })).state;
} catch (error) {
  permission = 'cannot be asked: ' + error.name;
}
const cameras = (await navigator.mediaDevices.enumerateDevices())
  .filter((device) => device.kind === 'videoinput')
  .map((device) => `${device.label || '(unnamed)'} ${short(device.deviceId) || '(no id)'}`);
const registration = await navigator.serviceWorker.getRegistration();
const scanner = document.querySelector('[data-qr-camera]');
return {
  page: location.href,
  homeScreenApp: matchMedia('(display-mode: standalone)').matches || navigator.standalone === true,
  visible: document.visibilityState,
  stored,
  permission,
  cameras,
  updateWaiting: !!(registration && registration.waiting),
  scannerScriptLoaded: !!document.querySelector('script[src*="qr-camera"]'),
  scanner: scanner
    ? {
      asksForId: short(scanner.getAttribute('data-camera-id')),
      asksForFacing: scanner.getAttribute('data-camera-facing'),
      mirrored: scanner.hasAttribute('data-mirrored'),
    }
    : null,
};
