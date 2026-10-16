import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fromDirectory, fromFiles } from './card.js';

const notFound = () => Object.assign(new Error('not found'), { name: 'NotFoundError' });

const dirHandle = (tree) => ({
  kind: 'directory',
  async getDirectoryHandle(name) {
    if (typeof tree[name] !== 'object') throw notFound();
    return dirHandle(tree[name]);
  },
  async getFileHandle(name) {
    if (typeof tree[name] !== 'string') throw notFound();
    return fileHandle(tree[name]);
  },
  async *entries() {
    for (const [name, value] of Object.entries(tree)) {
      yield [name, typeof value === 'object' ? dirHandle(value) : fileHandle(value)];
    }
  },
});
const fileHandle = (text) => ({ kind: 'file', getFile: async () => new File([text], 'f') });

const listed = (tree, prefix = 'card') =>
  Object.entries(tree).flatMap(([name, value]) =>
    typeof value === 'object'
      ? listed(value, `${prefix}/${name}`)
      : [Object.assign(new File([value], name), { webkitRelativePath: `${prefix}/${name}` })],
  );

const GAMES = { Games: { GBA: { 'A.gba': '' } } };
const CONFIG = 'A = rounded 112233 solid\n';
const SYSTEM = 'A = notched 445566 clear\n';
const LABELS = 'A = auto\n';

const layers = async (source) => ({ system: await source.systemShells(), labels: await source.labelShells() });
const both = async (tree) => [await layers(await fromDirectory(dirHandle(tree))), await layers(fromFiles(listed(tree)))];

test('a card slot has moved reads its shells from Config', async () => {
  for (const got of await both({ ...GAMES, Config: { 'cart_shell.txt': CONFIG }, Labels: { 'cart_shell.txt': LABELS } })) {
    assert.deepEqual(got, { system: CONFIG, labels: LABELS });
  }
});

test('a card slot has not moved yet reads its shells from System', async () => {
  for (const got of await both({ ...GAMES, System: { 'cart_shell.ini': SYSTEM } })) {
    assert.deepEqual(got, { system: SYSTEM, labels: '' });
  }
});

test('Config wins over a System file left behind', async () => {
  for (const got of await both({ ...GAMES, Config: { 'cart_shell.txt': CONFIG }, System: { 'cart_shell.ini': SYSTEM } })) {
    assert.deepEqual(got, { system: CONFIG, labels: '' });
  }
});

test('an empty Config file is still the one slot reads', async () => {
  for (const got of await both({ ...GAMES, Config: { 'cart_shell.txt': '' }, System: { 'cart_shell.ini': SYSTEM } })) {
    assert.deepEqual(got, { system: '', labels: '' });
  }
});

test('a Config folder without the file falls back to System', async () => {
  for (const got of await both({ ...GAMES, Config: {}, System: { 'cart_shell.ini': SYSTEM } })) {
    assert.deepEqual(got, { system: SYSTEM, labels: '' });
  }
});

test('a card with no shell files reads as empty', async () => {
  for (const got of await both(GAMES)) {
    assert.deepEqual(got, { system: '', labels: '' });
  }
});

test('a Config file the browser refuses fails only its own layer', async () => {
  const refused = Object.assign(new Error('could not be read'), { name: 'NotReadableError' });
  const card = dirHandle({ ...GAMES, Config: { 'cart_shell.txt': CONFIG }, Labels: { 'cart_shell.txt': LABELS } });
  const config = await card.getDirectoryHandle('Config');
  const source = await fromDirectory({
    ...card,
    async getDirectoryHandle(name) {
      if (name !== 'Config') return card.getDirectoryHandle(name);
      return { ...config, getFileHandle: async () => ({ kind: 'file', getFile: async () => Promise.reject(refused) }) };
    },
  });
  await assert.rejects(source.systemShells(), { name: 'NotReadableError' });
  assert.equal(await source.labelShells(), LABELS);
});

test('a card slot has not started on since the rename reads its .ini shells', async () => {
  for (const got of await both({ ...GAMES, Config: { 'cart_shell.ini': CONFIG }, Labels: { 'cart_shell.ini': LABELS } })) {
    assert.deepEqual(got, { system: CONFIG, labels: LABELS });
  }
});

test('a .txt wins over the .ini beside it', async () => {
  const tree = {
    ...GAMES,
    Config: { 'cart_shell.ini': SYSTEM, 'cart_shell.txt': CONFIG },
    Labels: { 'cart_shell.ini': SYSTEM, 'cart_shell.txt': LABELS },
  };
  for (const got of await both(tree)) {
    assert.deepEqual(got, { system: CONFIG, labels: LABELS });
  }
});

test('an .ini the browser will not open by name asks for slot to start once', async () => {
  const card = dirHandle({ ...GAMES, Labels: { 'cart_shell.ini': LABELS } });
  const labels = await card.getDirectoryHandle('Labels');
  const source = await fromDirectory({
    ...card,
    async getDirectoryHandle(name) {
      if (name !== 'Labels') return card.getDirectoryHandle(name);
      return {
        ...labels,
        async getFileHandle(file) {
          if (file.endsWith('.ini')) throw new TypeError('Name is not allowed.');
          return labels.getFileHandle(file);
        },
      };
    },
  });
  await assert.rejects(source.labelShells(), { name: 'OldShellFile' });
});

test('shells are written to Labels/cart_shell.txt', async () => {
  const written = {};
  const card = dirHandle({ ...GAMES, Labels: {} });
  const source = await fromDirectory({
    ...card,
    async getDirectoryHandle(name) {
      if (name !== 'Labels') return card.getDirectoryHandle(name);
      return {
        async getFileHandle(file) {
          return {
            async createWritable() {
              return { write: async (t) => (written[`${name}/${file}`] = t), close: async () => {} };
            },
          };
        },
      };
    },
  });
  await source.writeShells(LABELS);
  assert.deepEqual(written, { 'Labels/cart_shell.txt': LABELS });
});

test('a listed card carries every file in its Labels folder but dotfiles', async () => {
  const source = fromFiles(
    listed({
      ...GAMES,
      labels: {
        'cart_shell.txt': LABELS,
        '.DS_Store': '',
        GBA: { 'A.png': 'a', 'Gone.png': 'g', '._A.png': '' },
        Odd: { 'note.txt': 'n' },
      },
      Saves: { GBA: { 'A.sav': '' } },
    }),
  );
  const got = await Promise.all(source.labelFiles.map(async (f) => [f.path, await (await f.file()).text()]));
  assert.deepEqual(got, [
    ['Labels/cart_shell.txt', LABELS],
    ['Labels/GBA/A.png', 'a'],
    ['Labels/GBA/Gone.png', 'g'],
    ['Labels/Odd/note.txt', 'n'],
  ]);
});

test('a folder the browser listed as empty says it could not be read', () => {
  assert.throws(() => fromFiles([]), /couldn’t read that folder/);
});

test('a folder with files but no games is still not a slot card', () => {
  assert.throws(() => fromFiles(listed({ Photos: { 'a.jpg': '' } })), /doesn’t look like a slot SD card/);
});

test('roms loose in Games ask to be moved into platform folders by hand', async () => {
  const loose = { Games: { 'A.gba': '' } };
  const asks = { message: /Move them into the GBA, GB and GBC folders/ };
  await assert.rejects(fromDirectory(dirHandle(loose)), asks);
  assert.throws(() => fromFiles(listed(loose)), asks);
});

test('a card without shell files reads as empty where the browser refuses .ini names', async () => {
  const windows = (dir) => ({
    ...dir,
    async getDirectoryHandle(name) {
      return windows(await dir.getDirectoryHandle(name));
    },
    async getFileHandle(name) {
      if (name.endsWith('.ini')) throw new TypeError('Name is not allowed.');
      return dir.getFileHandle(name);
    },
  });
  const source = await fromDirectory(windows(dirHandle({ ...GAMES, Config: {}, Labels: {} })));
  assert.deepEqual(await layers(source), { system: '', labels: '' });
});
