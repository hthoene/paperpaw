"""tools/flash-x4.py validates --features before it backs up, asks or builds (#228)."""
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('flash_x4', ROOT / 'tools/flash-x4.py')
FLASH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FLASH)


class FeaturesTest(unittest.TestCase):
    def check(self, device, text):
        return FLASH.check_features(FLASH.DEVICES[device], text)

    def test_names_and_lists_pass(self):
        for text in ('ppn5', 'remote', 'ppn5,remote', 'a_b-c1'):
            self.assertIsNone(self.check('x4', text), text)

    def test_leading_dash_and_other_characters_are_refused(self):
        for text in ('-Zbuild-std', '--release', 'ppn5,-x', 'ppn5,', ',ppn5', '', 'a b',
                     'a;b', 'a=b', '$(x)', 'ppn5,,remote'):
            self.assertIsNotNone(self.check('x4', text), repr(text))

    def test_ppn5_is_refused_on_the_xtensa_devices(self):
        for device in ('x4-pro', 'x4-classic'):
            self.assertIsNotNone(self.check(device, 'ppn5'), device)
            self.assertIsNotNone(self.check(device, 'remote,ppn5'), device)
            self.assertIsNone(self.check(device, 'remote'), device)
        for device in ('x4', 'x3'):
            self.assertIsNone(self.check(device, 'ppn5'), device)


if __name__ == '__main__':
    unittest.main()
