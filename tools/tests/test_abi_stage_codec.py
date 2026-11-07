"""Owner-local codec collisions, service names and canonical framing."""
import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools'))
from abi_stage_codec import stage_codec_source
from abi_records import rust_records


def schema(name='filter.pair'):
    return dict(max_bytes=3840, max_records=33, records=[dict(
        id=1535, name=name, since=1, fields=[dict(name='text', type='str',
        since=1, optional=False, ordinal=None, default=None, max_bytes=96)])])


class StageCodec(unittest.TestCase):
    def test_rust_name_collision_refused(self):
        for names in [('filter.some_pair', 'filter.some.pair'),
                      ('filter.a_b', 'filter.a.b')]:
            value = schema(names[0])
            row = copy.deepcopy(value['records'][0])
            row.update(id=101, name=names[1])
            value['records'].append(row)
            with self.assertRaisesRegex(ValueError, 'colliding Rust'):
                stage_codec_source(value)

    def test_invalid_caps_and_tags(self):
        for key, value in [('max_bytes', 4097), ('max_records', 65)]:
            bad = schema()
            bad[key] = value
            with self.assertRaises(ValueError):
                stage_codec_source(bad)
        for tag in [0, 65536]:
            bad = schema()
            bad['records'][0]['id'] = tag
            with self.assertRaises(ValueError):
                stage_codec_source(bad)

    def test_service_named_stage_compiles_and_matches_kernel_framing(self):
        local = schema('service.fixture')
        abi = json.loads((ROOT / 'abi/abi.def').read_text())
        abi['records'].append(local['records'][0])
        source = ('#![allow(dead_code)]\nmod stage {\n' + stage_codec_source(local)
                  + '\n}\nmod kernel {\n' + rust_records(abi) + '\n}\n')
        source += r'''
#[test] fn framing() {
    let mut a = [0u8; 3840]; let mut b = [0u8; 3840];
    let n = stage::records_encode(&[stage::Record::ServiceFixture(stage::ServiceFixture {text:"hello"})], &mut a).unwrap();
    let m = kernel::records_encode(&[kernel::Record::ServiceFixture(kernel::ServiceFixture {text:"hello"})], &mut b).unwrap();
    assert_eq!(&a[..n], &b[..m]);
    assert_eq!(stage::records_decode(&a[..n]).unwrap().len(), 1);
    a[1] = 0x80; a[2] = 0x0c; // unknown tag1536
    assert_eq!(stage::records_decode(&a[..n]).unwrap_err(), -1);
    assert!(stage::records_decode(&[0;3841]).is_err());
    assert!(stage::records_encode(&[stage::Record::ServiceFixture(stage::ServiceFixture {text:&"x".repeat(97)})], &mut a).is_err());
    assert!(stage::record_encode_one(&stage::Record::ServiceFixture(stage::ServiceFixture {text:"x"}), &mut a).is_ok());
}
'''
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'codec.rs'
            path.write_text(source)
            exe = Path(tmp) / 'codec'
            subprocess.run(['rustc', '--edition=2024', '--test', str(path), '-o', str(exe)], check=True, capture_output=True)
            subprocess.run([str(exe)], check=True, capture_output=True)

    def test_checked_in_reader_codec_is_current(self):
        subprocess.run([sys.executable, str(ROOT / 'tools/reader-filter-gen.py'), '--check'], check=True)
