"""Keep menu needs and capability bits equal in the SDK, the host and the contract.

`menu.need` (record 15) numbers needs as the SDK's `MenuNeed` and capability
bits as the SDK's `Capability`; the host's checker and the contract text name
them in the same order. A need or capability added to the SDK must be added to
all three.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]


def kebab(name):
    """`NoTouch` and `CHARGE_LIMIT` both become `no-touch`/`charge-limit`."""
    if name.isupper() or '_' in name:
        return name.lower().replace('_', '-')
    return re.sub(r'(?<!^)(?=[A-Z])', '-', name).lower()


def rust_list(source, name):
    body = re.search(rf'pub const {name}: \[&str; \d+\] = \[(.*?)\];', source, re.S)
    return re.findall(r'"([a-z-]+)"', body.group(1))


class MenuNeeds(unittest.TestCase):
    def setUp(self):
        self.sdk_env = (ROOT / 'crates/paperpaw-sdk/src/env.rs').read_text()
        self.sdk_menus = (ROOT / 'crates/paperpaw-sdk/src/menus.rs').read_text()
        self.host = (ROOT / 'crates/paperpaw-core/src/native_host/declarations.rs').read_text()
        self.doc = (ROOT / 'docs/abi/screens-input.md').read_text()

    def test_capability_bits(self):
        bits = re.findall(r'pub const ([A-Z_]+): Self = Self\((1|1 << \d+)\);', self.sdk_env)
        sdk = {int(value.split('<<')[1]) if '<<' in value else 0: kebab(name)
               for name, value in bits}
        self.assertEqual(len(sdk), len(bits), 'two capabilities share a bit')
        sdk = [sdk[bit] for bit in range(len(sdk))]
        self.assertEqual(sdk, rust_list(self.host, 'MENU_NEED_CAPABILITIES'))
        listed = re.search(r'set of capability bits.*?bit 0\s+(.*?),\s+as the SDK', self.doc, re.S)
        doc = re.findall(r'`([a-z-]+)`', listed.group(1))
        self.assertEqual(sdk, doc)

    def test_needs(self):
        enum = re.search(r'pub enum MenuNeed \{(.*?)\n\}', self.sdk_menus, re.S).group(1)
        sdk = [kebab(name) for name in re.findall(r'^    ([A-Z][A-Za-z]*)', enum, re.M)]
        self.assertEqual(sdk, rust_list(self.host, 'MENU_NEEDS'))
        listed = re.search(r'`need`: 0 `speech`(.*?)\*\*A need', self.doc, re.S)
        doc = ['speech'] + re.findall(r'\d+ `([a-z-]+)`', listed.group(1))
        self.assertEqual(sdk, doc)


if __name__ == '__main__':
    unittest.main()
