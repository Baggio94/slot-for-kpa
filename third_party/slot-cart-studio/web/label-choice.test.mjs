import { test } from 'node:test';
import assert from 'node:assert/strict';
import { applyFill, autoFill, available, fillCounts, fillText, wears } from './label-choice.js';

const LOGO = new Uint8Array([1]);
const LABEL = new Uint8Array([2]);
const cart = (over = {}) => ({
  choice: null,
  byHand: false,
  onCard: false,
  hasReal: true,
  hasLogo: true,
  customLogoBytes: null,
  customLabelBytes: null,
  error: null,
  ...over,
});

test('an unlabelled cart before the fill wears nothing, waiting for it', () => {
  assert.deepEqual(wears(cart()), { kind: 'none', why: 'unfilled' });
});

test('a cart on the card keeps its label', () => {
  assert.deepEqual(wears(cart({ onCard: true, choice: 'card' })), { kind: 'card', why: 'card' });
});

test('a filled cart wears the fill when it has the art', () => {
  assert.deepEqual(wears(cart({ choice: 'real' })), { kind: 'real', why: 'fill' });
  assert.deepEqual(wears(cart({ choice: 'logo' })), { kind: 'logo', why: 'fill' });
});

test('a cart set by hand says so', () => {
  assert.deepEqual(wears(cart({ choice: 'logo', byHand: true })), { kind: 'logo', why: 'byHand' });
});

test('with no art for its style a cart falls back to the other', () => {
  assert.deepEqual(wears(cart({ choice: 'real', hasReal: false })), { kind: 'logo', why: 'fallback' });
  assert.deepEqual(wears(cart({ choice: 'logo', hasLogo: false })), { kind: 'real', why: 'fallback' });
});

test('with art for neither style a cart wears nothing', () => {
  assert.deepEqual(wears(cart({ choice: 'real', hasReal: false, hasLogo: false })), { kind: 'none', why: 'nothing' });
});

test('custom choices wear their file, and nothing without one', () => {
  assert.deepEqual(wears(cart({ choice: 'customLogo', byHand: true, customLogoBytes: LOGO })), { kind: 'customLogo', why: 'byHand' });
  assert.deepEqual(wears(cart({ choice: 'customLabel', byHand: true, customLabelBytes: LABEL })), { kind: 'customLabel', why: 'byHand' });
  assert.deepEqual(wears(cart({ choice: 'customLogo', byHand: true })), { kind: 'none', why: 'nothing' });
});

test('available says which tiles a cart can wear', () => {
  const c = cart({ hasReal: false, onCard: true, customLabelBytes: LABEL });
  assert.deepEqual(
    ['card', 'real', 'logo', 'customLogo', 'customLabel'].map((k) => available(c, k)),
    [true, false, true, true, true],
  );
  assert.equal(available(cart(), 'card'), false);
});

test('the fill only touches carts with no label yet', () => {
  const carts = [
    cart(),
    cart({ onCard: true, choice: 'card' }),
    cart({ choice: 'logo', byHand: true }),
    cart({ error: 'unreadable' }),
    cart({ choice: 'logo' }),
  ];
  assert.equal(applyFill(carts, 'real'), 1);
  assert.deepEqual(carts.map((c) => c.choice), ['real', 'card', 'logo', null, 'logo']);
});

test('the counts cover unlabelled carts only', () => {
  const carts = [
    cart(),
    cart({ hasReal: false }),
    cart({ hasReal: false, hasLogo: false }),
    cart({ onCard: true, choice: 'card' }),
    cart({ error: 'x' }),
    cart({ choice: 'real' }),
    cart({ choice: 'logo', byHand: true }),
  ];
  assert.deepEqual(fillCounts(carts), { unlabelled: 3, real: 1, logo: 2, neither: 1 });
  assert.deepEqual(fillCounts([cart({ onCard: true })]), { unlabelled: 0, real: 0, logo: 0, neither: 0 });
});

test('the prompt says how many carts, and asks how to fill them', () => {
  assert.deepEqual(fillText({ unlabelled: 7, real: 6, logo: 7, neither: 0 }), {
    title: '7 carts have no label yet',
    question: 'How should the studio fill them?',
  });
  assert.deepEqual(fillText({ unlabelled: 1, real: 0, logo: 1, neither: 0 }), {
    title: '1 cart has no label yet',
    question: 'How should the studio fill it?',
  });
});

test('the fill is only asked when both styles have art somewhere', () => {
  assert.equal(autoFill({ unlabelled: 3, real: 2, logo: 3, neither: 0 }), null);
  assert.equal(autoFill({ unlabelled: 3, real: 0, logo: 3, neither: 0 }), 'logo');
  assert.equal(autoFill({ unlabelled: 3, real: 3, logo: 0, neither: 0 }), 'real');
  assert.equal(autoFill({ unlabelled: 1, real: 0, logo: 0, neither: 1 }), 'logo');
});
