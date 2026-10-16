import { test } from 'node:test';
import assert from 'node:assert/strict';
import { smallPrint } from './smallprint.js';

test('a GBA cart prints its product code, with the region its code was made for', () => {
  assert.equal(smallPrint('GBA', 'AWRE', ['USA']), 'AGB-AWRE-USA');
  assert.equal(smallPrint('GBA', 'BPEP', []), 'AGB-BPEP-EUR');
  assert.equal(smallPrint('GBA', 'AMTJ', ['Japan']), 'AGB-AMTJ-JPN');
});

test('a GBA code whose last letter names no region prints without one', () => {
  assert.equal(smallPrint('GBA', 'AWRZ', ['USA']), 'AGB-AWRZ');
});

test('a GBA cart with no real code falls back to its regions', () => {
  assert.equal(smallPrint('GBA', '', ['USA', 'Europe']), 'AGB · USA, EUR');
  assert.equal(smallPrint('GBA', '\u0000\u0000ab', ['World']), 'AGB · World');
});

test('Game Boy and Game Boy Color carts print nothing', () => {
  assert.equal(smallPrint('GB', '', ['USA', 'Europe']), '');
  assert.equal(smallPrint('GBC', '', ['Japan']), '');
});

test('languages, revisions and repeats are not regions', () => {
  assert.equal(smallPrint('GBA', '', ['USA', 'En', 'Fr', 'Rev 1', 'USA']), 'AGB · USA');
  assert.equal(smallPrint('GBA', '', ['Proto']), 'AGB');
});
