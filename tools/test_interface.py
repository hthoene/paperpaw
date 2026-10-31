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


def struct(id_, name, field_ids, visibility="default"):
    return item(id_, name, {"struct": {
        "kind": {"plain": {"fields": field_ids}}, "generics": {"params": []}}}, visibility)


def path_to(id_, name):
    return {"resolved_path": {"path": name, "id": id_, "args": None}}


def hooked(field_type, extra=None):
    """A public `Hook { f: <field_type> }` over a private `Band { x: i8 }`
    that only `field_type` names."""
    doc = {
        "paths": {"1": {"path": ["t", "Hook"]}, "4": {"path": ["t", "Band"]},
                  "7": {"path": ["t", "Alias"]}},
        "index": {
            "1": struct(1, "Hook", [2], "public"),
            "2": item(2, "f", {"struct_field": field_type}),
            "4": struct(4, "Band", [5]),
            "5": item(5, "x", {"struct_field": {"primitive": "i8"}}),
        },
    }
    doc["index"].update(extra or {})
    return doc


BAND_MUT = {"borrowed_ref": {"lifetime": None, "is_mutable": True, "type": path_to(4, "Band")}}
FN_BAND = {"parenthesized": {"inputs": [BAND_MUT], "output": None}}
BOXED_FN = {"resolved_path": {"path": "Box", "id": 8, "args": {"angle_bracketed": {
    "args": [{"type": {"dyn_trait": {"traits": [{"trait": {
        "path": "Fn", "id": 9, "args": FN_BAND}}], "lifetime": None}}}], "constraints": []}}}}
FN_POINTER = {"function_pointer": {"sig": {"inputs": [["b", BAND_MUT]], "output": None,
                                           "is_c_variadic": False},
                                   "generic_params": [], "header": {}}}
ALIAS = {"7": item(7, "Alias", {"type_alias": {"type": FN_POINTER, "generics": {"params": []}}})}


class TypesNamedInCallables(unittest.TestCase):
    """A type that only a closure bound, a function pointer or an alias of
    one names crosses all the same (ADR 0026): its layout is pinned."""

    def test_a_type_named_only_in_a_boxed_dyn_fn_is_in_the_shape(self):
        self.assertIn("struct t::Band { x: i8 }", shape(hooked(BOXED_FN)))

    def test_the_arguments_of_the_dyn_fn_are_in_the_field_s_line(self):
        self.assertIn("struct t::Hook { f: Box<dyn Fn(&mut t::Band)> }", shape(hooked(BOXED_FN)))

    def test_a_type_named_only_in_a_function_pointer_is_in_the_shape(self):
        self.assertIn("struct t::Band { x: i8 }", shape(hooked(FN_POINTER)))

    def test_a_type_named_only_in_an_alias_of_a_function_pointer_is_in_the_shape(self):
        self.assertIn("struct t::Band { x: i8 }", shape(hooked(path_to(7, "Alias"), ALIAS)))

    def test_an_alias_itself_is_not_a_line(self):
        self.assertFalse([l for l in shape(hooked(path_to(7, "Alias"), ALIAS)) if "Alias {" in l])


if __name__ == "__main__":
    unittest.main()
