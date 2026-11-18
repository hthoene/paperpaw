"""Tests of the Thai language pack's word breaks (tools/thai-segment.py, #190):

    python3 -m unittest discover -s tools/plugin-files
"""

import importlib.util
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("thai_segment", ROOT / "tools/thai-segment.py")
ts = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ts)

SOURCE = ROOT / "tools/plugin-files/languages/th.ini"
Z = ts.ZWSP


def shown(text):
    return ts.segment(text, ts.words()).replace(Z, "|")


class Segment(unittest.TestCase):
    def test_words_are_found_without_a_space_between_them(self):
        self.assertEqual(shown("การตั้งค่าหน้าจอและปุ่มกด"), "การ|ตั้งค่า|หน้าจอ|และ|ปุ่มกด")
        self.assertEqual(shown("เปิดใช้เมื่อต้องการ"), "เปิด|ใช้|เมื่อ|ต้องการ")

    def test_it_is_idempotent_and_spaces_stay(self):
        once = ts.segment("ลบหนังสือ แล้วเปิดใหม่", ts.words())
        self.assertIn(" ", once)
        self.assertEqual(ts.segment(once, ts.words()), once)

    def test_placeholders_and_latin_are_left_alone(self):
        self.assertEqual(shown("อีก {count} เล่ม"), "อีก {count} เล่ม")
        self.assertEqual(shown("{count}เล่ม"), "{count}เล่ม")
        self.assertEqual(shown("ตั้งค่า Wi-Fi และ EPUB"), "ตั้งค่า Wi-Fi และ EPUB")

    def test_no_break_before_a_mark_or_after_a_leading_vowel(self):
        # Without the guard the word "เก" would be cut after its leading vowel.
        words = {"เ", "ก", "ที่", "ๆ"}
        self.assertNotIn("เ|", ts.segment("เก", words).replace(Z, "|"))
        for text in ("ที่ๆ", "เกที่"):
            made = ts.segment(text, words)
            for i, ch in enumerate(made):
                if ch == Z:
                    self.assertNotIn(made[i + 1], ts.FOLLOWS)
                    self.assertNotIn(made[i - 1], ts.LEADS)

    def test_a_word_not_listed_stays_whole(self):
        self.assertEqual(ts.segment("ซซซซซซซซซ", ts.words()), "ซซซซซซซซซ")


class Catalog(unittest.TestCase):
    def test_the_texts_hold_their_breaks(self):
        # What `apply` makes of the source is the source (run it after a change).
        known = ts.words()
        differ = [key for _, key, value in ts.messages(SOURCE)
                  if ts.segment(value, known) != value]
        self.assertEqual(differ, [])

    def test_nearly_every_long_thai_text_has_breaks(self):
        # A long run of letters with no break would never wrap at a word.
        long_run = ts.re.compile(r"[ก-๛]{14,}")
        whole = [key for _, key, value in ts.messages(SOURCE)
                 if long_run.search(ts.PLACEHOLDER.sub(" ", value))]
        total = sum(1 for _ in ts.messages(SOURCE))
        self.assertLess(len(whole), total // 50, whole[:20])

    def test_a_break_is_never_in_a_placeholder_or_before_a_mark(self):
        for _, key, value in ts.messages(SOURCE):
            for hole in ts.PLACEHOLDER.finditer(value):
                self.assertNotIn(Z, hole.group(), key)
            for i, ch in enumerate(value):
                if ch == Z:
                    self.assertNotIn(value[i + 1], ts.FOLLOWS, key)
                    self.assertNotIn(value[i - 1], ts.LEADS, key)

    def test_the_compiled_message_limit_holds_with_the_breaks(self):
        # card.rs: MAX_MESSAGE (4096 bytes); a text over it fails the build.
        for _, key, value in ts.messages(SOURCE):
            self.assertLessEqual(len(value.encode()), 4096, key)


if __name__ == "__main__":
    unittest.main()
