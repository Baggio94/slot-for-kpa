import { test } from 'node:test';
import assert from 'node:assert/strict';
import { hasLog, logError, logText } from './log.js';

test('an empty log has nothing to copy', () => {
  assert.equal(hasLog(), false);
});

test('the log carries each error with its name, message and what it was doing', () => {
  const quiet = console.error;
  console.error = () => {};
  try {
    const refused = Object.assign(new Error('The requested file could not be read'), { name: 'NotReadableError' });
    logError('Config/cart_shell.ini', refused);
    logError('Zelda', 'not an error');
  } finally {
    console.error = quiet;
  }
  assert.equal(hasLog(), true);
  const text = logText('Agent/1.0');
  assert.match(text, /^Agent\/1\.0\n/);
  assert.match(text, /Config\/cart_shell\.ini: NotReadableError: The requested file could not be read/);
  assert.match(text, /Zelda: not an error/);
});
