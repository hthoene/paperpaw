"""Check entry layouts and one native C callee/Rust caller exchange.

The linked fixture is test-only. It does not qualify a loader, malformed
tables, or the X4 calling convention. X4 checks only compile raw layouts.
"""
import os
import json
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
FIELDS = {
    'PpEntry': ('size', 'abi_major', 'needs_minor', 'metadata', 'make'),
    'PpPlugin': ('size', 'state', 'drop', 'dispatch_event', 'draw', 'input'),
}
MAKE = ('Option<unsafe extern "C" fn(*const PpHost, '
        '*mut core::ffi::c_void, *mut *const PpPlugin) -> i32>')
DROP = 'Option<unsafe extern "C" fn(*mut core::ffi::c_void)>'
DISPATCH_EVENT = ('Option<unsafe extern "C" fn(*mut core::ffi::c_void, '
                  'u32, PpBytes) -> i32>')
DRAW = 'Option<unsafe extern "C" fn(*mut core::ffi::c_void, u32) -> i32>'


def raw_module():
    return ('#[allow(dead_code, reason = "The probe uses only entry types.")]\n'
            f'#[path = "{ROOT}/abi/generated/paperpaw_abi.rs"] mod raw;\n'
            'use raw::*;\n')


def compile_c(source, output, *options):
    subprocess.run(shlex.split(os.environ.get('CC', 'cc')) + [
        '-std=c11', '-Wall', '-Wextra', '-Werror', '-I', str(ROOT),
        *options, str(source), '-o', str(output)], check=True)


def compile_rust(source, output, *options):
    subprocess.run(['rustc', '--edition=2024', '-Dwarnings', *options,
                    str(source), '-o', str(output)], check=True)


class EntryLayoutTests(unittest.TestCase):
    def test_declared_profiles_match_independently_asserted_layouts(self):
        schema = json.loads((ROOT / 'abi/abi.def').read_text())
        profiles = {p['name']: p for p in schema['metadata']['profiles']}
        expected = {'linux-x86_64-sysv64-le': (64, 32, 8),
                    'esp32c3-riscv32imc-ilp32-soft-le': (32, 20, 4),
                    'esp32s3-xtensa-lx7-ilp32-windowed-le': (32, 20, 4),
                    # Linked plugins only (Decision 172): no card file loads there.
                    'windows-x86_64-win64-le': (64, 32, 8),
                    'macos-x86_64-sysv64-le': (64, 32, 8),
                    'macos-aarch64-darwinpcs-le': (64, 32, 8),
                    'android-aarch64-aapcs64-le': (64, 32, 8),
                    'web-wasm32-c-le': (32, 20, 4)}
        self.assertEqual(set(profiles), set(expected))
        for name, values in expected.items():
            with self.subTest(profile=name):
                profile = profiles[name]
                self.assertEqual((profile['pointer_bits'], profile['entry_bytes'],
                                  profile['entry_align']), values)
                self.assertEqual(profile['byte_order'], 'little')
                self.assertEqual(profile['entry_layout'], 1)

    def require_native_x86_64(self):
        if platform.machine().lower() not in ('x86_64', 'amd64'):
            self.skipTest('Native entry probes currently cover x86_64 only')

    def test_c_and_rust_entry_layouts_match_known_x86_64_values(self):
        self.require_native_x86_64()
        c = ['#include <stdio.h>', '#include "abi/generated/paperpaw-abi.h"',
             'int main(void) {']
        rust = [raw_module(), 'fn main() {']
        for kind, names in FIELDS.items():
            c.append(f'printf("%zu %zu\\n", sizeof({kind}), _Alignof({kind}));')
            rust.append(f'println!("{{}} {{}}", core::mem::size_of::<{kind}>(), '
                        f'core::mem::align_of::<{kind}>());')
            for name in names:
                c.append(f'printf("%zu\\n", offsetof({kind}, {name}));')
                rust.append(f'println!("{{}}", core::mem::offset_of!({kind}, {name}));')
        c.extend(['printf("%zu\\n", sizeof(void *));',
                  'printf("%zu\\n", sizeof(((PpEntry *)0)->make));',
                  'printf("%zu\\n", sizeof(((PpPlugin *)0)->drop));',
                  'printf("%zu\\n", sizeof(((PpPlugin *)0)->dispatch_event));',
                  'printf("%zu\\n", sizeof(((PpPlugin *)0)->draw));',
                  'return 0; }'])
        rust.extend(['println!("{}", core::mem::size_of::<*mut core::ffi::c_void>());',
                     f'println!("{{}}", core::mem::size_of::<{MAKE}>());',
                     f'println!("{{}}", core::mem::size_of::<{DROP}>());',
                     f'println!("{{}}", core::mem::size_of::<{DISPATCH_EVENT}>());',
                     f'println!("{{}}", core::mem::size_of::<{DRAW}>());', '}'])
        expected = [32, 8, 0, 4, 6, 8, 24, 48, 8, 0, 8, 16, 24, 32, 40,
                    8, 8, 8, 8, 8]
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            (base / 'layout.c').write_text('\n'.join(c))
            (base / 'layout.rs').write_text('\n'.join(rust))
            compile_c(base / 'layout.c', base / 'c-layout')
            compile_rust(base / 'layout.rs', base / 'rust-layout')
            for executable in ('c-layout', 'rust-layout'):
                with self.subTest(language=executable):
                    values = subprocess.check_output([str(base / executable)]).split()
                    self.assertEqual([int(value) for value in values], expected)

    def test_x4_no_std_entry_layouts_match_known_32_bit_values(self):
        self.no_std_entry_layouts(['rustc'], 'riscv32imc-unknown-none-elf')

    def test_x4_pro_no_std_entry_layouts_match_known_32_bit_values(self):
        # The X4 Pro and X4 Classic (ESP32-S3, Xtensa LX7, windowed ABI):
        # only where the esp toolchain is installed (CI builds the S3 shells).
        rustup = shutil.which('rustup')
        if rustup is None or subprocess.run(
                [rustup, 'run', 'esp', 'rustc', '--print', 'target-list'],
                capture_output=True, text=True).stdout.find('xtensa-esp32s3-none-elf') < 0:
            self.skipTest('the esp toolchain (xtensa-esp32s3-none-elf) is not installed')
        self.no_std_entry_layouts([rustup, 'run', 'esp', 'rustc'], 'xtensa-esp32s3-none-elf')

    def test_linked_only_profiles_match_their_targets_layouts(self):
        # The profiles of the shells that host linked plugins only (Decision
        # 172), each where its Rust target is installed (CI installs them).
        installed = subprocess.run(['rustup', 'target', 'list', '--installed'],
                                   capture_output=True, text=True).stdout.split()
        for target, bits in (('x86_64-pc-windows-msvc', 64), ('x86_64-apple-darwin', 64),
                             ('aarch64-apple-darwin', 64), ('aarch64-linux-android', 64),
                             ('wasm32-unknown-unknown', 32)):
            with self.subTest(target=target):
                if target not in installed:
                    self.skipTest(f'the Rust target {target} is not installed')
                self.no_std_entry_layouts(['rustc'], target, bits)

    def no_std_entry_layouts(self, rustc, target, bits=32):
        word = bits // 8
        assertions = ['#![no_std]', raw_module(), 'const _: () = {']
        layouts = ((('PpEntry', 20, 4, (0, 4, 6, 8, 16)),
                    ('PpPlugin', 24, 4, (0, 4, 8, 12, 16, 20))) if bits == 32 else
                   (('PpEntry', 32, 8, (0, 4, 6, 8, 24)),
                    ('PpPlugin', 48, 8, (0, 8, 16, 24, 32, 40))))
        for kind, size, align, offsets in layouts:
            assertions.append(f'assert!(core::mem::size_of::<{kind}>() == {size});')
            assertions.append(f'assert!(core::mem::align_of::<{kind}>() == {align});')
            for field, offset in zip(FIELDS[kind], offsets):
                assertions.append(
                    f'assert!(core::mem::offset_of!({kind}, {field}) == {offset});')
        for kind in ('*mut core::ffi::c_void', MAKE, DROP, DISPATCH_EVENT, DRAW):
            assertions.append(f'assert!(core::mem::size_of::<{kind}>() == {word});')
            assertions.append(f'assert!(core::mem::align_of::<{kind}>() == {word});')
        assertions.extend(['assert!(core::mem::size_of::<u64>() == 8);',
                           'assert!(core::mem::align_of::<u64>() == 8);'])
        assertions.append('};')
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / 'x4.rs'
            source.write_text('\n'.join(assertions))
            subprocess.run([*rustc, '--edition=2024', '-Dwarnings', '--crate-type=lib',
                            '--target', target, str(source), '-o', str(base / 'x4.rlib')],
                           check=True)

    def test_x4_freestanding_c_entry_layouts_match_known_32_bit_values(self):
        clang = shutil.which('clang')
        self.assertIsNotNone(clang, 'clang is required for the freestanding X4 C layout probe')
        assertions = ['#include "abi/generated/paperpaw-abi.h"']
        for kind, size, align, offsets in (
                ('PpEntry', 20, 4, (0, 4, 6, 8, 16)),
                ('PpPlugin', 24, 4, (0, 4, 8, 12, 16, 20))):
            assertions.extend([
                f'_Static_assert(sizeof({kind}) == {size}, "{kind} size");',
                f'_Static_assert(_Alignof({kind}) == {align}, "{kind} alignment");'])
            for field, offset in zip(FIELDS[kind], offsets):
                assertions.append(f'_Static_assert(offsetof({kind}, {field}) == '
                                  f'{offset}, "{kind}.{field} offset");')
        for kind in ('void *', 'int32_t (*)(const PpHost *, void *, const PpPlugin **)',
                     'void (*)(void *)',
                     'int32_t (*)(void *, uint32_t, PpBytes)',
                     'int32_t (*)(void *, uint32_t)'):
            assertions.extend([
                f'_Static_assert(sizeof({kind}) == 4, "pointer size");',
                f'_Static_assert(_Alignof({kind}) == 4, "pointer alignment");'])
        assertions.extend([
            '_Static_assert(sizeof(uint64_t) == 8, "uint64_t size");',
            '_Static_assert(_Alignof(uint64_t) == 8, "uint64_t alignment");'])
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / 'x4.c'
            source.write_text('\n'.join(assertions))
            subprocess.run([clang, '--target=riscv32-unknown-none-elf',
                            '-march=rv32imc', '-mabi=ilp32', '-ffreestanding',
                            '-std=c11', '-Wall', '-Wextra', '-Werror',
                            '-I', str(ROOT), '-c', str(source),
                            '-o', str(base / 'x4.o')], check=True)

    def test_native_c_entry_make_and_drop_called_from_rust(self):
        self.require_native_x86_64()
        c = r'''
#include "abi/generated/paperpaw-abi.h"

/* All storage and callbacks below belong only to this linked test fixture. */
static unsigned host_context;
static unsigned drops;
static unsigned calls;
static unsigned bad_drops;
static const uint8_t metadata[] = {0x50, 0x50, 0x00, 0xff, 0x07};
/* Rust receives only a raw pointer and never forms a PpHost reference. */
static const PpHost host = {
    .size = sizeof(PpHost), .major = PP_ABI_MAJOR, .minor = PP_ABI_MINOR
};

static void drop_plugin(void *state) {
    if (state == &drops) {
        ++drops;
    } else {
        ++bad_drops;
    }
}

static const PpPlugin plugin = {
    .size = sizeof(PpPlugin), .state = &drops, .drop = drop_plugin
};

static int32_t make_plugin(const PpHost *table, void *ctx, const PpPlugin **out) {
    ++calls;
    /* No slot beyond the fixed host prefix is read or called. */
    if (table == NULL || table->size != sizeof(PpHost) ||
        table->major != PP_ABI_MAJOR || table->minor != PP_ABI_MINOR ||
        ctx != &host_context || out == NULL) {
        return PP_ERR_INVALID;
    }
    *out = &plugin;
    return PP_OK;
}

static const PpEntry entry = {
    .size = sizeof(PpEntry), .abi_major = PP_ABI_MAJOR,
    .needs_minor = PP_ABI_MINOR,
    .metadata = {metadata, sizeof(metadata)}, .make = make_plugin
};

const PpEntry *probe_entry(void) { return &entry; }
const PpHost *probe_host(void) { return &host; }
void *probe_host_context(void) { return &host_context; }
unsigned probe_drops(void) { return drops; }
unsigned probe_calls(void) { return calls; }
unsigned probe_bad_drops(void) { return bad_drops; }
'''
        rust = raw_module() + r'''
use core::ffi::{c_uint, c_void};
use core::mem::{offset_of, size_of};
use core::ptr::{addr_of, null};

unsafe extern "C" {
    fn probe_entry() -> *const PpEntry;
    fn probe_host() -> *const PpHost;
    fn probe_host_context() -> *mut c_void;
    fn probe_drops() -> c_uint;
    fn probe_calls() -> c_uint;
    fn probe_bad_drops() -> c_uint;
}

fn main() {
    // The linked C fixture owns valid static storage for this entire call.
    unsafe {
        let entry = probe_entry();
        assert!(!entry.is_null());
        // Check the byte count before reading fields. No table reference is made.
        let entry_size = addr_of!((*entry).size).read() as usize;
        assert!(entry_size >= size_of::<PpEntry>());
        assert_eq!(addr_of!((*entry).abi_major).read(), MAJOR);
        assert_eq!(addr_of!((*entry).needs_minor).read(), MINOR);
        let metadata = addr_of!((*entry).metadata).read();
        assert!(!metadata.ptr.is_null());
        assert_eq!(metadata.len, 5);
        // The known C fixture supplies five readable static bytes.
        assert_eq!(core::slice::from_raw_parts(metadata.ptr, metadata.len),
                   &[0x50, 0x50, 0x00, 0xff, 0x07]);
        let make = addr_of!((*entry).make).read().expect("fixture make callback");
        let host = probe_host();
        assert!(!host.is_null());
        let context = probe_host_context();
        assert!(!context.is_null());
        assert_eq!(probe_calls(), 0);
        assert_eq!(probe_drops(), 0);
        let mut plugin: *const PpPlugin = null();
        assert_eq!(make(host, context, &mut plugin), OK);
        assert_eq!(probe_calls(), 1);
        assert_eq!(probe_drops(), 0);
        assert!(!plugin.is_null());
        let plugin_size = addr_of!((*plugin).size).read() as usize;
        assert!(plugin_size >= offset_of!(PpPlugin, drop)
                + size_of::<Option<unsafe extern "C" fn(*mut c_void)>>());
        let state = addr_of!((*plugin).state).read();
        assert!(!state.is_null());
        let drop = addr_of!((*plugin).drop).read().expect("fixture drop callback");
        drop(state);
        assert_eq!(probe_drops(), 1);
        assert_eq!(probe_calls(), 1);
        assert_eq!(probe_bad_drops(), 0);
    }
}
'''
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            (base / 'callee.c').write_text(c)
            (base / 'caller.rs').write_text(rust)
            compile_c(base / 'callee.c', base / 'callee.o', '-c')
            compile_rust(base / 'caller.rs', base / 'entry-probe',
                         '-C', f'link-arg={base / "callee.o"}')
            subprocess.run([str(base / 'entry-probe')], check=True)


if __name__ == '__main__':
    unittest.main()
