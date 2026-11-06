"""Local tag validation and canonical framing equivalence (no ABI output edits)."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
from abi_stage_codec import stage_codec_source


class StageCodec(unittest.TestCase):
    def schema(self):
        abi = json.loads((ROOT / "abi/abi.def").read_text())
        record = next(r for r in abi["records"] if r["name"] == "setting.group")
        return dict(max_bytes=4096, max_records=8, records=[record])

    def test_rejects_duplicate_and_bad_caps(self):
        schema = self.schema()
        for key, value in [("max_bytes", 0), ("max_records", 65)]:
            bad = copy.deepcopy(schema)
            bad[key] = value
            with self.assertRaises(ValueError):
                stage_codec_source(bad)
        schema["records"] *= 2
        with self.assertRaises(ValueError):
            stage_codec_source(schema)

    def test_scalar_only_schema_compiles(self):
        schema = self.schema()
        schema["records"][0]["fields"] = [
            field for field in schema["records"][0]["fields"] if field["type"] == "i32"]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / "scalar.rs").write_text(stage_codec_source(schema))
            subprocess.run(["rustc", "--edition=2024", "--crate-type=lib",
                            str(path / "scalar.rs"), "-o", str(path / "scalar.rlib")], check=True)

    def test_same_schema_same_canonical_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / "local.rs").write_text(stage_codec_source(self.schema()))
            kernel = (ROOT / "abi/generated/records_wire.rs").as_posix()
            (path / "test.rs").write_text('''#![allow(dead_code)]
mod local { include!("local.rs"); }
mod kernel { include!("''' + kernel + '''"); }
#[test] fn same() {
    let mut a=[0;4096]; let mut b=[0;4096];
    let x=local::Record::SettingGroup(local::SettingGroup {
        name:"group",title:"title",order:-3,page:"page",parent:"parent"});
    let y=kernel::Record::SettingGroup(kernel::SettingGroup {
        name:"group",title:"title",order:-3,page:"page",parent:"parent"});
    let n=local::records_encode(&[x],&mut a).unwrap();
    let m=kernel::records_encode(&[y],&mut b).unwrap();
    assert_eq!(&a[..n],&b[..m]);
    assert_eq!(local::records_decode(&b[..m]).unwrap().len(),1);
    a[1]=6; assert_eq!(local::records_decode(&a[..n]).unwrap_err(),-1);
}
''')
            subprocess.run(["rustc", "--edition=2024", "--test", str(path / "test.rs"),
                            "-o", str(path / "check")], check=True)
            subprocess.run([str(path / "check")], check=True)


if __name__ == "__main__":
    unittest.main()
