/**
 * kipukas-crypto.js — AES-GCM encryption layer for player data exports.
 *
 * Phase D: Provides confidentiality on top of the Rust HMAC-SHA256 integrity
 * layer. Uses Web Crypto API (universally supported since 2017) for:
 *   - PBKDF2 key derivation (passphrase → AES-256 key)
 *   - AES-GCM authenticated encryption
 *
 * Lazy-loaded only when the user opens the export/import modal.
 *
 * Export file format (.kipukas):
 * {
 *   "version": 1,
 *   "format": "kipukas-player-export",
 *   "salt": "<base64 16-byte PBKDF2 salt>",
 *   "iv": "<base64 12-byte AES-GCM IV>",
 *   "data": "<base64 AES-GCM ciphertext of inner payload>"
 * }
 */

const PBKDF2_ITERATIONS = 100000;

/**
 * Derive an AES-256-GCM key from a passphrase using PBKDF2.
 * @param {string} passphrase
 * @param {Uint8Array} salt - 16-byte salt
 * @returns {Promise<CryptoKey>}
 */
async function deriveKey(passphrase, salt) {
  const enc = new TextEncoder();
  const keyMaterial = await crypto.subtle.importKey(
    'raw',
    enc.encode(passphrase),
    'PBKDF2',
    false,
    ['deriveKey'],
  );
  return crypto.subtle.deriveKey(
    { name: 'PBKDF2', salt, iterations: PBKDF2_ITERATIONS, hash: 'SHA-256' },
    keyMaterial,
    { name: 'AES-GCM', length: 256 },
    false,
    ['encrypt', 'decrypt'],
  );
}

/**
 * Encode a Uint8Array as URL-safe base64 (no padding).
 * @param {Uint8Array} bytes
 * @returns {string}
 */
function toBase64(bytes) {
  let binary = '';
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(
    /=+$/,
    '',
  );
}

/**
 * Decode URL-safe base64 (no padding) to Uint8Array.
 * @param {string} b64
 * @returns {Uint8Array}
 */
function fromBase64(b64) {
  const standard = b64.replace(/-/g, '+').replace(/_/g, '/');
  const pad = (4 - (standard.length % 4)) % 4;
  const padded = standard + '='.repeat(pad);
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/**
 * Encrypt an inner payload JSON string with AES-GCM.
 * @param {string} passphrase - User's passphrase
 * @param {string} innerPayloadJSON - JSON string from WASM signed export
 * @returns {Promise<string>} - Outer envelope JSON string for the .kipukas file
 */
export async function encryptExport(passphrase, innerPayloadJSON) {
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const key = await deriveKey(passphrase, salt);

  const enc = new TextEncoder();
  const ciphertext = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv },
    key,
    enc.encode(innerPayloadJSON),
  );

  return JSON.stringify({
    version: 1,
    format: 'kipukas-player-export',
    salt: toBase64(salt),
    iv: toBase64(iv),
    data: toBase64(new Uint8Array(ciphertext)),
  });
}

/**
 * Decrypt a .kipukas file envelope back to the inner payload JSON.
 * @param {string} passphrase - User's passphrase
 * @param {string} outerJSON - Content of the .kipukas file
 * @returns {Promise<string>} - Inner payload JSON string
 * @throws {Error} on wrong passphrase, tampered data, or invalid format
 */
export async function decryptImport(passphrase, outerJSON) {
  const envelope = JSON.parse(outerJSON);

  if (envelope.format !== 'kipukas-player-export') {
    throw new Error('Not a valid Kipukas export file');
  }
  if (envelope.version !== 1) {
    throw new Error('Unsupported export version: ' + envelope.version);
  }

  const salt = fromBase64(envelope.salt);
  const iv = fromBase64(envelope.iv);
  const ciphertext = fromBase64(envelope.data);
  const key = await deriveKey(passphrase, salt);

  try {
    const plaintext = await crypto.subtle.decrypt(
      { name: 'AES-GCM', iv },
      key,
      ciphertext,
    );
    return new TextDecoder().decode(plaintext);
  } catch (_e) {
    throw new Error(
      'Decryption failed — wrong passphrase or file has been tampered with',
    );
  }
}
