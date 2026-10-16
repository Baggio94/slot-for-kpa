// slot cart studio. Everything that needs a browser lives here: the card, the network and the page.
// Everything that decides what a label looks like is Rust, in pkg/.

import init, {
  Crc32,
  Dat,
  Label,
  Zip,
  auto_shell,
  box_hue,
  cart_shells,
  layered_cart_shells,
  cart_size,
  clean_label,
  existing_face,
  fallback_hue,
  header_code,
  label_tags,
  logo_luma,
  merge_cart_shells,
  shell_key_ok,
  shell_presets,
  stop_colours,
  stub_target,
  thumbnail_name,
} from './pkg/slot_cart_studio.js';
import { fromDirectory, fromFiles, labelKey, PLATFORMS } from './card.js';
import { fetchArt, fetchDat, fetchIndex, fetchThumb, limiter } from './libretro.js';
import { createEditor } from './editor.js';
import { amend, commit } from './history.js';
import { smallPrint } from './smallprint.js';
import { hasLog, logError, logText } from './log.js';
import { applyFill, autoFill, available, fillCounts, fillText, wears } from './label-choice.js';

const $ = (id) => document.getElementById(id);
const limit = limiter(4);
// The GBA game code sits at 0xAC and the Game Boy header ends at 0x150; nothing past that is read.
const HEAD = 0x150;
const FATAL = 'The studio ran out of memory or hit an internal error. Reload the page to start again.';
const BUILD = 'dev';

// The box each platform's cart is drawn in, taken from slot at startup. A Game Boy Game Pak is
// the same width as a GBA cart and nearly twice as tall, so a canvas cut for one refuses the
// other's pixels outright rather than drawing them badly.
const faceBox = new Map();
const boxOf = (platform) => faceBox.get(platform) ?? faceBox.get('GBA');
// One parsed No-Intro database per platform, kept across cards: the databases are large and a
// second card of the same platform should not fetch one again.
const dats = new Map();
let session = null;
let busy = 0;
let writing = false;
// Set by the first WASM trap. What the module holds is undefined after one (a RefCell can stay
// borrowed, and then every face panics), so nothing calls into it again.
let fatal = false;

const track = (promise) => {
  busy++;
  return promise.finally(() => {
    busy--;
  });
};

function newCart({ platform, stem, file }) {
  return {
    // The folder the rom sits in, and so the only thing that says which console it is for.
    platform,
    stem,
    file,
    code: '',
    // A Game Boy pak's header, which picks its shell and plastic. Empty on a GBA cart.
    head: new Uint8Array(0),
    shell: '',
    shellOnCard: '',
    existingPng: null,
    crc: null,
    game: null,
    // Logos stay PNG bytes, which are small. The label made from one is not kept: see withLabel.
    logoBytes: null,
    // The real label's ground and band, measured from its scan: sure looks only.
    look: null,
    choice: null,
    byHand: false,
    customLogoBytes: null,
    customLogoLuma: 255,
    customLabelBytes: null,
    real: null,
    dressed: false,
    rejected: false,
    boxHue: null,
    // How bright the logo is decides which way the ground goes, so it is read once with the logo
    // and kept. 255 until there is a logo: nothing is drawn before then anyway.
    logoLuma: 255,
    deep: null,
    userHue: false,
    existing: null,
    error: null,
    result: null,
    looking: true,
    el: null,
  };
}

const realPathOf = (c) => {
  const key = c.rejected ? null : artKey(c);
  return (key && artIndex?.[key]?.['support-texture']) || null;
};

const realOf = (c) => (c.real && c.real.path === realPathOf(c) ? c.real : null);
const realBytesOf = (c) => realOf(c)?.bytes ?? null;

const view = (c) => ({
  choice: c.choice,
  byHand: c.byHand,
  onCard: !!c.existingPng,
  hasReal: !realOf(c)?.failed && !!realPathOf(c),
  hasLogo: !c.rejected && !!c.logoBytes,
  customLogoBytes: c.customLogoBytes,
  customLabelBytes: c.customLabelBytes,
  error: c.error,
});

const wearing = (c) => wears(view(c));

function stateOf(c) {
  if (c.error) return 'error';
  const { kind, why } = wearing(c);
  if (kind === 'card') return 'has-label';
  if (c.looking || (kind === 'real' && !realBytesOf(c))) return 'looking';
  if (kind === 'none') return why === 'unfilled' ? 'unfilled' : 'needs-logo';
  return 'ready';
}

// The box art belongs to the matched game, so its hue only stands while the match does.
const baseHueOf = (c) =>
  c.game && !c.rejected && c.boxHue !== null ? c.boxHue : fallback_hue(c.stem);

// A label is described by its deep corner, whether computed or picked, and the corner a hue
// produces depends on the logo going over it: dark logos get a pale ground, bright ones a deep
// one. stop_colours is that rule, so the page never restates the house numbers itself.
const stopsOf = (c) => stop_colours(baseHueOf(c), lumaOf(c, wearing(c).kind));
const baseDeep = (c) => lookOf(c)?.ground ?? Array.from(stopsOf(c)).slice(0, 3);

const hex = (rgb) => `#${[...rgb].map((v) => v.toString(16).padStart(2, '0')).join('')}`;
// A checksum as the set names its files: eight upper case hex digits.
const crcHex = (crc) => crc.toString(16).toUpperCase().padStart(8, '0');
// Our own set, published beside the studio. `?art=` points at another, a local one while it is
// being harvested, and an empty `?art=` leaves every cart to libretro.
const ART_BASE = new URLSearchParams(location.search).get('art') ?? 'https://art.slot-cfw.fyi/';
let artIndex = null;
const fromHex = (value) => [1, 3, 5].map((at) => parseInt(value.slice(at, at + 2), 16));
// A sure look from the art set, or null: anything missing, unsure or malformed counts as no look.
const rgbOf = (h) => (typeof h === 'string' && /^[0-9a-f]{6}$/i.test(h) ? fromHex(`#${h}`) : null);
function parseLook(raw) {
  if (!raw || raw.sure !== true) return null;
  const ground = rgbOf(raw.ground);
  if (!ground) return null;
  if (!raw.band) return { ground, band: null };
  const { edge, size } = raw.band;
  const colour = rgbOf(raw.band.colour);
  const ok = ['top', 'bottom', 'left', 'right'].includes(edge) && typeof size === 'number' && size > 0 && size < 0.5;
  return ok && colour ? { ground, band: { edge, size, colour } } : null;
}
const lumaOf = (c, kind) => (kind === 'customLogo' ? c.customLogoLuma : c.logoLuma);
const lookOf = (c) => (wearing(c).kind === 'logo' ? c.look : null);
const bandOf = (c) => lookOf(c)?.band ?? null;

const readyCarts = () =>
  session ? session.carts.filter((c) => stateOf(c) === 'ready' && c.result !== 'skipped') : [];

const shellChanges = () => (session ? session.carts.filter((c) => c.shell !== c.shellOnCard) : []);
// Into the Labels layer.
const mergedShells = (s, carts, values) =>
  merge_cart_shells(
    s.shellText ?? '',
    carts.map((c) => c.stem),
    values.map((v, i) => v || (s.systemShells?.has(carts[i].stem.normalize('NFC')) ? 'auto' : '')),
  );
const SHELL_FILE = 'Labels/cart_shell.txt';
const pathKey = (path) => path.normalize('NFC').toLowerCase();

// A Label holds its composition in WASM memory, which never shrinks and stops at 4 GiB.
// One kept per cart runs a big card out of it, so a Label lives only for the call that needs it.
function withLabel(bytes, deep, platform, use, band = null) {
  const label = band
    ? new Label(bytes, deep, platform, band.edge, band.size, new Uint8Array(band.colour))
    : new Label(bytes, deep, platform, '', 0, new Uint8Array(0));
  try {
    return use(label);
  } finally {
    label.free();
  }
}

function withKindLabel(c, kind, use) {
  if (kind === 'real' || kind === 'customLabel') {
    const bytes = kind === 'real' ? realBytesOf(c) : c.customLabelBytes;
    if (!bytes) return null;
    const label = Label.full(bytes, c.platform);
    try {
      return use(label);
    } finally {
      label.free();
    }
  }
  const bytes = kind === 'logo' ? c.logoBytes : kind === 'customLogo' ? c.customLogoBytes : null;
  if (!bytes) return null;
  const look = kind === 'logo' ? c.look : null;
  const deep = c.deep ?? look?.ground ?? Array.from(stop_colours(baseHueOf(c), lumaOf(c, kind))).slice(0, 3);
  return withLabel(bytes, deep, c.platform, use, look?.band ?? null);
}

const withCartLabel = (c, use) => withKindLabel(c, wearing(c).kind, use);

const BLANK = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAIAAAB7QOjdAAAAD0lEQVR4nGO0snJiYGAAAAOUALgNWObMAAAAAElFTkSuQmCC'), (ch) => ch.charCodeAt(0));

function faceFor(c, kind) {
  if (kind === 'card') return c.existing;
  if (kind === 'none') return existing_face(new Uint8Array(0), c.platform, c.code, c.head, c.shell, c.stem);
  if (kind === 'unfilled') {
    const label = Label.full(BLANK, c.platform);
    try {
      return label.face(c.platform, c.code, c.head, c.shell, c.stem);
    } finally {
      label.free();
    }
  }
  return withKindLabel(c, kind, (label) => label.face(c.platform, c.code, c.head, c.shell, c.stem));
}

// Whether slot's decoder takes a logo, asked the only way the module can be asked: by making one.
function readable(bytes) {
  try {
    // Any platform will do: the question is only whether slot's decoder takes these bytes.
    withLabel(bytes, [0, 0, 0], 'GBA', () => {});
    return true;
  } catch (e) {
    if (e instanceof WebAssembly.RuntimeError) throw e;
    return false;
  }
}

// A WebAssembly.RuntimeError is a trap: memory ran out, or Rust panicked. The plain Error a Label
// throws for bytes that aren't a PNG is not one. Returns whether `e` was a trap.
function trapped(e) {
  if (!(e instanceof WebAssembly.RuntimeError)) return false;
  if (!fatal) {
    fatal = true;
    loading(null);
    logError('trap', e);
    $('progress').hidden = true;
    banner(FATAL);
    updateWriteBar();
  }
  return true;
}

// A thumbnail that won't fetch is only a missing image. A trap on the way is not.
const noImage = (e) => {
  if (e instanceof WebAssembly.RuntimeError) throw e;
  return null;
};

function describe(c, state) {
  if (state === 'error') return c.error;
  if (state === 'looking' && !c.looking) return 'Loading the real label…';
  if (state === 'looking') return c.crc === null ? 'Reading the ROM…' : 'Looking it up…';
  if (state === 'unfilled') return '';
  return '';
}

function buildCard(c) {
  const root = $('card-tpl').content.firstElementChild.cloneNode(true);
  const q = (selector) => root.querySelector(selector);
  c.el = { root, canvas: q('canvas'), face: q('.face'), stem: q('.stem'), print: q('.print'), game: q('.game') };
  const [boxW, boxH] = boxOf(c.platform);
  c.el.canvas.width = boxW;
  c.el.canvas.height = boxH;
  // The device's own title, with the file it came from on hover.
  c.el.stem.textContent = clean_label(c.stem);
  c.el.stem.title = c.stem;
  // slot keeps `(USA, Europe)` as one tag so it never claims two releases; the small print reads
  // each region on its own.
  c.tags = label_tags(c.stem)
    .flatMap((group) => group.split(','))
    .map((tag) => tag.trim())
    .filter(Boolean);
  root.setAttribute('aria-label', `Edit ${clean_label(c.stem)}`);
  root.addEventListener('click', () => editor.open(c));
  root.addEventListener('keydown', (e) => {
    if (e.target !== root || (e.key !== 'Enter' && e.key !== ' ')) return;
    e.preventDefault();
    editor.open(c);
  });
  // Always taken, so a file dropped on a cart that can't use it doesn't navigate the page away.
  c.el.face.addEventListener('dragover', (e) => {
    e.preventDefault();
    c.el.face.classList.toggle('over', canLabel(c));
  });
  c.el.face.addEventListener('dragleave', () => c.el.face.classList.remove('over'));
  c.el.face.addEventListener('drop', (e) => {
    e.preventDefault();
    c.el.face.classList.remove('over');
    if (canLabel(c)) track(takeLogo(c, e.dataTransfer.files[0]));
  });
  return root;
}

// A slider fires faster than a label recomposes, so paints are coalesced to one per frame.
const queued = new Set();
function schedulePaint(c) {
  if (queued.has(c)) return;
  queued.add(c);
  requestAnimationFrame(() => {
    queued.delete(c);
    paint(c);
  });
}

// The face a cart shows. A needs-logo cart has none of its own, so slot's generated label stands
// in, as it does on the device.
function faceOf(c, state) {
  if (state === 'has-label') return faceFor(c, 'card');
  if (state === 'ready') return faceFor(c, wearing(c).kind);
  if (state === 'needs-logo') return faceFor(c, 'none');
  if (state === 'unfilled') return faceFor(c, 'unfilled');
  return null;
}

function drawFace(canvas, face, platform) {
  const pixels = new Uint8ClampedArray(face.buffer, face.byteOffset, face.byteLength);
  const [boxW, boxH] = boxOf(platform);
  canvas.getContext('2d').putImageData(new ImageData(pixels, boxW, boxH), 0, 0);
}

function paint(c) {
  if (fatal) return;
  if (session && needsReal(c)) track(fetchReal(session, c));
  const state = stateOf(c);
  const { root, canvas, game } = c.el;
  root.dataset.state = state;
  let face;
  try {
    face = faceOf(c, state);
  } catch (e) {
    if (trapped(e)) return;
    throw e;
  }
  canvas.hidden = !face;
  if (face) drawFace(canvas, face, c.platform);
  game.textContent = describe(c, state);
  // The game code is read with the ROM, after the card is built.
  c.el.print.textContent = smallPrint(c.platform, c.code, c.tags);
  updateWriteBar();
  updateFillButton();
  if (editor.current() === c) editor.refresh();
}

// A game named by hand is dressed exactly as a matched one, so its logo, box art hue and the
// clash rule all apply. The CRC keeps whatever it said; the name is what the label follows.
function chooseGame(c, name) {
  if (c.game === name && !c.rejected) return;
  c.game = name;
  c.rejected = false;
  c.logoBytes = null;
  c.looking = true;
  track(redress(session, c));
}

async function redress(s, c) {
  paint(c);
  try {
    await dress(s, c);
  } catch (e) {
    if (!trapped(e)) throw e;
  }
  if (s !== session) return;
  c.looking = false;
  paint(c);
}

const choiceOf = (c) => {
  const [outline, colour, finish] = (c.shell || auto_shell(c.platform, c.code, c.head)).split(' ');
  return { outline, colour, finish };
};

async function readLogo(file) {
  if (!file) return null;
  let bytes = null;
  try {
    bytes = new Uint8Array(await file.arrayBuffer());
    if (fatal) return null;
    if (!readable(bytes)) bytes = null;
  } catch (e) {
    if (trapped(e)) return null;
    bytes = null;
  }
  if (!bytes) banner(`${file.name} is not a PNG this studio can read`);
  return bytes;
}

// A logo of the person's own decides which way its ground goes the same way a fetched one does.
function useCustomLogo(c, bytes) {
  c.customLogoBytes = bytes;
  c.customLogoLuma = logo_luma(bytes);
  c.choice = 'customLogo';
  c.byHand = true;
  if (!c.userHue) c.deep = null;
}

function useCustomLabel(c, bytes) {
  c.customLabelBytes = bytes;
  c.choice = 'customLabel';
  c.byHand = true;
}

function choose(c, kind) {
  c.choice = kind;
  c.byHand = kind !== 'card';
  if (!c.userHue) c.deep = null;
  if (kind !== 'card' && !c.dressed) ensureTiles(c);
  else if (needsReal(c)) track(fetchReal(session, c));
}

function ensureTiles(c) {
  if (c.error) return;
  if (!c.dressed && !c.looking) {
    c.looking = true;
    track(lookUp(session, c).then(() => c.dressed && ensureTiles(c)));
    return;
  }
  if (c.dressed && realPathOf(c) && !realOf(c)) track(fetchReal(session, c));
}

function fetchReal(s, c) {
  const path = realPathOf(c);
  if (!path) return Promise.resolve();
  if (realOf(c)) return realOf(c).pending;
  const real = { path, bytes: null, failed: false, pending: null };
  c.real = real;
  real.pending = (async () => {
    let bytes = null;
    try {
      bytes = await limit(async () => (s === session && !fatal ? fetchArt(ART_BASE, path) : null));
      if (bytes && !readable(bytes)) bytes = null;
    } catch (e) {
      if (trapped(e)) return;
      bytes = null;
    }
    if (s !== session || fatal) return;
    if (bytes) real.bytes = bytes;
    else real.failed = true;
    paint(c);
  })();
  return real.pending;
}

function retryReal(c) {
  if (realOf(c)?.failed) c.real = null;
}

const needsReal = (c) => wearing(c).kind === 'real' && !realOf(c);

async function takeLogo(c, file) {
  const bytes = await readLogo(file);
  if (bytes && commit(c, () => useCustomLogo(c, bytes))) changed(c);
}

function loading(text) {
  document.body.classList.toggle('loading', !!text);
  $('loader').hidden = !text;
  if (text) $('loader-text').textContent = text;
}

function progress(done, total, stem, verb = 'Reading') {
  if (document.body.classList.contains('loading') && stem) loading(`${verb} ${done + 1} of ${total}: ${stem}`);
  $('progress').hidden = done >= total;
  $('bar').max = total;
  $('bar').value = done;
  $('progress-text').textContent = stem ? `${verb} ${done + 1} of ${total}: ${stem}` : '';
}

function banner(text, retry) {
  const b = $('banner');
  b.replaceChildren(Object.assign(document.createElement('span'), { className: 'message', textContent: text }));
  const key = (label, click, extra = '') => {
    const button = Object.assign(document.createElement('button'), {
      type: 'button',
      className: `key dark ${extra}`.trim(),
      textContent: label,
    });
    button.addEventListener('click', () => click(button));
    b.append(' ', button);
  };
  if (retry) key('Retry', retry);
  if (hasLog()) {
    key('Copy log', async (button) => {
      try {
        await navigator.clipboard.writeText(logText(navigator.userAgent));
        button.textContent = 'Copied';
      } catch (e) {
        console.error('clipboard', e);
        button.textContent = 'Couldn’t copy';
      }
    }, 'copy-log');
  }
  b.hidden = false;
}

async function open(source) {
  if (fatal) {
    banner(FATAL);
    return;
  }
  editor.close();
  loading(null);
  session = { source, carts: source.carts.map(newCart) };
  $('banner').hidden = true;
  $('eject').hidden = true;
  // A card opened mid-scan replaces identify()'s loop before it reaches progress(total, total, ''),
  // so the old bar has to be cleared here rather than left for a call that may never come.
  $('progress').hidden = true;
  $('progress-text').textContent = '';
  $('grid').replaceChildren(...session.carts.map(buildCard));
  session.carts.forEach(paint);
  showPlatform(platformsOf(session)[0] ?? PLATFORMS[0]);
  // The card is chosen, so the chooser goes: the write bar sits in its place, once there is
  // something to write. updateWriteBar, reached through paint, is what shows it.
  $('pick').hidden = true;
  $('pick-files-label').hidden = true;
  $('write-bar').hidden = true;
  $('fill-open').hidden = true;
  if (session.carts.length === 0) {
    banner('There are no .gba files in that card’s Games folder.');
    return;
  }
  loading('Reading the card…');
  track(identify(session));
}

async function crcOf(file) {
  const crc = new Crc32();
  const reader = file.stream().getReader();
  for (;;) {
    const { done, value } = await reader.read();
    if (done) return crc.finish();
    crc.update(value);
  }
}

async function identify(s) {
  const text = { system: '', labels: '' };
  const failed = [];
  try {
    text.system = await s.source.systemShells();
  } catch (e) {
    logError('Config/cart_shell.txt', e);
    failed.push(e);
  }
  try {
    text.labels = await s.source.labelShells();
  } catch (e) {
    logError(SHELL_FILE, e);
    s.shellReadFailed = true;
    failed.push(e);
  }
  if (s !== session || fatal) return;
  if (failed.length) banner(failed.find((e) => e.name === 'OldShellFile')?.message ?? 'The card’s shells couldn’t be read.');
  s.shellText = text.labels;
  const system = cart_shells(text.system);
  s.systemShells = new Set(system.filter((_, i) => i % 2 === 0).map((stem) => stem.normalize('NFC')));
  const pairs = layered_cart_shells(text.system, text.labels);
  const chosen = new Map();
  for (let i = 0; i < pairs.length; i += 2) chosen.set(pairs[i].normalize('NFC'), pairs[i + 1]);
  for (const c of s.carts) c.shell = c.shellOnCard = chosen.get(c.stem.normalize('NFC')) ?? '';

  const total = s.carts.length;
  for (const [i, c] of s.carts.entries()) {
    if (s !== session || fatal) return;
    progress(i, total, c.stem);
    try {
      const file = await c.file();
      const head = new Uint8Array(await file.slice(0, HEAD).arrayBuffer());
      // Only a GBA rom carries a game code. On a Game Boy cart 0xAC is inside the RST vectors,
      // so a code read from there is opcode bytes dressed up as one.
      c.code = c.platform === 'GBA' ? header_code(head) : '';
      if (c.platform !== 'GBA') c.head = head;
      const label = s.source.labels.get(labelKey(c.platform, c.stem));
      if (label) {
        const bytes = new Uint8Array(await (await label()).arrayBuffer());
        c.existingPng = bytes;
        c.choice = 'card';
        c.existing = existing_face(bytes, c.platform, c.code, c.head, c.shell, c.stem);
        c.looking = false;
      } else {
        c.crc = await crcOf(file);
      }
    } catch (e) {
      if (trapped(e)) return;
      c.error = `Could not read this ROM: ${e.message}`;
    }
    paint(c);
  }
  // A card opened during the last read has its own bar up by now.
  if (s !== session) return;
  progress(total, total, '');
  loading('Looking up the games…');
  await match(s);
  if (s === session) loading(null);
}

async function match(s) {
  // One database per platform, fetched only for the platforms this card actually holds: a card
  // of nothing but GBA carts never pays for the two Game Boy databases.
  for (const platform of new Set(s.carts.map((c) => c.platform))) {
    if (dats.has(platform)) continue;
    try {
      dats.set(platform, Dat.parse(await fetchDat(platform)));
    } catch (e) {
      if (trapped(e) || s !== session || fatal) return;
      s.datFailed = true;
      banner(
        `The ${platform} database didn’t load (${e.message}), so carts can’t be matched yet. Logos can still be dropped in by hand.`,
        () => track(match(s)),
      );
      for (const c of s.carts) {
        if (c.looking) {
          c.looking = false;
          paint(c);
        }
      }
      return;
    }
  }
  if (s !== session || fatal) return;
  if (s.datFailed) {
    s.datFailed = false;
    $('banner').hidden = true;
  }
  const pending = s.carts.filter((c) => c.crc !== null && !c.game && !c.existing && !c.error);
  await Promise.all(pending.map((c) => art(s, c)));
  if (s === session && !fatal && s.fill == null) await fillOrAsk(s);
}

async function art(s, c) {
  c.looking = true;
  paint(c);
  try {
    c.game = dats.get(c.platform)?.game_for(c.crc) ?? null;
    await dress(s, c);
  } catch (e) {
    if (trapped(e)) return;
    throw e;
  }
  c.looking = false;
  paint(c);
}

// Where the set files this cart's art: under its own checksum while its game is the one that
// checksum names, else under the first dump of the game chosen for it that the set has.
function artKey(c) {
  const own = c.crc === null ? null : crcHex(c.crc);
  const db = dats.get(c.platform);
  if (!c.game || !db || (c.crc !== null && db.game_for(c.crc) === c.game)) return own;
  return Array.from(db.crcs_for(c.game), crcHex).find((k) => artIndex?.[k]) ?? null;
}

// Fetch what `c.game` names and let it decide the cart's hue. The Game row re-runs this for a game
// chosen by hand, so a chosen match is dressed exactly the way a matched one is.
async function dress(s, c) {
  // A set of our own is found by checksum, so a cart no database has a name for can still be
  // dressed from it. libretro is found by name, so those requests only happen when there is one.
  const name = c.game ? thumbnail_name(c.game) : null;
  const key = artKey(c);
  c.look = parseLook(key && artIndex?.[key]?.look);
  // The queue is shared, so a card replaced while its jobs wait gives up their turns without
  // a request instead of making the new card wait behind its downloads.
  const thumb = (folder) =>
    !name
      ? Promise.resolve(null)
      : limit(async () =>
          s === session && !fatal ? fetchThumb(c.platform, folder, name, stub_target) : null,
        ).catch(noImage);
  // Logos come from our own set alone, by checksum; libretro is asked only for box art.
  const ours = (media) => {
    const path = key && artIndex?.[key]?.[media];
    if (!path) return Promise.resolve(null);
    return limit(async () =>
      s === session && !fatal ? fetchArt(ART_BASE, path) : null,
    ).catch(noImage);
  };
  // The ground's colour is read off the box art, so that request starts now and is waited on
  // after the logo.
  const boxJob = thumb('Named_Boxarts');
  if (!c.logoBytes) {
    const logo = await ours('wheel');
    if (s !== session || fatal) return;
    c.logoBytes = logo && readable(logo) ? logo : null;
    // How bright the logo is decides which way its ground goes, so it is measured with the logo.
    c.logoLuma = c.logoBytes ? logo_luma(c.logoBytes) : 255;
  }
  const box = await boxJob;
  if (s !== session || fatal) return;
  c.boxHue = box ? (box_hue(box) ?? null) : null;
  if (!c.userHue) c.deep = null;
  c.dressed = true;
  if (needsReal(c)) await fetchReal(s, c);
  if (editor.current() === c) ensureTiles(c);
}

// The platforms this card actually holds, in the studio's shelf order.
const platformsOf = (s) => PLATFORMS.filter((p) => s.carts.some((c) => c.platform === p));

// A tab per platform on the card, with what it holds. One platform is no choice at all, so a
// card of nothing but GBA carts shows no switcher and reads exactly as it did before.
const PLATFORM_NAMES = { GBA: 'Game Boy Advance', GB: 'Game Boy', GBC: 'Game Boy Color' };

function renderTabs() {
  const s = session;
  const present = platformsOf(s);
  $('tabs').hidden = present.length < 2;
  $('tabs').replaceChildren(
    ...present.map((p) => {
      const n = s.carts.filter((c) => c.platform === p).length;
      const tab = Object.assign(document.createElement('button'), { type: 'button', className: 'shelf-tab' });
      tab.append(PLATFORM_NAMES[p] ?? p, Object.assign(document.createElement('span'), { className: 'n', textContent: n }));
      tab.setAttribute('aria-pressed', String(p === s.platform));
      tab.addEventListener('click', () => showPlatform(p));
      return tab;
    }),
  );
}

// Switching hides the other shelves rather than rebuilding the grid: a cart keeps its canvas, its
// chosen colour and any logo dropped on it, so coming back to a tab finds it as it was left.
function showPlatform(p) {
  editor.close();
  session.platform = p;
  for (const c of session.carts) c.el.root.hidden = c.platform !== p;
  renderTabs();
}

function updateWriteBar() {
  if (!session) return;
  const n = readyCarts().length + shellChanges().length;
  $('write-bar').hidden = n === 0 && !writing && $('eject').hidden;
  // Nothing to write means no button at all.
  $('write').hidden = n === 0 && !writing;
  $('write').disabled = n === 0 || writing || fatal;
  $('write').textContent = session.source.direct ? 'Write to card' : 'Download ZIP';
}

function writeProgress(done, total) {
  $('write').classList.toggle('busy', total > 0);
  $('write').style.setProperty('--done', total > 0 ? String(done / total) : '0');
}

function updateFillButton() {
  if (!session) return;
  const counts = fillCounts(session.carts.map(view));
  $('fill-open').hidden = counts.unlabelled === 0 || (session.fill != null && autoFill(counts) !== null);
}

async function fillOrAsk(s) {
  const counts = fillCounts(s.carts.map(view));
  if (counts.unlabelled === 0 || s !== session || fatal) return;
  const auto = autoFill(counts);
  if (auto) answerFill(auto);
  else await askFill(s);
}

async function askFill(s) {
  const counts = fillCounts(s.carts.map(view));
  if (counts.unlabelled === 0 || s !== session || fatal) return;
  const { title, question } = fillText(counts);
  $('fill-title').textContent = title;
  $('fill-question').textContent = question;
  const sample =
    s.carts.find((c) => !c.existingPng && view(c).hasReal && view(c).hasLogo) ??
    s.carts.find((c) => !c.existingPng && (view(c).hasReal || view(c).hasLogo));
  if (sample) await fetchReal(s, sample);
  if (s !== session || fatal) return;
  for (const kind of ['real', 'logo']) {
    const canvas = $(`fill-${kind}`).querySelector('canvas');
    let face = null;
    try {
      face = sample ? faceFor(sample, kind) : null;
    } catch (e) {
      if (trapped(e)) return;
      throw e;
    }
    canvas.hidden = !face;
    if (face) {
      [canvas.width, canvas.height] = boxOf(sample.platform);
      drawFace(canvas, face, sample.platform);
    }
  }
  if (!$('fill').open) {
    $('fill').showModal();
    $('fill').focus();
  }
}

function answerFill(fill) {
  const s = session;
  if (!s) return;
  s.fill = fill;
  for (const c of s.carts) {
    const v = view(c);
    if (!applyFill([v], fill)) continue;
    c.choice = v.choice;
    amend(c, (snap) => {
      if (snap.choice == null) snap.choice = fill;
    });
  }
  $('fill').close();
  for (const c of s.carts) {
    if (!c.userHue) c.deep = null;
    paint(c);
    if (needsReal(c)) track(fetchReal(s, c));
  }
  updateFillButton();
}

function download(blob, name) {
  const a = Object.assign(document.createElement('a'), { href: URL.createObjectURL(blob), download: name });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(a.href), 60_000);
}

async function writeLabels() {
  const s = session;
  const carts = readyCarts();
  const shells = shellChanges();
  if ((!carts.length && !shells.length) || writing || fatal) return;
  writing = true;
  $('eject').hidden = true;
  updateWriteBar();
  writeProgress(0, carts.length + shells.length);
  try {
    if (s.source.direct) {
      const count = { written: 0, skipped: 0, failed: 0 };
      for (const c of carts) {
        if (fatal) return;
        let face = null;
        let png = null;
        try {
          [png, face] = withCartLabel(c, (label) => [
            label.png(),
            label.face(c.platform, c.code, c.head, c.shell, c.stem),
          ]);
          c.result = await s.source.write(c.platform, c.stem, png, !!c.existingPng);
        } catch (e) {
          if (trapped(e)) return;
          logError(c.stem, e);
          c.result = 'failed';
        }
        if (c.result === 'written') {
          c.existing = face;
          c.existingPng = png;
          c.choice = 'card';
          c.byHand = false;
        }
        count[c.result]++;
        writeProgress(count.written + count.skipped + count.failed, carts.length + shells.length);
        paint(c);
      }
      let shellNote = '';
      if (shells.length) {
        if (s.shellReadFailed) {
          shellNote = 'Shells not written: the card’s shells couldn’t be read.';
        } else {
          // Captured before the write's await, so a change made during it is not marked written.
          const values = shells.map((c) => c.shell);
          try {
            const text = mergedShells(s, shells, values);
            await s.source.writeShells(text);
            s.shellText = text;
            shells.forEach((c, i) => (c.shellOnCard = values[i]));
            count.shells = shells.length;
          } catch (e) {
            if (trapped(e)) return;
            logError(SHELL_FILE, e);
            shellNote = 'Writing the shells failed.';
          }
        }
      }
      // One that didn't, all of it, says so. Not over a card opened while this one was writing.
      if (s === session) {
        $('eject').hidden = !(count.written || count.shells);
        const plural = (k, one, many) => `${k} ${k === 1 ? one : many}`;
        const problems = [
          count.failed && `${plural(count.failed, 'label', 'labels')} couldn’t be written.`,
          count.skipped && `${plural(count.skipped, 'label was', 'labels were')} skipped: one turned up on the card meanwhile.`,
          shellNote,
        ].filter(Boolean);
        if (problems.length) banner(problems.join(' '));
      }
    } else {
      const zip = new Zip();
      // Each entry is taken out of WASM memory as soon as it is added, so the card's labels pile
      // up here, where memory is given back, and not in the module.
      const parts = [];
      const written = new Set();
      const carried = s.source.labelFiles;
      const total = carts.length + shells.length + carried.length;
      for (const [i, c] of carts.entries()) {
        writeProgress(i + 1, total);
        const path = `Labels/${c.platform}/${c.stem}.png`;
        zip.add(path, withCartLabel(c, (label) => label.png()));
        parts.push(zip.take());
        written.add(pathKey(path));
        // Packing a few hundred carts is seconds of synchronous work with nothing on screen, so
        // give the tab a turn to paint the bar and take input.
        await new Promise((r) => setTimeout(r, 0));
        // A trap elsewhere, or a card replacing this one, can happen while this awaits.
        if (fatal || s !== session) return;
      }
      let shellNote = '';
      if (shells.length) {
        if (s.shellReadFailed) {
          shellNote = 'Shells not written: the card’s shells couldn’t be read.';
        } else {
          try {
            const text = mergedShells(s, shells, shells.map((c) => c.shell));
            zip.add(SHELL_FILE, new TextEncoder().encode(text));
            parts.push(zip.take());
            written.add(pathKey(SHELL_FILE));
          } catch (e) {
            if (trapped(e)) return;
            logError(SHELL_FILE, e);
            shellNote = 'Writing the shells failed.';
          }
        }
      }
      let unread = 0;
      for (const [i, f] of carried.entries()) {
        writeProgress(carts.length + shells.length + i + 1, total);
        if (written.has(pathKey(f.path))) continue;
        try {
          zip.add(f.path, new Uint8Array(await (await f.file()).arrayBuffer()));
          parts.push(zip.take());
        } catch (e) {
          if (trapped(e)) return;
          logError(f.path, e);
          unread++;
        }
        if (fatal || s !== session) return;
      }
      parts.push(zip.finish());
      download(new Blob(parts, { type: 'application/zip' }), 'labels.zip');
      const unreadNote =
        unread > 0 &&
        `${unread === 1 ? '1 file' : `${unread} files`} in the card’s Labels folder couldn’t be read and aren’t in the zip. Merge it into the card’s Labels folder rather than replacing it.`;
      const notes = [shellNote, unreadNote].filter(Boolean);
      if (s === session && notes.length) banner(notes.join(' '));
    }
  } catch (e) {
    if (!trapped(e)) throw e;
  } finally {
    writing = false;
    writeProgress(0, 0);
    updateWriteBar();
  }
}

const canLabel = (c) => !c.error && !c.looking;

const SMALL = new Set(['in', 'of', 'the', 'and']);
const titled = (name) =>
  name
    .split(' ')
    .map((w, i) => (i > 0 && SMALL.has(w) ? w : w.charAt(0).toUpperCase() + w.slice(1)))
    .join(' ');

let presetList = null;
const presets = () =>
  (presetList ??= shell_presets().map((entry) => {
    const [name, value] = entry.split('\t');
    return { name: titled(name), value };
  }));

function shellOf(c) {
  const { outline, colour, finish } = choiceOf(c);
  const preset = presets().find(({ value }) => value.endsWith(` ${colour} ${finish}`));
  return { outline, colour, finish, auto: !c.shell, name: preset?.name ?? `#${colour}` };
}

async function lookUp(s, c) {
  paint(c);
  try {
    if (c.crc === null) c.crc = await crcOf(await c.file());
    if (!dats.has(c.platform)) dats.set(c.platform, Dat.parse(await fetchDat(c.platform)));
    if (s !== session || fatal) return;
    c.game = dats.get(c.platform).game_for(c.crc) ?? null;
    await dress(s, c);
  } catch (e) {
    if (trapped(e)) return;
    logError(c.stem, e);
  }
  if (s !== session) return;
  c.looking = false;
  paint(c);
}

function setShell(c, next) {
  c.shell = next ? `${next.outline} ${next.colour} ${next.finish}` : '';
}

function setBackground(c, rgb) {
  c.deep = rgb;
  c.userHue = !!rgb;
}

// A CRC can be right about the bytes and wrong about the game: a ROM renamed to another's name.
function noMatch(c) {
  c.rejected = true;
  c.userHue = false;
  c.deep = null;
}

// Painted at once, so the editor's buttons are right before the next click; only a drag's
// follow-on inputs wait for a frame.
function changed(c, soon = false) {
  try {
    if (c.existingPng) c.existing = existing_face(c.existingPng, c.platform, c.code, c.head, c.shell, c.stem);
  } catch (e) {
    if (trapped(e)) return;
    throw e;
  }
  if (soon) schedulePaint(c);
  else paint(c);
}

const editor = createEditor({
  name: (c) => clean_label(c.stem),
  box: boxOf,
  draw: drawFace,
  face: (c) => {
    try {
      return faceOf(c, stateOf(c));
    } catch (e) {
      if (trapped(e)) return null;
      throw e;
    }
  },
  hex,
  fromHex,
  matched: (c) => !!c.game && !c.rejected,
  canLabel,
  canBackground: (c) => stateOf(c) === 'ready' && ['logo', 'customLogo'].includes(wearing(c).kind),
  wearing,
  available: (c, kind) => available(view(c), kind),
  realFailed: (c) => !!realOf(c)?.failed,
  retryReal,
  fill: () => session?.fill ?? null,
  tileFace: (c, kind) => {
    try {
      return faceFor(c, kind);
    } catch (e) {
      if (trapped(e)) return null;
      throw e;
    }
  },
  choose,
  useCustomLogo,
  useCustomLabel,
  ensureTiles,
  canShell: (c) => !c.looking && ['ready', 'has-label', 'needs-logo', 'unfilled'].includes(stateOf(c)) && shell_key_ok(c.stem),
  search: (c, query) => dats.get(c.platform)?.search(query, 30) ?? null,
  chooseGame,
  noMatch,
  readLogo,
  background: (c) => ({ rgb: c.deep ?? baseDeep(c), auto: !c.userHue }),
  setBackground,
  shellOf,
  setShell,
  presets,
  changed,
});

async function start() {
  await init();
  for (const platform of PLATFORMS) faceBox.set(platform, Array.from(cart_size(platform)));
  // A set of our own, if the address named one. Not having it is not a failure: every cart falls
  // back to libretro, which is all the page had before there was a set to point at.
  if (ART_BASE) {
    try {
      artIndex = await fetchIndex(ART_BASE);
      console.info(`art set: ${Object.keys(artIndex).length} checksums from ${ART_BASE}`);
    } catch (e) {
      banner(`The art set at ${ART_BASE} didn’t load (${e.message}), so carts get slot’s own labels.`);
    }
  }
  const direct = 'showDirectoryPicker' in window;
  $('pick').hidden = !direct;
  $('pick-files-label').hidden = direct;

  $('pick').addEventListener('click', async () => {
    let root;
    try {
      root = await window.showDirectoryPicker({ id: 'slot-card', mode: 'readwrite' });
    } catch (e) {
      if (e.name !== 'AbortError') banner(e.message);
      return;
    }
    try {
      await open(await fromDirectory(root));
    } catch (e) {
      banner(e.message);
    }
  });
  $('pick-files').addEventListener('change', async (e) => {
    // Emptied at once, or choosing the same card again fires no change. The FileList belongs to
    // the input, so the files are copied out of it first.
    const files = [...e.target.files];
    e.target.value = '';
    try {
      await open(fromFiles(files));
    } catch (err) {
      banner(err.message);
    }
  });
  $('write').addEventListener('click', () => track(writeLabels()));
  $('fill-open').addEventListener('click', () => track(askFill(session)));
  for (const kind of ['real', 'logo']) $(`fill-${kind}`).addEventListener('click', () => answerFill(kind));
  $('fill').addEventListener('cancel', (e) => {
    if (session?.fill == null) e.preventDefault();
  });

  // slot's masthead: a row side by side, a list behind a button on a phone.
  const menu = (open) => {
    $('mast-links').classList.toggle('is-open', open);
    $('menu-btn').setAttribute('aria-expanded', String(open));
  };
  $('menu-btn').addEventListener('click', (e) => {
    e.stopPropagation();
    menu(!$('mast-links').classList.contains('is-open'));
  });
  document.addEventListener('click', (e) => {
    if (!$('menu-btn').contains(e.target)) menu(false);
  });


  // For tools/verify.py, which has no native picker to click. Never set on the published site.
  if (['localhost', '127.0.0.1'].includes(location.hostname)) {
    window.__studio = {
      openFiles: (files) => open(fromFiles(files)),
      openDirectory: async (root) => open(await fromDirectory(root)),
      idle: () =>
        busy === 0 && session !== null && session.carts.every((c) => stateOf(c) !== 'looking'),
      states: () =>
        session
          ? session.carts.map((c) => ({
              platform: c.platform,
              stem: c.stem,
              state: stateOf(c),
              game: c.game,
              deep: hex(c.deep ?? baseDeep(c)),
              band: !!bandOf(c),
              logo: (wearing(c).kind === 'customLogo' ? c.customLogoBytes : c.logoBytes)?.length ?? 0,
              wears: wearing(c).kind,
              why: wearing(c).why,
            }))
          : [],
      shells: () => (session ? session.carts.map((c) => ({ stem: c.stem, shell: c.shell })) : []),
      shellPresets: () => shell_presets(),
      editing: () => editor.current()?.stem ?? null,
      artBase: () => ART_BASE,
      fill: () => session?.fill ?? null,
    };
  }
  document.body.dataset.ready = 'true';
}

$('build').append(
  BUILD === 'dev'
    ? 'dev'
    : Object.assign(document.createElement('a'), {
        href: `https://github.com/BrandonKowalski/slot-cart-studio/commit/${BUILD}`,
        textContent: BUILD.slice(0, 7),
      }),
);
start();
