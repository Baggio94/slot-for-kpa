// Each cart's edits for the session. Every change the editor makes is one step, and Revert goes
// back to how the cart was when the editor came to it.
export const EDITABLE = [
  'game',
  'rejected',
  'choice',
  'byHand',
  'customLogoBytes',
  'customLogoLuma',
  'customLabelBytes',
  'logoBytes',
  'logoLuma',
  'look',
  'boxHue',
  'deep',
  'userHue',
  'shell',
];

const capture = (c) => Object.fromEntries(EDITABLE.map((k) => [k, c[k]]));
const eq = (a, b) =>
  a === b || (Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every((v, i) => v === b[i]));
const same = (a, b) => EDITABLE.every((k) => eq(a[k], b[k]));
const log = (c) => (c.history ??= { undo: [], redo: [], visit: null });

// A cart still being looked up has not settled into anything worth going back to.
export function visit(c) {
  log(c).visit = c.looking ? null : capture(c);
}

export const visited = (c) => !!c.history?.visit;

export function commit(c, change) {
  const before = capture(c);
  change();
  if (same(before, capture(c))) return false;
  const h = log(c);
  h.undo.push(before);
  h.redo = [];
  return true;
}

function step(c, from, to) {
  if (c.looking || from.length === 0) return false;
  to.push(capture(c));
  Object.assign(c, from.pop());
  return true;
}

export const undo = (c) => step(c, log(c).undo, log(c).redo);
export const redo = (c) => step(c, log(c).redo, log(c).undo);

export function revert(c) {
  const { visit: was } = log(c);
  return !c.looking && !!was && commit(c, () => Object.assign(c, was));
}

export const canUndo = (c) => !c.looking && !!c.history?.undo.length;
export const canRedo = (c) => !c.looking && !!c.history?.redo.length;
export const canRevert = (c) => !c.looking && !!c.history?.visit && !same(c.history.visit, capture(c));

export function amend(c, fix) {
  const h = log(c);
  for (const snap of [h.visit, ...h.undo, ...h.redo]) if (snap) fix(snap);
}
