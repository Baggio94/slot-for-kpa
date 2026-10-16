// Android-only source adapter for the unchanged official Cart Studio.
// All carts come from Slot's previously indexed, read-only SAF library.
// The WebView only sees opaque IDs, never arbitrary SAF URI or filesystem paths.
import { labelKey } from './card.js';

function decode(encoded) {
  const raw = atob(encoded || '');
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return bytes;
}

function encode(bytes) {
  const chunks = [];
  for (let i = 0; i < bytes.length; i += 8192) {
    chunks.push(String.fromCharCode(...bytes.subarray(i, i + 8192)));
  }
  return btoa(chunks.join(''));
}

const PLATFORMS = new Set(['GB', 'GBC', 'GBA']);

function loadFile(bridge, cart) {
  // The original Studio asks for file.slice(header) and CRC32. Android
  // computes the CRC once while streaming SAF and caches it with size/mtime.
  // Resolve this only when identify() reaches the cart, not at page launch.
  let file = null;
  return async () => {
    if (!file) {
      const fingerprint = cart.fingerprint || JSON.parse(bridge.fingerprint(cart.id));
      if (!Number.isSafeInteger(fingerprint.crc) || fingerprint.crc < 0 ||
          fingerprint.crc > 0xffffffff) throw new Error('Invalid cartridge CRC32');
      const header = decode(fingerprint.head);
      if (header.length !== 0x150) throw new Error('Incomplete cartridge header');
      file = {
        slotCrc: fingerprint.crc >>> 0,
        slice(start = 0, end = header.length) {
          const bytes = header.slice(start, end);
          return { arrayBuffer: async () => bytes.buffer };
        },
      };
    }
    return file;
  };
}

export function fromAndroid(bridge) {
  const data = JSON.parse(bridge.session());
  if (!Array.isArray(data.carts)) throw new Error('Slot ROM catalog not available');
  const carts = [];
  const labels = new Map();
  const identities = new Set();
  for (const cart of data.carts) {
    if (!PLATFORMS.has(cart.platform) || typeof cart.stem !== 'string' ||
        !cart.stem || typeof cart.id !== 'string' ||
        !/^[0-9a-f]{16}$/.test(cart.id)) {
      throw new Error('Invalid cartridge in Slot library');
    }
    const token = labelKey(cart.platform, cart.stem);
    if (identities.has(token)) {
      // The original Studio writes by platform/stem, so it cannot select
      // one of two identically named carts safely. Stop instead of mixing labels.
      throw new Error('Duplicate cartridge name: ' + cart.stem);
    }
    identities.add(token);
    carts.push({ platform: cart.platform, stem: cart.stem,
                 file: loadFile(bridge, cart), slotId: cart.id });
    if (cart.hasLabel) {
      labels.set(token, async () => {
        const encoded = bridge.readLabel(cart.id);
        if (encoded.startsWith('error:')) throw new Error(encoded);
        const bytes = decode(encoded);
        if (!bytes.length) throw new Error('Saved label missing for ' + cart.stem);
        return new Blob([bytes], { type: 'image/png' });
      });
    }
  }
  // Alphabetical order only affects presentation, never the ROM identity,
  // No-Intro CRC, private label key or save target. Keep a predictable
  // case/accent-insensitive order within each original Studio platform tab.
  const selectedKey = data.selectedKey || '';
  const compareTitles = new Intl.Collator(undefined, {
    usage: 'sort', sensitivity: 'base', numeric: true,
  });
  carts.sort((a, b) =>
    compareTitles.compare(a.stem, b.stem) ||
    a.stem.localeCompare(b.stem) ||
    a.slotId.localeCompare(b.slotId));
  // X still opens directly on the selected game by opaque key; sorting does
  // not change this selection (the full catalog path has no selected key).
  return {
    carts,
    labels,
    selectedKey,
    allCached: data.allCached === true,
    focused: !!selectedKey,
    direct: true,
    systemShells: async () => '',
    labelShells: async () => bridge.labelShells(),
    async write(platform, stem, bytes, replace = false) {
      if (!identities.has(labelKey(platform, stem))) {
        throw new Error('Cannot write to a cartridge outside Slot library');
      }
      const result = bridge.saveLabel(platform, stem, encode(bytes), replace);
      if (!['written', 'skipped'].includes(result)) {
        throw new Error((result || 'Label save failed') + ' (' + platform + '/' + stem + ')');
      }
      return result;
    },
    async writeShells(text) {
      if (bridge.saveShells(text) !== true) throw new Error('Shell save failed');
    },
  };
}
