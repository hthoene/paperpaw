"""Card plugins must package their own translation catalogs, including Battery."""
import pathlib
import re
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import catalogue


def card_plugins():
    """Native file candidates absent from the linked app, plus store plugins.

    The X4 leaves the store plugins out even when another profile links them.
    Include the old store catalogue too while its interface-7 list is ported.
    """
    app = (catalogue.ROOT / "crates/paperpaw-app/src/lib.rs").read_text()
    linked_table = app.split("pub static NATIVE_PLUGINS:", 1)[1].split("];", 1)[0]
    linked = set(re.findall(r'\(\s*"([a-z0-9-]+)"', linked_table))
    store_table = app.split("pub const STORE_PLUGINS:", 1)[1].split("];", 1)[0]
    store = set(re.findall(r'"([a-z0-9-]+)"', store_table))
    native = {p.stem for p in (catalogue.ROOT / "tools/plugin-files7/src/bin").glob("*.rs")}
    return (native - linked) | store | {plugin for plugin, _ in catalogue.plugins()}


class CardCatalogs(unittest.TestCase):
    def test_every_card_plugin_selects_its_source_catalogs(self):
        plugins = card_plugins()
        self.assertIn("battery", plugins)
        self.assertIn("vocabulary", plugins)
        self.assertNotIn("home", plugins)
        for plugin in sorted(plugins):
            selected = dict(catalogue.card_files(plugin))
            source = catalogue.ROOT / "plugins" / plugin
            for catalog in sorted((source / "i18n").glob("*.ini")):
                name = catalog.relative_to(source).as_posix()
                with self.subTest(plugin=plugin, catalog=name):
                    self.assertIn(name, selected, "source catalog missing from card package")
                    self.assertEqual(selected[name], catalog.read_text().encode())

    def test_every_store_plugin_folder_carries_english_and_german(self):
        for plugin, _ in catalogue.plugins():
            names = {name for name, _ in catalogue.card_files(plugin)}
            for language in ("en", "de"):
                with self.subTest(plugin=plugin, language=language):
                    self.assertIn(f"i18n/{language}.ini", names, "the folder would show raw text keys")

    def test_battery_zip_contains_english_and_german_titles(self):
        with tempfile.TemporaryDirectory() as directory:
            package = pathlib.Path(directory) / "battery.zip"
            catalogue.folder_zip(package, "[plugin]\nid = battery\n", catalogue.card_files("battery"))
            with zipfile.ZipFile(package) as archive:
                for language, title in [("en", "Battery"), ("de", "Akku")]:
                    with self.subTest(language=language):
                        catalog = archive.read(f"i18n/{language}.ini").decode()
                        self.assertIn(f"battery.title = {title}\n", catalog)


if __name__ == "__main__":
    unittest.main()
