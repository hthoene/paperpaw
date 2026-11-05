"""Compile raw generated declarations and compare native C/Rust layouts.

This does not qualify cross-target calling conventions or short-table access.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class LayoutTests(unittest.TestCase):
    def test_c_and_rust_sizes_alignments_and_every_field_offset_agree(self):
        schema = json.loads((ROOT / 'abi/abi.def').read_text())
        fields = {'PpBytes': ['ptr', 'len'], 'PpBuffer': ['ptr', 'cap'],
                  'PpHost': ['size', 'major', 'minor'] + [
                      e['name'].replace('.', '_') for e in schema['host_entries']]}
        fields.update({table['name']: [field['name'] for field in table['fields']]
                       for table in schema['entry_contract']['tables']})
        c = ['#include <stdio.h>', '#include "abi/generated/paperpaw-abi.h"',
             'int main(void) {']
        rust = ['#[allow(dead_code, reason = "layout probe uses only selected generated items")]', f'#[path = "{ROOT}/abi/generated/paperpaw_abi.rs"] mod raw;',
                'use raw::*;', 'fn main() {']
        for kind, names in fields.items():
            c.append(f'printf("%zu %zu\\n", sizeof({kind}), _Alignof({kind}));')
            rust.append(f'println!("{{}} {{}}", core::mem::size_of::<{kind}>(), '
                        f'core::mem::align_of::<{kind}>());')
            for name in names:
                c.append(f'printf("%zu\\n", offsetof({kind}, {name}));')
                rust.append(f'println!("{{}}", core::mem::offset_of!({kind}, {name}));')
        for c_name, rust_name in [('PP_ABI_MAJOR', 'MAJOR'), ('PP_ABI_MINOR', 'MINOR')] + [
                ('PP_' + s['name'], s['name']) for s in schema['statuses']]:
            c.append(f'printf("%d\\n", {c_name});')
            rust.append(f'println!("{{}}", {rust_name});')
        c.append('return 0; }')
        rust.append('}')
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            (base / 'layout.c').write_text('\n'.join(c))
            (base / 'layout.rs').write_text('\n'.join(rust))
            subprocess.run([os.environ.get('CC', 'cc'), '-std=c11', '-Wall', '-Werror',
                            '-I', str(ROOT), str(base / 'layout.c'), '-o',
                            str(base / 'c-layout')], check=True)
            subprocess.run(['rustc', '--edition=2024', '-Dwarnings',
                            str(base / 'layout.rs'), '-o', str(base / 'rust-layout')], check=True)
            self.assertEqual(subprocess.check_output([str(base / 'c-layout')]),
                             subprocess.check_output([str(base / 'rust-layout')]))

    def test_raw_module_compiles_without_std_on_host_and_x4(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / 'raw.rs'
            source.write_text('#![no_std]\n//! Raw ABI compile probe.\n' +
                              (ROOT / 'abi/generated/paperpaw_abi.rs').read_text())
            for target in (None, 'riscv32imc-unknown-none-elf'):
                with self.subTest(target=target):
                    args = ['rustc', '--edition=2024', '--crate-type=lib',
                            '-Dwarnings', '-Dmissing-docs', '--crate-name=abi_layout',
                            str(source), '--out-dir', str(base)]
                    if target:
                        args.extend(['--target', target])
                    subprocess.run(args, check=True)


if __name__ == '__main__':
    unittest.main()
