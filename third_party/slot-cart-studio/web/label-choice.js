export const KINDS = ['card', 'real', 'logo', 'customLogo', 'customLabel'];

const OTHER = { real: 'logo', logo: 'real' };

export function available(c, kind) {
  if (kind === 'card') return c.onCard;
  if (kind === 'real') return c.hasReal;
  if (kind === 'logo') return c.hasLogo;
  return true;
}

export function wears(c) {
  const why = c.byHand ? 'byHand' : 'fill';
  switch (c.choice) {
    case 'card':
      return { kind: 'card', why: 'card' };
    case 'customLogo':
      return c.customLogoBytes ? { kind: 'customLogo', why } : { kind: 'none', why: 'nothing' };
    case 'customLabel':
      return c.customLabelBytes ? { kind: 'customLabel', why } : { kind: 'none', why: 'nothing' };
    case 'real':
    case 'logo':
      if (available(c, c.choice)) return { kind: c.choice, why };
      if (available(c, OTHER[c.choice])) return { kind: OTHER[c.choice], why: 'fallback' };
      return { kind: 'none', why: 'nothing' };
    default:
      return { kind: 'none', why: 'unfilled' };
  }
}

const fillable = (c) => !c.onCard && !c.error && !c.byHand && c.choice == null;

export function applyFill(carts, fill) {
  let n = 0;
  for (const c of carts) {
    if (!fillable(c)) continue;
    c.choice = fill;
    n++;
  }
  return n;
}

export function fillCounts(carts) {
  const counts = { unlabelled: 0, real: 0, logo: 0, neither: 0 };
  for (const c of carts) {
    if (!fillable(c)) continue;
    counts.unlabelled++;
    if (c.hasReal) counts.real++;
    if (c.hasLogo) counts.logo++;
    if (!c.hasReal && !c.hasLogo) counts.neither++;
  }
  return counts;
}

export function fillText({ unlabelled }) {
  return unlabelled === 1
    ? { title: '1 cart has no label yet', question: 'How should the studio fill it?' }
    : { title: `${unlabelled} carts have no label yet`, question: 'How should the studio fill them?' };
}

export function autoFill({ real, logo }) {
  if (real > 0 && logo > 0) return null;
  return real > 0 ? 'real' : 'logo';
}
