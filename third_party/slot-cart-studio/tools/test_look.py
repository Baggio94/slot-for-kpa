"""A label's look measured from a synthetic cart scan. Run from tools/: python3 -m unittest test_look"""

import io
import random
import unittest

from PIL import Image

import look

BLUE, RED, WHITE, GREY = (30, 80, 200), (200, 20, 30), (245, 245, 240), (150, 150, 150)


def png(im):
    out = io.BytesIO()
    im.save(out, 'PNG')
    return out.getvalue()


def rgb(h):
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))


def near(a, b, by=14):
    return all(abs(x - y) <= by for x, y in zip(a, b))


def scan(size, ground=BLUE):
    """A template scan whose label box is filled with `ground` and a white logo in its middle."""
    im = Image.new('RGB', size, GREY)
    l, t, r, b = look.LABEL_BOX[size]
    im.paste(ground, (l, t, r, b))
    w, h = r - l, b - t
    im.paste(WHITE, (l + w * 3 // 8, t + h * 2 // 5, l + w * 5 // 8, t + h * 3 // 5))
    return im, (l, t, r, b)


class MeasureLook(unittest.TestCase):
    def test_a_top_band_and_its_ground(self):
        im, (l, t, r, b) = scan((600, 355))
        im.paste(RED, (l, t, r, t + round((b - t) * 0.16)))
        got = look.measure_look(png(im))
        self.assertTrue(near(rgb(got['ground']), BLUE), got)
        self.assertEqual(got['band']['edge'], 'top')
        self.assertAlmostEqual(got['band']['size'], 0.16, delta=0.02)
        self.assertTrue(near(rgb(got['band']['colour']), RED), got)
        self.assertTrue(got['sure'])
        self.assertEqual(got['by'], 'analysis')

    def test_no_band(self):
        im, _ = scan((600, 355))
        got = look.measure_look(png(im))
        self.assertIsNone(got['band'])
        self.assertTrue(near(rgb(got['ground']), BLUE))
        self.assertTrue(got['sure'])

    def test_a_left_band_on_a_game_boy_label_beyond_its_side_strip(self):
        im, (l, t, r, b) = scan((600, 678))
        strip = round((r - l) * look.SIDE_STRIP)
        inner = (r - l) - 2 * strip
        im.paste(RED, (l + strip, t, l + strip + round(inner * 0.12), b))
        got = look.measure_look(png(im))
        self.assertEqual(got['band']['edge'], 'left')
        self.assertAlmostEqual(got['band']['size'], 0.12, delta=0.02)

    def test_game_boy_side_strips_are_not_a_band(self):
        im, (l, t, r, b) = scan((600, 678))
        strip = round((r - l) * look.SIDE_STRIP)
        rnd = random.Random(1)
        for x0 in (l, r - strip):
            im.paste(WHITE, (x0, t, x0 + strip, b))
            for _ in range(400):  # the strip's printed text
                x, y = rnd.randrange(x0, x0 + strip), rnd.randrange(t, b)
                im.putpixel((x, y), (20, 20, 20))
        got = look.measure_look(png(im))
        self.assertIsNone(got['band'])
        self.assertTrue(near(rgb(got['ground']), BLUE))

    def test_an_off_centre_game_boy_scan_does_not_turn_a_side_strip_into_a_band(self):
        # The label sits 20 px off in the scan: 14% of paper down the left, 6% down the right.
        im, (l, t, r, b) = scan((600, 678))
        w = r - l
        im.paste(WHITE, (l, t, l + round(w * 0.14), b))
        im.paste(WHITE, (r - round(w * 0.06), t, r, b))
        got = look.measure_look(png(im))
        self.assertFalse(got['band'] is not None and got['sure'], got)

    def test_a_faint_strip_is_not_a_band_and_is_still_sure(self):
        im, (l, t, r, b) = scan((600, 355))
        im.paste((30, 80, 170), (l, t, r, t + round((b - t) * 0.16)))  # 30 away from the ground
        got = look.measure_look(png(im))
        self.assertIsNone(got['band'])
        self.assertTrue(got['sure'])

    def test_a_strip_the_colour_of_the_ground_is_not_a_band(self):
        # The ground runs to the bottom edge past a dark rule; the strip below the rule is
        # the ground again, not a band of its own.
        im, (l, t, r, b) = scan((600, 355))
        h = b - t
        im.paste((10, 10, 10), (l, t + round(h * 0.84), r, t + round(h * 0.92)))
        got = look.measure_look(png(im))
        self.assertIsNone(got['band'])

    def test_busy_artwork_is_not_sure(self):
        im, (l, t, r, b) = scan((600, 355))
        rnd = random.Random(2)
        noise = Image.new('RGB', (r - l, b - t))
        noise.putdata([tuple(rnd.randrange(256) for _ in range(3)) for _ in range((r - l) * (b - t))])
        im.paste(noise, (l, t))
        self.assertFalse(look.measure_look(png(im))['sure'])

    def test_an_unknown_scan_or_bad_bytes_have_no_look(self):
        self.assertIsNone(look.measure_look(png(Image.new('RGB', (500, 500), BLUE))))
        self.assertIsNone(look.measure_look(b'not a png'))


if __name__ == '__main__':
    unittest.main()
