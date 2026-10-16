import { test } from 'node:test';
import assert from 'node:assert/strict';
import { arrange } from './palette.js';

const swatch = (name, hex, finish = 'solid') => ({ name, value: `auto ${hex} ${finish}` });

test('neutrals come first, light to dark, then colours around the wheel', () => {
  const { neutrals, colours } = arrange([
    swatch('black', '333031'),
    swatch('pink', 'ec94b4', 'clear'),
    swatch('blue', '2f5cc0', 'clear'),
    swatch('white', 'eceee8'),
    swatch('red', 'c2332e', 'clear'),
    swatch('yellow', 'e2b413'),
    swatch('grey', '9a978f'),
    swatch('green', '249c60', 'clear'),
    swatch('smoke clear', '7c7a8a', 'clear'),
  ]);
  assert.deepEqual(neutrals.map((s) => s.name), ['white', 'grey', 'smoke clear', 'black']);
  assert.deepEqual(colours.map((s) => s.name), ['red', 'yellow', 'green', 'blue', 'pink']);
});

test('a red just short of the full circle stays with the reds, ahead of pink', () => {
  const { colours } = arrange([swatch('pink', 'ec94b4'), swatch('orange', 'd85224'), swatch('deep red', 'c0282c')]);
  assert.deepEqual(colours.map((s) => s.name), ['deep red', 'orange', 'pink']);
});
