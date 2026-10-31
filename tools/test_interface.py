"""Tests of what tools/interface.py writes into a crate's shape.txt."""

import copy
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("interface", HERE / "interface.py")
interface = importlib.util.module_from_spec(spec)
spec.loader.exec_module(interface)


def item(id_, name, inner, visibility="default"):
    return {"id": id_, "crate_id": 0, "name": name, "visibility": visibility,
            "attrs": [], "inner": inner}


SELF = {"borrowed_ref": {"lifetime": None, "is_mutable": False, "type": {"generic": "Self"}}}

# A public handle over a private trait object, whose item returns a
# private type that no field names: `Cache::anchors() -> Anchors`.
DOC = {
    "paths": {
        "1": {"path": ["t", "GlyphCache"]},
        "2": {"path": ["t", "Cache"]},
        "4": {"path": ["t", "Anchors"]},
    },
    "index": {
        "1": item(1, "GlyphCache", {"struct": {
            "kind": {"tuple": [6]}, "generics": {"params": []}}}, "public"),
        "6": item(6, "0", {"struct_field": {"dyn_trait": {
            "traits": [{"trait": {"path": "Cache", "id": 2, "args": None}}],
            "lifetime": None}}}),
        "2": item(2, "Cache", {"trait": {"items": [3], "generics": {"params": []}}}),
        "3": item(3, "anchors", {"function": {
            "sig": {"inputs": [["self", SELF], ["size", {"primitive": "f32"}]],
                    "output": {"resolved_path": {"path": "Anchors", "id": 4, "args": None}}},
            "generics": {"params": []}}}),
        "4": item(4, "Anchors", {"struct": {
            "kind": {"plain": {"fields": [5]}}, "generics": {"params": []}}}),
        "5": item(5, "x", {"struct_field": {"primitive": "i8"}}),
    },
}


def shape(doc):
    shapes = interface.Shapes(doc)
    return sorted(shapes.line(i) for i in shapes.wanted())


class TraitItems(unittest.TestCase):
    def test_a_trait_s_items_have_their_signatures(self):
        self.assertIn("trait t::Cache { function anchors(&Self, f32) -> t::Anchors }", shape(DOC))

    def test_a_type_a_trait_item_returns_is_in_the_shape(self):
        self.assertIn("struct t::Anchors { x: i8 }", shape(DOC))

    def test_changing_that_type_changes_the_shape(self):
        changed = copy.deepcopy(DOC)
        changed["index"]["5"]["inner"]["struct_field"] = {"primitive": "i16"}
        self.assertNotEqual(shape(DOC), shape(changed))
        self.assertIn("struct t::Anchors { x: i16 }", shape(changed))


if __name__ == "__main__":
    unittest.main()
