"""Pin temporary SDK storage limits to the ABI and Kernel's contract."""
import json
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]


class StorageLimits(unittest.TestCase):
    def test_storage_limits_match_the_schema_and_contract(self):
        sdk = (ROOT / "crates/paperpaw-sdk/src/native/storage.rs").read_text()
        limits = {name: int(value) for name, value in re.findall(
            r"pub const (MAX_[A-Z]+): usize = ([0-9]+);", sdk)}
        abi = json.loads((ROOT / "abi/abi.def").read_text())
        slots = {entry["name"]: entry for entry in abi["host_entries"]}
        for name in ("storage.size", "storage.rename", "storage.remove"):
            self.assertEqual(limits["MAX_PATH"], slots[name]["max_bytes"])
        for name in ("storage.read_at", "storage.write", "storage.append"):
            self.assertEqual(limits["MAX_DATA"], slots[name]["max_bytes"])
        contract = (ROOT / "docs/abi/proposals/interface-7-events-storage-services.md").read_text()
        segment = re.search(r"D2\.3 Path rules.*?(\d+) per segment", contract, re.S)
        self.assertIsNotNone(segment)
        self.assertEqual(limits["MAX_SEGMENT"], int(segment.group(1)))


if __name__ == "__main__":
    unittest.main()
