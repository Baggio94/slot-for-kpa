// One panel for everything about one cart. It keeps no cart state: every change goes through the
// studio's api, one history step at a time.
import { canRedo, canRevert, canUndo, commit, redo, revert, undo, visit, visited } from './history.js';
import { arrange } from './palette.js';

const $ = (id) => document.getElementById(id);
const OPENERS = { 'ed-games': 'ed-game', 'ed-bg-pal': 'ed-bg', 'ed-shell-pal': 'ed-shell' };

export function createEditor(api) {
  const panel = $('editor');
  let cart = null;
  let built = false;
  // A drag through a colour picker is one step: the first input records it, the rest follow.
  let dragging = false;

  function change(fn, c = cart) {
    dragging = false;
    if (c && commit(c, () => fn(c))) api.changed(c);
  }

  function drag(fn) {
    const c = cart;
    if (!c) return;
    if (dragging) {
      fn(c);
      api.changed(c, true);
    } else if ((dragging = commit(c, () => fn(c)))) {
      api.changed(c);
    }
  }

  function step(fn) {
    dragging = false;
    const c = cart;
    if (c && fn(c)) api.changed(c);
  }

  // Hiding or disabling the focused control drops focus to the page, where ⌘Z never reaches the panel.
  function keepFocus(fallback = panel, lost = false) {
    const at = document.activeElement;
    if (cart && (lost || (panel.contains(at) && (at.disabled || at.closest('[hidden]'))))) {
      (fallback.disabled ? panel : fallback).focus({ preventScroll: true });
    }
  }

  function closePops() {
    dragging = false;
    $('ed-tip').hidden = true;
    for (const [pop, opener] of Object.entries(OPENERS)) {
      const had = $(pop).contains(document.activeElement);
      $(pop).hidden = true;
      $(opener).setAttribute('aria-expanded', 'false');
      if (had) keepFocus($(opener), true);
    }
  }

  function toggle(pop) {
    const opening = $(pop).hidden;
    closePops();
    $(pop).hidden = !opening;
    $(OPENERS[pop]).setAttribute('aria-expanded', String(opening));
    return opening;
  }

  const press = (group, value) => {
    for (const b of $(group).querySelectorAll('button')) b.setAttribute('aria-pressed', String(b.dataset.value === value));
  };
  const enable = (group, on) => {
    for (const b of $(group).querySelectorAll('button')) b.disabled = !on;
  };

  // Background and Shell share one palette: the real plastics, by colour.
  function build() {
    if (built) return;
    built = true;
    const { neutrals, colours } = arrange(api.presets());
    const swatch = ({ name, value }) => {
      const b = Object.assign(document.createElement('button'), { type: 'button' });
      b.setAttribute('aria-label', name);
      b.dataset.value = value;
      b.dataset.finish = value.split(' ')[2];
      b.style.backgroundColor = `#${value.split(' ')[1]}`;
      return b;
    };
    for (const id of ['ed-bg-presets', 'ed-shell-presets']) {
      const gap = Object.assign(document.createElement('span'), { className: 'pal-break' });
      $(id).replaceChildren(...neutrals.map(swatch), gap, ...colours.map(swatch));
    }
  }

  const presetNamed = (hex) => api.presets().find(({ value }) => `#${value.split(' ')[1]}` === hex)?.name ?? hex;

  function showGames() {
    const c = cart;
    const item = (text, pick, className = '') => {
      const b = Object.assign(document.createElement('button'), { type: 'button', textContent: text, className });
      b.addEventListener('click', () => {
        closePops();
        pick();
      });
      const li = document.createElement('li');
      li.append(b);
      return li;
    };
    $('ed-hits').replaceChildren(
      ...(api.matched(c) ? [item('No match', () => change(api.noMatch), 'none')] : []),
      ...(api.search(c, $('ed-query').value) ?? []).map((name) =>
        item(name, () => change((c) => api.chooseGame(c, name))),
      ),
    );
  }

  function renderTiles(c) {
    const wearing = api.wearing(c);
    const fill = api.fill();
    const can = api.canLabel(c);
    for (const tile of $('ed-tiles').querySelectorAll('.tile')) {
      const kind = tile.dataset.kind;
      const ok = api.available(c, kind);
      const retry = kind === 'real' && api.realFailed(c);
      tile.hidden = kind === 'card' && !ok;
      tile.disabled = !can || (!ok && !retry && kind !== 'customLogo' && kind !== 'customLabel');
      tile.setAttribute('aria-pressed', String(wearing.kind === kind));
      const badge = tile.querySelector('.badge.default');
      if (badge) badge.hidden = fill !== kind;
      const small = tile.querySelector('small');
      if (small && (kind === 'real' || kind === 'logo')) small.textContent = retry ? 'Couldn’t load. Click to try again' : ok ? '' : 'None for this game';
      const hasFile = kind === 'customLogo' ? !!c.customLogoBytes : kind === 'customLabel' ? !!c.customLabelBytes : true;
      tile.classList.toggle('has-file', hasFile);
      if (small && (kind === 'customLogo' || kind === 'customLabel')) small.textContent = hasFile ? 'Click again to use another file' : '';
      const canvas = tile.querySelector('canvas');
      const face = ok && hasFile ? api.tileFace(c, kind) : null;
      canvas.hidden = !face;
      if (face) {
        [canvas.width, canvas.height] = api.box(c.platform);
        api.draw(canvas, face, c.platform);
      }
    }
  }

  function refresh() {
    const c = cart;
    if (!c) return;
    if (!visited(c)) visit(c);
    $('ed-name').textContent = api.name(c);
    const face = api.face(c);
    $('ed-face').hidden = !face;
    if (face) api.draw($('ed-face'), face, c.platform);

    const label = api.canLabel(c);
    $('ed-game').disabled = !label;
    $('ed-game').querySelector('span').textContent = api.matched(c) ? c.game : 'Choose a game';
    renderTiles(c);

    const bg = api.background(c);
    $('ed-bg').closest('.prop').hidden = !api.canBackground(c);
    $('ed-bg').querySelector('i').style.background = api.hex(bg.rgb);
    $('ed-bg').querySelector('span').textContent = bg.auto ? 'Automatic' : presetNamed(api.hex(bg.rgb));
    for (const b of $('ed-bg-presets').querySelectorAll('button')) {
      b.setAttribute('aria-pressed', String(!bg.auto && `#${b.dataset.value.split(' ')[1]}` === api.hex(bg.rgb)));
    }
    $('ed-bg-auto').setAttribute('aria-pressed', String(bg.auto));
    $('ed-bg-custom').value = api.hex(bg.rgb);

    const shell = api.shellOf(c);
    const shells = api.canShell(c);
    $('ed-shell').disabled = !shells;
    $('ed-shell').querySelector('i').style.background = `#${shell.colour}`;
    $('ed-shell').querySelector('span').textContent = shell.auto ? 'Automatic' : shell.name;
    for (const b of $('ed-shell-presets').querySelectorAll('button')) {
      const [, colour, finish] = b.dataset.value.split(' ');
      b.setAttribute('aria-pressed', String(!shell.auto && colour === shell.colour && finish === shell.finish));
    }
    $('ed-shell-auto').setAttribute('aria-pressed', String(shell.auto));
    $('ed-shell-custom').value = `#${shell.colour}`;
    press('ed-finish', shell.finish);
    enable('ed-finish', shells && !shell.auto);
    $('ed-outline-row').hidden = c.platform === 'GBA';
    press('ed-outline', shell.outline);
    enable('ed-outline', shells);

    if (Object.entries(OPENERS).some(([pop, opener]) => !$(pop).hidden && $(opener).disabled)) closePops();
    $('ed-undo').disabled = !canUndo(c);
    $('ed-redo').disabled = !canRedo(c);
    $('ed-revert').disabled = !canRevert(c);
    keepFocus();
  }

  function open(c) {
    build();
    if (c === cart) return;
    cart?.el.root.classList.remove('current');
    cart = c;
    dragging = false;
    api.ensureTiles(c);
    visit(c);
    closePops();
    c.el.root.classList.add('current');
    [$('ed-face').width, $('ed-face').height] = api.box(c.platform);
    panel.hidden = false;
    document.body.classList.add('editing');
    refresh();
    panel.focus({ preventScroll: true });
  }

  function close() {
    if (!cart) return;
    cart.el.root.classList.remove('current');
    cart = null;
    closePops();
    panel.hidden = true;
    document.body.classList.remove('editing');
  }

  // A swatch's name, at once and kept inside the window; the browser's own tooltip is slow to come.
  function tip(b) {
    const t = $('ed-tip');
    if (!b) {
      t.hidden = true;
      return;
    }
    t.textContent = b.getAttribute('aria-label');
    t.hidden = false;
    const r = b.getBoundingClientRect();
    const left = Math.min(Math.max(8, r.left + r.width / 2 - t.offsetWidth / 2), innerWidth - t.offsetWidth - 8);
    t.style.left = `${left}px`;
    t.style.top = `${Math.max(8, r.top - t.offsetHeight - 6)}px`;
  }
  const swatch = (e) => e.target.closest?.('.pal-grid button') ?? null;
  for (const id of ['ed-bg-presets', 'ed-shell-presets']) {
    $(id).addEventListener('pointerover', (e) => tip(swatch(e)));
    $(id).addEventListener('pointerout', () => tip(null));
    $(id).addEventListener('focusin', (e) => tip(swatch(e)));
    $(id).addEventListener('focusout', () => tip(null));
  }

  $('ed-close').addEventListener('click', close);

  $('ed-game').addEventListener('click', () => {
    if (!toggle('ed-games')) return;
    $('ed-query').value = api.name(cart);
    showGames();
    $('ed-query').select();
  });
  $('ed-query').addEventListener('input', showGames);

  let picking = null;
  $('ed-tiles').addEventListener('click', (e) => {
    const tile = e.target.closest('.tile');
    if (!tile || tile.disabled) return;
    const kind = tile.dataset.kind;
    const c = cart;
    const file = kind === 'customLogo' ? c.customLogoBytes : kind === 'customLabel' ? c.customLabelBytes : null;
    if ((kind === 'customLogo' || kind === 'customLabel') && (!file || c.choice === kind)) {
      picking = kind;
      $('ed-file').click();
      return;
    }
    if (kind === 'real') api.retryReal(c);
    if (kind === 'customLogo') change((c) => api.useCustomLogo(c, file));
    else if (kind === 'customLabel') change((c) => api.useCustomLabel(c, file));
    else change((c) => api.choose(c, kind));
  });
  $('ed-file').addEventListener('change', async () => {
    const c = cart;
    const kind = picking;
    const file = $('ed-file').files[0];
    $('ed-file').value = '';
    const bytes = await api.readLogo(file);
    if (!bytes || !kind) return;
    change((c) => (kind === 'customLogo' ? api.useCustomLogo(c, bytes) : api.useCustomLabel(c, bytes)), c);
  });

  $('ed-bg').addEventListener('click', () => toggle('ed-bg-pal'));
  $('ed-bg-presets').addEventListener('click', (e) => {
    const value = e.target.closest('button')?.dataset.value;
    if (!value) return;
    closePops();
    change((c) => api.setBackground(c, api.fromHex(`#${value.split(' ')[1]}`)));
  });
  $('ed-bg-auto').addEventListener('click', () => {
    closePops();
    change((c) => api.setBackground(c, null));
  });
  $('ed-bg-custom').addEventListener('input', () =>
    drag((c) => api.setBackground(c, api.fromHex($('ed-bg-custom').value))),
  );

  $('ed-shell').addEventListener('click', () => toggle('ed-shell-pal'));
  $('ed-shell-presets').addEventListener('click', (e) => {
    const value = e.target.closest('button')?.dataset.value;
    if (!value) return;
    const [, colour, finish] = value.split(' ');
    closePops();
    change((c) => api.setShell(c, { ...api.shellOf(c), colour, finish }));
  });
  $('ed-shell-auto').addEventListener('click', () => {
    closePops();
    change((c) => api.setShell(c, null));
  });
  $('ed-shell-custom').addEventListener('input', () =>
    drag((c) => api.setShell(c, { ...api.shellOf(c), colour: $('ed-shell-custom').value.slice(1) })),
  );
  for (const id of ['ed-bg-custom', 'ed-shell-custom']) {
    $(id).addEventListener('change', () => {
      dragging = false;
      closePops();
    });
  }
  $('ed-finish').addEventListener('click', (e) => {
    const finish = e.target.closest('button')?.dataset.value;
    if (finish) change((c) => api.setShell(c, { ...api.shellOf(c), finish }));
  });
  $('ed-outline').addEventListener('click', (e) => {
    const outline = e.target.closest('button')?.dataset.value;
    if (outline) change((c) => api.setShell(c, { ...api.shellOf(c), outline }));
  });

  $('ed-undo').addEventListener('click', () => step(undo));
  $('ed-redo').addEventListener('click', () => step(redo));
  $('ed-revert').addEventListener('click', () => step(revert));

  panel.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      if (Object.keys(OPENERS).some((pop) => !$(pop).hidden)) closePops();
      else close();
      e.preventDefault();
      return;
    }
    if (!(e.metaKey || e.ctrlKey) || e.target.matches('input[type="search"]')) return;
    const key = e.key.toLowerCase();
    if (key === 'z' && !e.shiftKey) step(undo);
    else if ((key === 'z' && e.shiftKey) || key === 'y') step(redo);
    else return;
    e.preventDefault();
  });

  return { open, close, refresh, current: () => cart };
}
