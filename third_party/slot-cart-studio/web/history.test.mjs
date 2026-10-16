import { test } from 'node:test';
import assert from 'node:assert/strict';
import { amend, canRedo, canRevert, canUndo, commit, redo, revert, undo, visit, visited } from './history.js';

const cart = (over = {}) => ({
  game: 'Tetris',
  rejected: false,
  choice: 'logo',
  byHand: false,
  customLogoBytes: null,
  customLogoLuma: 255,
  customLabelBytes: null,
  logoBytes: null,
  logoLuma: 255,
  look: null,
  boxHue: null,
  deep: null,
  userHue: false,
  shell: '',
  looking: false,
  ...over,
});

test('undo puts back what a change replaced, and redo puts it again', () => {
  const c = cart();
  assert.equal(commit(c, () => (c.shell = 'auto c2332e solid')), true);
  assert.equal(undo(c), true);
  assert.equal(c.shell, '');
  assert.equal(redo(c), true);
  assert.equal(c.shell, 'auto c2332e solid');
});

test('a change that changes nothing is no step', () => {
  const c = cart({ shell: 'auto c2332e solid' });
  assert.equal(commit(c, () => (c.shell = 'auto c2332e solid')), false);
  assert.equal(canUndo(c), false);
});

test('colours compare by value, so the same colour picked again is no step', () => {
  const c = cart({ deep: [1, 2, 3], userHue: true });
  assert.equal(commit(c, () => (c.deep = [1, 2, 3])), false);
});

test('a new change after an undo drops the redo', () => {
  const c = cart();
  commit(c, () => (c.shell = 'a'));
  undo(c);
  commit(c, () => (c.shell = 'b'));
  assert.equal(canRedo(c), false);
  assert.equal(redo(c), false);
});

test('one step covers every field a change touched', () => {
  const c = cart();
  commit(c, () => Object.assign(c, { rejected: true, deep: null, userHue: false, customLogoBytes: null }));
  commit(c, () => Object.assign(c, { deep: [9, 9, 9], userHue: true }));
  undo(c);
  assert.deepEqual([c.rejected, c.deep, c.userHue], [true, null, false]);
  undo(c);
  assert.equal(c.rejected, false);
});

test('a tile change is one step, and undo puts the previous choice and file back', () => {
  const logo = new Uint8Array([1]);
  const label = new Uint8Array([2]);
  const c = cart({ choice: 'customLogo', byHand: true, customLogoBytes: logo });
  assert.equal(commit(c, () => Object.assign(c, { choice: 'customLabel', customLabelBytes: label })), true);
  assert.equal(undo(c), true);
  assert.deepEqual([c.choice, c.customLogoBytes, c.customLabelBytes], ['customLogo', logo, null]);
  assert.equal(redo(c), true);
  assert.deepEqual([c.choice, c.customLabelBytes], ['customLabel', label]);
});

test('going back to the card label is a step like any other', () => {
  const c = cart({ choice: 'card' });
  commit(c, () => Object.assign(c, { choice: 'real', byHand: true }));
  undo(c);
  assert.deepEqual([c.choice, c.byHand], ['card', false]);
});

test('revert goes back to the visit and is itself undoable', () => {
  const c = cart();
  visit(c);
  commit(c, () => (c.shell = 'a'));
  commit(c, () => (c.deep = [1, 2, 3]));
  assert.equal(canRevert(c), true);
  assert.equal(revert(c), true);
  assert.deepEqual([c.shell, c.deep], ['', null]);
  assert.equal(canRevert(c), false);
  assert.equal(undo(c), true);
  assert.deepEqual([c.shell, c.deep], ['a', [1, 2, 3]]);
});

test('revert with nothing changed since the visit is no step', () => {
  const c = cart();
  visit(c);
  assert.equal(revert(c), false);
  assert.equal(canUndo(c), false);
});

test('a later visit moves where revert goes back to', () => {
  const c = cart();
  visit(c);
  commit(c, () => (c.shell = 'a'));
  visit(c);
  commit(c, () => (c.shell = 'b'));
  revert(c);
  assert.equal(c.shell, 'a');
});

test('a cart still being looked up has no visit until it settles', () => {
  const c = cart({ looking: true, game: null });
  visit(c);
  assert.equal(visited(c), false);
  c.looking = false;
  visit(c);
  assert.equal(visited(c), true);
});

test('nothing steps while a cart is being looked up', () => {
  const c = cart();
  visit(c);
  commit(c, () => (c.game = 'Tetris DX'));
  c.looking = true;
  assert.deepEqual([canUndo(c), canRedo(c), canRevert(c)], [false, false, false]);
  assert.equal(undo(c), false);
  assert.equal(revert(c), false);
  assert.equal(c.game, 'Tetris DX');
});

test('undo leaves alone the fields history does not own', () => {
  const c = cart({ existing: 'face', error: null });
  commit(c, () => (c.shell = 'a'));
  c.existing = 'other face';
  undo(c);
  assert.equal(c.existing, 'other face');
});

test('amend rewrites every snapshot, so an undo after a re-fill keeps the new fill', () => {
  const c = cart({ choice: 'real' });
  visit(c);
  commit(c, () => (c.shell = 'a'));
  c.choice = 'logo';
  amend(c, (snap) => {
    if (!snap.byHand && snap.choice !== 'card') snap.choice = 'logo';
  });
  undo(c);
  assert.deepEqual([c.shell, c.choice], ['', 'logo']);
  redo(c);
  assert.deepEqual([c.shell, c.choice], ['a', 'logo']);
  revert(c);
  assert.deepEqual([c.shell, c.choice], ['', 'logo']);
});
