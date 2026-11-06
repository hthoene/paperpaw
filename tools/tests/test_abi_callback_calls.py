"""Run native C plugin callbacks from a Rust caller through generated tables.

This linked fixture proves callback argument and result exchange on x86_64.
It does not exercise loader activation or the X4 calling convention.
"""
import os
from pathlib import Path
import platform
import shlex
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]

C_FIXTURE = r'''
#include "abi/generated/paperpaw-abi.h"

/* Storage and callbacks belong only to this linked test fixture. */
static struct {
    uint32_t event_token;
    unsigned event_calls;
    unsigned draw_calls;
} callback_state;

static int32_t dispatch_event(void *state, uint32_t token, PpBytes input) {
    static const uint8_t expected[] = {0x00, 0xff, 0x50, 0x80, 0x07};
    if (state != &callback_state) {
        return PP_ERR_INVALID;
    }
    ++callback_state.event_calls;
    if (token != UINT32_C(0xfedcba98) || input.ptr == NULL ||
        input.len != sizeof(expected)) {
        return PP_ERR_INVALID;
    }
    for (size_t i = 0; i < sizeof(expected); ++i) {
        if (input.ptr[i] != expected[i]) {
            return PP_ERR_INVALID;
        }
    }
    callback_state.event_token = token;
    return PP_ERR_CANCELLED;
}

static int32_t draw(void *state, uint32_t token) {
    if (state != &callback_state) {
        return PP_ERR_INVALID;
    }
    ++callback_state.draw_calls;
    if (token != UINT32_C(0x89abcdef) ||
        callback_state.event_token != UINT32_C(0xfedcba98)) {
        return PP_ERR_STATE;
    }
    return PP_ERR_BUSY;
}

static const PpPlugin plugin = {
    .size = sizeof(PpPlugin), .state = &callback_state, .drop = NULL,
    .dispatch_event = dispatch_event, .draw = draw
};
/* Existing designated initializers leave optional callbacks null. */
static const PpPlugin prefix_only = {
    .size = sizeof(PpPlugin), .state = &callback_state, .drop = NULL
};

const PpPlugin *probe_plugin(void) { return &plugin; }
const PpPlugin *probe_prefix_only(void) { return &prefix_only; }
void *probe_state(void) { return &callback_state; }
unsigned probe_event_calls(void) { return callback_state.event_calls; }
unsigned probe_draw_calls(void) { return callback_state.draw_calls; }
'''

RUST_CALLER = r'''
use core::ffi::{c_uint, c_void};
use core::mem::{offset_of, size_of};
use core::ptr::{addr_of, null_mut};

unsafe extern "C" {
    fn probe_plugin() -> *const PpPlugin;
    fn probe_prefix_only() -> *const PpPlugin;
    fn probe_state() -> *mut c_void;
    fn probe_event_calls() -> c_uint;
    fn probe_draw_calls() -> c_uint;
}

fn main() {
    // C owns valid static table storage throughout this executable.
    unsafe {
        let plugin = probe_plugin();
        assert!(!plugin.is_null());
        let bytes = addr_of!((*plugin).size).read() as usize;
        assert!(bytes >= offset_of!(PpPlugin, draw)
                + size_of::<Option<unsafe extern "C" fn(*mut c_void, u32) -> i32>>());
        let state = addr_of!((*plugin).state).read();
        assert!(!state.is_null());
        assert_eq!(state, probe_state());
        let dispatch = addr_of!((*plugin).dispatch_event).read().expect("fixture dispatch");
        let draw = addr_of!((*plugin).draw).read().expect("fixture draw");
        let input = [0x00, 0xff, 0x50, 0x80, 0x07];
        let bytes = PpBytes { ptr: input.as_ptr(), len: input.len() };
        assert_eq!(probe_event_calls(), 0);
        assert_eq!(probe_draw_calls(), 0);
        assert_eq!(dispatch(state, 0xfedcba98, bytes), ERR_CANCELLED);
        assert_eq!(probe_event_calls(), 1);
        assert_eq!(draw(state, 0x89abcdef), ERR_BUSY);
        assert_eq!(probe_draw_calls(), 1);

        // C checks the opaque state, full-width tokens, length and binary bytes.
        assert_eq!(dispatch(null_mut(), 0xfedcba98, bytes), ERR_INVALID);
        assert_eq!(dispatch(state, 0x76543210, bytes), ERR_INVALID);
        let short = PpBytes { ptr: input.as_ptr(), len: input.len() - 1 };
        assert_eq!(dispatch(state, 0xfedcba98, short), ERR_INVALID);
        let wrong = [0x00, 0xff, 0x50, 0x80, 0x08];
        let wrong = PpBytes { ptr: wrong.as_ptr(), len: wrong.len() };
        assert_eq!(dispatch(state, 0xfedcba98, wrong), ERR_INVALID);
        assert_eq!(draw(null_mut(), 0x89abcdef), ERR_INVALID);
        assert_eq!(draw(state, 0x01234567), ERR_STATE);
        assert_eq!(probe_event_calls(), 4);
        assert_eq!(probe_draw_calls(), 2);

        let prefix = probe_prefix_only();
        assert!(!prefix.is_null());
        assert_eq!(addr_of!((*prefix).size).read() as usize, size_of::<PpPlugin>());
        assert!(addr_of!((*prefix).dispatch_event).read().is_none());
        assert!(addr_of!((*prefix).draw).read().is_none());
    }
}
'''


class CallbackCallTests(unittest.TestCase):
    def test_native_c_dispatch_and_draw_called_from_rust(self):
        if platform.machine().lower() not in ('x86_64', 'amd64'):
            self.skipTest('Native callback probes currently cover x86_64 only')
        raw = ('#[allow(dead_code, reason = "The probe uses only callback types.")]\n'
               f'#[path = "{ROOT}/abi/generated/paperpaw_abi.rs"] mod raw;\n'
               'use raw::*;\n')
        with tempfile.TemporaryDirectory(prefix='paperpaw-callback-') as directory:
            base = Path(directory)
            (base / 'callee.c').write_text(C_FIXTURE)
            (base / 'caller.rs').write_text(raw + RUST_CALLER)
            subprocess.run(shlex.split(os.environ.get('CC', 'cc')) + [
                '-std=c11', '-Wall', '-Wextra', '-Werror', '-I', str(ROOT),
                '-c', str(base / 'callee.c'), '-o', str(base / 'callee.o')], check=True)
            executable = base / 'callback-probe'
            subprocess.run(['rustc', '--edition=2024', '-Dwarnings',
                            str(base / 'caller.rs'), '-o', str(executable),
                            '-C', f'link-arg={base / "callee.o"}'], check=True)
            subprocess.run([str(executable)], check=True)


if __name__ == '__main__':
    unittest.main()
