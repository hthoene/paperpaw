"""Compile probes for borrowed ABI frame views, independent of Cargo artifacts."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
MODULE = ROOT / 'crates/paperpaw-core/src/abi7/context.rs'
PREFIX = '''#![allow(dead_code, reason = "compile-only lifetime probe")]
extern crate self as paperpaw_sdk;
extern crate self as paperpaw_i18n;
extern crate self as paperpaw_native;
extern crate self as paperpaw_platform;
pub mod host_glue { pub const ERR_STATE: i32 = -6; }
mod snapshot_wire { pub struct Snapshot; }
pub struct Settings;
pub struct Fonts;
pub trait Storage {}
pub struct Translator;
pub struct Id;
impl Id { pub fn as_str(&self) -> &str { "fixture" } }
pub mod usage {
    pub struct Refreshes { pub fast: u32, pub partial: u32, pub full: u32 }
    pub struct Usage {
        pub refreshes: Refreshes, pub pixels: u64, pub inputs: u32, pub sleeps: u32,
        pub panel_ms: u64, pub bluetooth_ms: u64,
    }
    pub struct PluginUsage {
        pub plugin: super::Id, pub busy_us: u64, pub calls: u32, pub shown_ms: u32,
        pub refreshes: Refreshes, pub pixels: u64, pub read: u32, pub written: u32,
        pub requests: u32, pub refused: u32, pub awake_ms: u32, pub awake_reason: &'static str,
    }
}
#[path = MODULE_PATH]
mod context;
'''.replace('MODULE_PATH', json.dumps(str(MODULE)))


class FrameLifetimes(unittest.TestCase):
    def test_owned_results_compile_but_frame_references_cannot_escape(self):
        cases = [
            ('owned', 'fn valid(s: &context::FrameSlot) { '
             'let _: Result<usize, _> = s.with(|_| 7); }', True),
            ('snapshot_owned', 'fn valid(s: &context::FrameSlot) { '
             'let _: Result<Result<snapshot_wire::Snapshot, i32>, _> = '
             's.with(|frame| frame.snapshot()); }', True),
            ('usage_owned', 'fn valid(s: &context::FrameSlot) { '
             'let _: Result<Result<context::DeviceUsageView, i32>, _> = '
             's.with(|frame| frame.device_usage()); }', True),
            ('row_text', 'fn escape(s: &context::FrameSlot) { let mut out = None; '
             's.with(|f| f.plugin_rows(0, &mut |r| { out = Some(r.plugin); '
             'core::ops::ControlFlow::Continue(()) })).unwrap(); let _ = out; }', False),
            ('name_text', 'fn escape(s: &context::FrameSlot) { let mut out = None; '
             's.with(|f| f.plugin_name("fixture", &mut |text| out = Some(text))).unwrap(); '
             'let _ = out; }', False),
            ('note_text', 'fn escape(s: &context::FrameSlot) { let mut out = None; '
             's.with(|f| f.work_note("fixture", &mut |_, text| out = Some(text))).unwrap(); '
             'let _ = out; }', False),
            ('frame', 'fn escape(s: &context::FrameSlot) -> &dyn context::Frame { '
             's.with(|f| f).unwrap() }', False),
            ('derived', 'fn escape(s: &context::FrameSlot) -> &Settings { '
             's.with(|f| f.settings()).unwrap() }', False),
            ('capture', 'fn escape(s: &context::FrameSlot) { let mut out = None; '
             's.with(|f| out = Some(f)).unwrap(); let _ = out; }', False),
        ]
        with tempfile.TemporaryDirectory(prefix='paperpaw-frame-') as directory:
            folder = Path(directory)
            for name, body, accepted in cases:
                with self.subTest(case=name):
                    source = folder / (name + '.rs')
                    source.write_text(PREFIX + body)
                    result = subprocess.run(
                        ['rustc', '--edition', '2024', '--crate-type', 'lib',
                         str(source), '--out-dir', str(folder)],
                        capture_output=True, text=True, check=False)
                    if accepted:
                        self.assertEqual(result.returncode, 0, result.stderr)
                    else:
                        self.assertNotEqual(result.returncode, 0, 'borrow escaped')
                        self.assertRegex(result.stderr, r'lifetime may not live long enough|E0521')


if __name__ == '__main__':
    unittest.main()
