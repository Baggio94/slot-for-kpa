"""Which media a cart still needs. Run from tools/: python3 -m unittest test_harvest"""

import tempfile
import unittest
from pathlib import Path

import harvest


class TodoFor(unittest.TestCase):
    def test_look_is_skipped_once_recorded_unless_relooked(self):
        root = Path(tempfile.mkdtemp())
        (root / 'wheel').mkdir()
        (root / 'wheel' / 'AAAA0000.png').write_bytes(b'x')
        index = {'AAAA0000': {'look': {'ground': '000000', 'band': None, 'sure': True, 'by': 'analysis'}}}
        wanted = ['wheel', 'look']
        self.assertEqual(harvest.todo_for('AAAA0000', wanted, index, root, relook=False), [])
        self.assertEqual(harvest.todo_for('AAAA0000', wanted, index, root, relook=True), ['look'])
        self.assertEqual(harvest.todo_for('BBBB1111', wanted, index, root, relook=False), ['wheel', 'look'])



class WriteIndex(unittest.TestCase):
    def test_the_index_is_compact_and_sorted(self):
        path = Path(tempfile.mkdtemp()) / 'index.json'
        harvest.write_index(path, {'BBBB1111': {'wheel': 'GB/wheel/BBBB1111.png'}, 'AAAA0000': {'look': None}})
        self.assertEqual(
            path.read_text(),
            '{"AAAA0000":{"look":null},"BBBB1111":{"wheel":"GB/wheel/BBBB1111.png"}}',
        )


class RegionsFor(unittest.TestCase):
    def test_a_dump_prefers_its_own_region(self):
        self.assertEqual(harvest.regions_for('Metroid Fusion (USA)'), ('us', 'wor', 'eu', 'jp'))
        self.assertEqual(harvest.regions_for('Metroid Fusion (USA, Australia)'), ('us', 'wor', 'eu', 'jp'))
        self.assertEqual(harvest.regions_for('Metroid Fusion (Japan)'), ('jp', 'us', 'wor', 'eu'))
        self.assertEqual(harvest.regions_for('Metroid Fusion (Europe) (En,Fr,De,Es,It)'), ('eu', 'us', 'wor', 'jp'))
        self.assertEqual(harvest.regions_for('Pokemon - Gelbe Edition (Germany)'), ('de', 'eu', 'us', 'wor', 'jp'))
        self.assertEqual(harvest.regions_for('Tetris (World) (Rev 1)'), ('wor', 'us', 'eu', 'jp'))
        self.assertEqual(harvest.regions_for('E60EC183'), ('us', 'wor', 'eu', 'jp'))

    def test_pick_follows_the_order_it_is_given(self):
        medias = [{'type': 'wheel', 'region': 'us', 'url': 'u'}, {'type': 'wheel', 'region': 'jp', 'url': 'j'}]
        self.assertEqual(harvest.pick(medias, 'wheel')['url'], 'u')
        self.assertEqual(harvest.pick(medias, 'wheel', harvest.regions_for('X (Japan)'))['url'], 'j')
        self.assertEqual(harvest.pick(medias, 'wheel', harvest.regions_for('X (France)'))['url'], 'u')


class Redo(unittest.TestCase):
    def test_redo_asks_for_every_media_again(self):
        root = Path(tempfile.mkdtemp())
        (root / 'wheel').mkdir()
        (root / 'wheel' / 'AAAA0000.png').write_bytes(b'x')
        index = {'AAAA0000': {'look': {'ground': '000000', 'band': None, 'sure': True, 'by': 'analysis'}}}
        self.assertEqual(harvest.todo_for('AAAA0000', ['wheel', 'look'], index, root, relook=False, redo=True), ['wheel', 'look'])


if __name__ == '__main__':
    unittest.main()
