"""Probe generated callback readers without Cargo or generated-file freshness."""
import copy
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('abi_gen_callbacks', ROOT / 'tools/abi-gen.py')
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)
from abi_callbacks import rust_callbacks


def plugin_fields(schema):
    return schema['entry_contract']['tables'][0]['fields']


RUST_TESTS = r'''
use core::ffi::c_void;
use core::mem::{align_of, offset_of, size_of};
use core::sync::atomic::{AtomicUsize, Ordering};

type Dispatch = unsafe extern "C" fn(*mut c_void, u32, PpBytes) -> i32;
type Draw = unsafe extern "C" fn(*mut c_void, u32) -> i32;
static CALLS: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn dispatch(_: *mut c_void, token: u32, _: PpBytes) -> i32 {
    CALLS.fetch_add(1, Ordering::SeqCst);
    token as i32
}

unsafe extern "C" fn draw(_: *mut c_void, token: u32) -> i32 {
    CALLS.fetch_add(1, Ordering::SeqCst);
    token as i32
}

unsafe extern "C" fn prefix_dispatch(_: *mut c_void, token: u32, _: PpBytes) -> i32 {
    token as i32
}

fn table() -> PpPlugin {
    PpPlugin {
        size: size_of::<PpPlugin>() as u32,
        // These readers must not inspect or validate plugin state or drop.
        state: core::ptr::without_provenance_mut(1),
        drop: None,
        dispatch_event: Some(dispatch),
        draw: Some(draw),
        input: None,
    }
}

fn bytes(table: &PpPlugin) -> *const u8 {
    core::ptr::from_ref(table).cast()
}

fn prefix_end() -> usize {
    offset_of!(PpPlugin, drop) + size_of::<Option<unsafe extern "C" fn(*mut c_void)>>()
}

unsafe fn both_fail(table: *const u8, accessible: usize, expected: i32) {
    // SAFETY: The test supplies either readable storage or a span rejected first.
    unsafe {
        assert_eq!(read_plugin_dispatch_event(table, accessible).unwrap_err(), expected);
        assert_eq!(read_plugin_draw(table, accessible).unwrap_err(), expected);
    }
}

#[test]
fn null_misaligned_and_invalid_lengths_are_rejected_before_size_read() {
    let full = table();
    let pointer = bytes(&full);
    // SAFETY: These spans must be rejected before any dereference.
    unsafe {
        both_fail(core::ptr::null(), 0, ERR_INVALID);
        both_fail(core::ptr::null(), size_of::<PpPlugin>(), ERR_INVALID);
        both_fail(pointer.add(1), size_of::<PpPlugin>() - 1, ERR_INVALID);
        for length in 0..size_of::<u32>() {
            both_fail(pointer, length, ERR_INVALID);
        }
        both_fail(pointer, isize::MAX as usize + 1, ERR_INVALID);
        // Aligned, non-null, and short enough, but its numeric span wraps.
        let last_aligned = usize::MAX & !(align_of::<PpPlugin>() - 1);
        both_fail(core::ptr::without_provenance(last_aligned), 16, ERR_INVALID);
    }
}

#[test]
fn truncated_spans_and_oversized_or_short_declarations_are_invalid() {
    let mut full = table();
    // SAFETY: All reads are within full's live initialized storage.
    unsafe {
        for length in size_of::<u32>()..size_of::<PpPlugin>() {
            both_fail(bytes(&full), length, ERR_INVALID);
        }
        for declared in [0, 1, size_of::<u32>() as u32, (prefix_end() - 1) as u32,
                         (size_of::<PpPlugin>() + 1) as u32, u32::MAX] {
            full.size = declared;
            both_fail(bytes(&full), size_of::<PpPlugin>(), ERR_INVALID);
        }
    }
}

#[repr(C)]
struct Prefix {
    size: u32,
    state: *mut c_void,
    drop: Option<unsafe extern "C" fn(*mut c_void)>,
}

#[repr(C)]
struct ThroughDispatch {
    prefix: Prefix,
    dispatch_event: Option<Dispatch>,
}

#[test]
fn real_short_prefix_has_no_optional_callbacks() {
    let prefix = Prefix {
        size: size_of::<Prefix>() as u32,
        state: core::ptr::null_mut(),
        drop: None,
    };
    assert_eq!(size_of::<Prefix>(), prefix_end());
    assert_eq!(align_of::<Prefix>(), align_of::<PpPlugin>());
    // SAFETY: Only the size word of this real short allocation may be read.
    unsafe {
        both_fail(core::ptr::from_ref(&prefix).cast(), size_of::<Prefix>(), ERR_UNSUPPORTED);
    }
}

#[test]
fn real_dispatch_prefix_can_retrieve_only_dispatch() {
    let prefix = ThroughDispatch {
        prefix: Prefix {
            size: size_of::<ThroughDispatch>() as u32,
            state: core::ptr::null_mut(),
            drop: None,
        },
        dispatch_event: Some(prefix_dispatch),
    };
    assert_eq!(offset_of!(ThroughDispatch, dispatch_event), offset_of!(PpPlugin, dispatch_event));
    assert_eq!(size_of::<ThroughDispatch>(), offset_of!(PpPlugin, draw));
    let pointer = core::ptr::from_ref(&prefix).cast();
    // SAFETY: Only fields present in this real short allocation may be read.
    unsafe {
        let found = read_plugin_dispatch_event(pointer, size_of::<ThroughDispatch>()).unwrap();
        // Function addresses need not be identical across codegen instances.
        let input = PpBytes { ptr: core::ptr::null(), len: 0 };
        assert_eq!(found(core::ptr::null_mut(), 19, input), 19);
        assert_eq!(read_plugin_draw(pointer, size_of::<ThroughDispatch>()).unwrap_err(),
                   ERR_UNSUPPORTED);
    }
}

#[test]
fn declared_field_bounds_take_precedence_over_available_storage() {
    let mut full = table();
    let dispatch_end = offset_of!(PpPlugin, dispatch_event) + size_of::<Option<Dispatch>>();
    let draw_end = offset_of!(PpPlugin, draw) + size_of::<Option<Draw>>();
    for declared in prefix_end()..dispatch_end {
        full.size = declared as u32;
        // SAFETY: The declaration excludes both callbacks in valid storage.
        unsafe { both_fail(bytes(&full), size_of::<PpPlugin>(), ERR_UNSUPPORTED); }
    }
    for declared in dispatch_end..draw_end {
        full.size = declared as u32;
        // SAFETY: The declaration covers dispatch, but not the complete draw field.
        unsafe {
            assert!(read_plugin_dispatch_event(bytes(&full), size_of::<PpPlugin>()).is_ok());
            assert_eq!(read_plugin_draw(bytes(&full), size_of::<PpPlugin>()).unwrap_err(),
                       ERR_UNSUPPORTED);
        }
    }
}

#[test]
fn null_callbacks_are_independently_unsupported() {
    let mut full = table();
    full.dispatch_event = None;
    // SAFETY: All callback fields are initialized with valid nullable pointers.
    unsafe {
        assert_eq!(read_plugin_dispatch_event(bytes(&full), size_of::<PpPlugin>()).unwrap_err(),
                   ERR_UNSUPPORTED);
        assert!(read_plugin_draw(bytes(&full), size_of::<PpPlugin>()).is_ok());
        full.dispatch_event = Some(dispatch);
        full.draw = None;
        assert!(read_plugin_dispatch_event(bytes(&full), size_of::<PpPlugin>()).is_ok());
        assert_eq!(read_plugin_draw(bytes(&full), size_of::<PpPlugin>()).unwrap_err(),
                   ERR_UNSUPPORTED);
        full.dispatch_event = None;
        both_fail(bytes(&full), size_of::<PpPlugin>(), ERR_UNSUPPORTED);
    }
}

#[test]
fn retrieval_does_not_call_callbacks_and_returns_callable_unsafe_pointers() {
    let full = table();
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    // SAFETY: The table and callback module stay live throughout this test.
    unsafe {
        let dispatch_pointer: Dispatch =
            read_plugin_dispatch_event(bytes(&full), size_of::<PpPlugin>()).unwrap();
        let draw_pointer: Draw = read_plugin_draw(bytes(&full), size_of::<PpPlugin>()).unwrap();
        assert_eq!(CALLS.load(Ordering::SeqCst), 0);
        let input = PpBytes { ptr: core::ptr::null(), len: 0 };
        assert_eq!(dispatch_pointer(full.state, 17, input), 17);
        assert_eq!(draw_pointer(full.state, 23), 23);
    }
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
}
unsafe extern "C" fn prefix_drop(state: *mut c_void) {
    // SAFETY: This test passes a pointer to its live AtomicUsize.
    unsafe { &*state.cast::<AtomicUsize>() }.fetch_add(1, Ordering::SeqCst);
}

#[test]
fn prefix_accessors_support_real_short_tables_without_invoking_drop() {
    let drops = AtomicUsize::new(0);
    let state = core::ptr::from_ref(&drops).cast_mut().cast();
    let mut prefix = Prefix { size: size_of::<Prefix>() as u32, state, drop: Some(prefix_drop) };
    assert_eq!(PLUGIN_PREFIX_BYTES, size_of::<Prefix>());
    let pointer = core::ptr::from_ref(&prefix).cast();
    // SAFETY: The exact prefix allocation and callback/state remain live.
    unsafe {
        assert_eq!(read_plugin_size(pointer, size_of::<Prefix>()), Ok(size_of::<Prefix>()));
        assert_eq!(read_plugin_state(pointer, size_of::<Prefix>()), Ok(state));
        let drop = read_plugin_drop(pointer, size_of::<Prefix>()).unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(state);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        for length in 0..size_of::<Prefix>() {
            assert_eq!(read_plugin_size(pointer, length), Err(ERR_INVALID));
            assert_eq!(read_plugin_state(pointer, length), Err(ERR_INVALID));
            assert_eq!(read_plugin_drop(pointer, length).unwrap_err(), ERR_INVALID);
        }
    }
    prefix.drop = None;
    // SAFETY: A null optional representation is valid, but mandatory drop rejects it.
    unsafe {
        assert_eq!(read_plugin_drop(core::ptr::from_ref(&prefix).cast(), size_of::<Prefix>())
                   .unwrap_err(), ERR_INVALID);
    }
}

'''


class CallbackReaderTests(unittest.TestCase):
    def setUp(self):
        self.schema = ABI.load(ROOT / 'abi/abi.def')

    def test_absent_contract_and_absent_tails_emit_nothing(self):
        prior = copy.deepcopy(self.schema)
        plugin_fields(prior)[:] = plugin_fields(prior)[:3]
        self.assertEqual(rust_callbacks(prior), '')
        del prior['statuses']
        self.assertEqual(rust_callbacks(prior), '')
        del prior['entry_contract']
        self.assertEqual(rust_callbacks(prior), '')
        fixture = ABI.load(ROOT / 'abi/fixtures/metadata/schema.json')
        plugin_fields(fixture)[:] = plugin_fields(fixture)[:3]
        self.assertEqual(rust_callbacks(fixture), '')

    def test_only_declared_recognized_callbacks_get_readers(self):
        for name in ('dispatch_event', 'draw'):
            candidate = copy.deepcopy(self.schema)
            fields = plugin_fields(candidate)
            fields[:] = [field for field in fields
                         if field['name'] not in ('dispatch_event', 'draw') or field['name'] == name]
            rendered = rust_callbacks(candidate)
            self.assertIn(f'pub unsafe fn read_plugin_{name}(', rendered)
            other = 'draw' if name == 'dispatch_event' else 'dispatch_event'
            self.assertNotIn(f'pub unsafe fn read_plugin_{other}(', rendered)

    def test_known_names_require_exact_nullable_signatures(self):
        mutations = [lambda field: field['callback'].update(nullable=False),
                     lambda field: field['callback'].update(nullable=1),
                     lambda field: field['callback'].update(result='void'),
                     lambda field: field['callback']['args'][0].update(name='other'),
                     lambda field: field['callback']['args'][0].update(type='u32'),
                     lambda field: field['callback']['args'].reverse(),
                     lambda field: field['callback']['args'].pop(),
                     lambda field: field['callback'].update(extra=True)]
        for name in ('dispatch_event', 'draw'):
            for index, mutate in enumerate(mutations):
                with self.subTest(name=name, mutation=index):
                    candidate = copy.deepcopy(self.schema)
                    field = next(field for field in plugin_fields(candidate) if field['name'] == name)
                    mutate(field)
                    with self.assertRaises(ValueError):
                        rust_callbacks(candidate)
            candidate = copy.deepcopy(self.schema)
            field = next(field for field in plugin_fields(candidate) if field['name'] == name)
            del field['callback']
            field['type'] = 'pointer'
            with self.assertRaises(ValueError):
                rust_callbacks(candidate)

    def test_callback_status_constants_are_required_and_exact(self):
        for name in ('ERR_INVALID', 'ERR_UNSUPPORTED'):
            for mutation in ('missing', 'wrong', 'duplicate'):
                with self.subTest(name=name, mutation=mutation):
                    candidate = copy.deepcopy(self.schema)
                    statuses = candidate['statuses']
                    status = next(item for item in statuses if item['name'] == name)
                    if mutation == 'missing':
                        statuses.remove(status)
                    elif mutation == 'wrong':
                        status['value'] = -99
                    else:
                        statuses.append(copy.deepcopy(status))
                    with self.assertRaisesRegex(ValueError, name):
                        rust_callbacks(candidate)
        for statuses in (None, {}, [None]):
            with self.subTest(statuses=statuses):
                candidate = dict(self.schema, statuses=statuses)
                with self.assertRaisesRegex(ValueError, 'status declarations'):
                    rust_callbacks(candidate)

    def test_readers_are_core_only_and_never_borrow_full_table(self):
        rendered = rust_callbacks(self.schema)
        self.assertNotIn('&PpPlugin', rendered)
        self.assertNotIn('std::', rendered.replace('core::', ''))
        self.assertIn('offset_of!(PpPlugin, drop)', rendered)
        self.assertIn('offset_of!(PpPlugin, dispatch_event)', rendered)
        self.assertIn('offset_of!(PpPlugin, draw)', rendered)

    def compile_probe(self, base, suffix):
        # Render into a temporary raw module: checked-in generated files need not
        # be fresh, and integration may already include the same public readers.
        (base / 'raw.rs').write_text(ABI.rust_module(self.schema))
        source = base / 'probe.rs'
        source.write_text(
            '#[allow(dead_code, reason = "The probe uses only callback types.")]\n'
            'mod raw;\nuse raw::*;\n' + rust_callbacks(self.schema) + suffix)
        return source

    def test_native_raw_callback_reads(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = self.compile_probe(base, RUST_TESTS)
            subprocess.run(['rustc', '--edition=2024', '-Dwarnings', '--test',
                            str(source), '-o', str(base / 'probe')], check=True)
            subprocess.run([str(base / 'probe')], check=True)

    def test_x4_readers_compile_without_std(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = self.compile_probe(base, '')
            source.write_text('#![no_std]\n' + source.read_text())
            subprocess.run(['rustc', '--edition=2024', '-Dwarnings', '--crate-type=lib',
                            '--target', 'riscv32imc-unknown-none-elf',
                            str(source), '-o', str(base / 'probe.rlib')], check=True)


if __name__ == '__main__':
    unittest.main()
