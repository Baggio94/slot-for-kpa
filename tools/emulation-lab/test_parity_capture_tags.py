"""Keep the capture filter connected to the actual native and Kotlin producers."""
from pathlib import Path
import re
import shlex
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ParityCaptureTags(unittest.TestCase):
    def test_all_emulation_producers_reach_the_same_capture(self):
        kotlin = ROOT / 'android/app/src/main/java/fyi/slot/konkr'
        producers = set()
        for source in (kotlin / 'MainActivity.kt', kotlin / 'EmulationDiagnostics.kt'):
            producers.add(re.search(r'const val TAG = "([^"]+)"', source.read_text())[1])
        native = (ROOT / 'crates/slot-android/src/game.rs').read_text()
        producers.add(re.search(r'__android_log_write\(4, c"([^"]+)"', native)[1])
        capture = (ROOT / 'tools/retroarch-parity/capture_pair.sh').read_text()
        command = next(line for line in capture.splitlines() if 'logcat -v' in line)
        args = shlex.split(command)
        selected = {arg.rsplit(':', 1)[0] for arg in args if arg.endswith(':I')}
        self.assertTrue(producers <= selected, f'missing producers: {producers - selected}')
        self.assertIn('*:S', args)
        self.assertNotIn('-c', args, 'capture must not erase device logs')


if __name__ == '__main__':
    unittest.main()
