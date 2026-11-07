#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use alloc::vec::Vec;
use core::{cell::Cell, mem::MaybeUninit, ptr};
use paperpaw_native::host_glue as abi;
use paperpaw_sdk::{Env, Settings};

struct Broker<'a> {
    env: &'a Env<'a>,
    calls: Cell<u32>,
    result: Result<usize, i32>,
    allocates: bool,
}
impl Frame for Broker<'_> {
    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn translator(&self) -> &paperpaw_i18n::Translator {
        self.env.i18n()
    }
    fn stage_run(
        &self,
        caller: &str,
        stage: &str,
        cursor: u32,
        input: &[u8],
        out: &mut [MaybeUninit<u8>],
    ) -> Result<usize, i32> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(
            (caller, stage, cursor, input),
            ("owner", "owner.insert", 2, &b"input"[..])
        );
        if self.allocates {
            let mut allocation = Vec::<u8>::new();
            allocation
                .try_reserve_exact(1)
                .map_err(|_| abi::ERR_NO_MEMORY)?;
        }
        if let Ok(count) = self.result
            && count <= out.len()
        {
            for (index, slot) in out[..count].iter_mut().enumerate() {
                slot.write(b"rpc".get(index).copied().unwrap_or(0));
            }
        }
        self.result
    }
}
fn pair(bytes: &[u8]) -> PpBytes {
    PpBytes {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

#[test]
fn forwards_identity_cursor_and_payload_and_publishes_only_success() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    for result in [
        Ok(3),
        Ok(9),
        Err(abi::ERR_DENIED),
        Err(abi::ERR_NO_SPACE),
        Err(abi::ERR_STATE),
        Err(abi::ERR_NOT_FOUND),
        Err(abi::ERR_NO_MEMORY),
    ] {
        let broker = Broker {
            env: &env,
            calls: Cell::new(0),
            result,
            allocates: false,
        };
        let mut out = [0xa5; 8];
        // SAFETY: bounded readable inputs and disjoint exclusive output are live.
        let got = unsafe {
            in_frame(
                &broker,
                "owner",
                pair(b"owner.insert"),
                2,
                pair(b"input"),
                PpBuffer {
                    ptr: out.as_mut_ptr(),
                    cap: out.len(),
                },
            )
        };
        assert_eq!(
            got,
            if result == Ok(9) {
                Err(ERR_INVALID)
            } else {
                result
            }
        );
        assert_eq!(broker.calls.get(), 1);
        if got == Ok(3) {
            assert_eq!(&out[..3], b"rpc");
            assert_eq!(&out[3..], &[0xa5; 5]);
        } else {
            assert_eq!(out, [0xa5; 8]);
        }
    }
}

#[test]
fn accepts_uninitialized_caller_output_without_reading_it() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let broker = Broker {
        env: &env,
        calls: Cell::new(0),
        result: Ok(3),
        allocates: false,
    };
    let mut out = [MaybeUninit::<u8>::uninit(); 8];
    // SAFETY: output permits uninitialized writable bytes and does not alias input.
    assert_eq!(
        unsafe {
            in_frame(
                &broker,
                "owner",
                pair(b"owner.insert"),
                2,
                pair(b"input"),
                PpBuffer {
                    ptr: out.as_mut_ptr().cast(),
                    cap: out.len(),
                },
            )
        },
        Ok(3)
    );
    // SAFETY: only the returned prefix was initialized, and only it is read.
    assert_eq!(
        out[..3]
            .iter()
            .map(|v| unsafe { v.assume_init() })
            .collect::<Vec<_>>(),
        b"rpc"
    );
}

#[test]
fn bad_ids_and_buffers_never_reach_the_broker_or_change_output() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let broker = Broker {
        env: &env,
        calls: Cell::new(0),
        result: Ok(3),
        allocates: false,
    };
    for (stage, expected) in [
        (b"".as_slice(), ERR_INVALID),
        (b"owner..insert", ERR_INVALID),
        (b"Owner.insert", ERR_INVALID),
        (b"owner/insert", ERR_INVALID),
        (b"\xff", abi::ERR_UTF8),
        (&[b'x'; 129], ERR_INVALID),
    ] {
        let mut out = [0xa5; 8];
        // SAFETY: readable inputs and exclusive output are live, despite invalid ids.
        assert_eq!(
            unsafe {
                in_frame(
                    &broker,
                    "owner",
                    pair(stage),
                    2,
                    pair(b"input"),
                    PpBuffer {
                        ptr: out.as_mut_ptr(),
                        cap: out.len(),
                    },
                )
            },
            Err(expected)
        );
        assert_eq!(out, [0xa5; 8]);
    }
    for (input, capacity) in [
        (
            PpBytes {
                ptr: ptr::null(),
                len: 1,
            },
            8,
        ),
        (
            PpBytes {
                ptr: ptr::NonNull::dangling().as_ptr(),
                len: 4097,
            },
            8,
        ),
        (pair(b"input"), 4097),
    ] {
        let mut out = [0xa5; 8];
        // SAFETY: invalid numeric pairs are rejected before dereferencing them.
        assert_eq!(
            unsafe {
                in_frame(
                    &broker,
                    "owner",
                    pair(b"owner.insert"),
                    2,
                    input,
                    PpBuffer {
                        ptr: out.as_mut_ptr(),
                        cap: capacity,
                    },
                )
            },
            Err(ERR_INVALID)
        );
        assert_eq!(out, [0xa5; 8]);
    }
    assert_eq!(broker.calls.get(), 0);
    // SAFETY: null ctx is refused before examining other arguments.
    assert_eq!(
        unsafe {
            run::<()>(
                ptr::null_mut(),
                pair(b"owner.insert"),
                2,
                pair(b"input"),
                PpBuffer {
                    ptr: ptr::null_mut(),
                    cap: 0,
                },
            )
        },
        ERR_INVALID
    );
}

#[test]
fn ordinary_query_frames_refuse_stage_execution() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let mut out = [0xa5; 8];
    // SAFETY: all bounded pairs are live and disjoint.
    assert_eq!(
        unsafe {
            in_frame(
                &env,
                "owner",
                pair(b"owner.insert"),
                2,
                pair(b"input"),
                PpBuffer {
                    ptr: out.as_mut_ptr(),
                    cap: out.len(),
                },
            )
        },
        Err(abi::ERR_STATE)
    );
    assert_eq!(out, [0xa5; 8]);
}

#[test]
fn every_pair_overlap_is_refused_before_forming_slices() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let broker = Broker {
        env: &env,
        calls: Cell::new(0),
        result: Ok(3),
        allocates: false,
    };
    let mut shared = *b"owner.insert........";
    let original = shared;
    let raw = shared.as_mut_ptr();
    for (stage, input, out) in [
        (
            pair(b"owner.insert"),
            PpBytes { ptr: raw, len: 5 },
            PpBuffer { ptr: raw, cap: 8 },
        ),
        (
            PpBytes { ptr: raw, len: 12 },
            pair(b"input"),
            PpBuffer { ptr: raw, cap: 8 },
        ),
        (
            PpBytes { ptr: raw, len: 12 },
            PpBytes { ptr: raw, len: 5 },
            PpBuffer {
                ptr: ptr::null_mut(),
                cap: 0,
            },
        ),
        (
            pair(b"owner.insert"),
            PpBytes { ptr: raw, len: 5 },
            PpBuffer {
                ptr: raw.wrapping_add(4),
                cap: 8,
            },
        ),
    ] {
        // SAFETY: storage is live; deliberate overlaps are rejected before slices exist.
        assert_eq!(
            unsafe { in_frame(&broker, "owner", stage, 2, input, out) },
            Err(ERR_INVALID)
        );
        assert_eq!(shared, original);
    }
    assert_eq!(broker.calls.get(), 0);
}

#[test]
fn broker_handles_oom_and_null_empty_output_without_slot_allocation() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let broker = Broker {
        env: &env,
        calls: Cell::new(0),
        result: Ok(3),
        allocates: true,
    };
    let mut out = [0xa5; 8];
    super::super::tests::arm_failure(1);
    // SAFETY: all bounded pairs are live and disjoint.
    let refused = unsafe {
        in_frame(
            &broker,
            "owner",
            pair(b"owner.insert"),
            2,
            pair(b"input"),
            PpBuffer {
                ptr: out.as_mut_ptr(),
                cap: out.len(),
            },
        )
    };
    super::super::tests::arm_failure(0);
    assert_eq!(refused, Err(abi::ERR_NO_MEMORY));
    assert_eq!(broker.calls.get(), 1, "allocation failed inside the broker");
    assert_eq!(out, [0xa5; 8]);
    // SAFETY: fixture storage remains live; the next broker allocation succeeds.
    assert_eq!(
        unsafe {
            in_frame(
                &broker,
                "owner",
                pair(b"owner.insert"),
                2,
                pair(b"input"),
                PpBuffer {
                    ptr: out.as_mut_ptr(),
                    cap: out.len(),
                },
            )
        },
        Ok(3)
    );
    let empty_broker = Broker {
        env: &env,
        calls: Cell::new(0),
        result: Ok(0),
        allocates: false,
    };
    // SAFETY: null with capacity zero is valid and never dereferenced.
    assert_eq!(
        unsafe {
            in_frame(
                &empty_broker,
                "owner",
                pair(b"owner.insert"),
                2,
                pair(b"input"),
                PpBuffer {
                    ptr: ptr::null_mut(),
                    cap: 0,
                },
            )
        },
        Ok(0)
    );
    assert_eq!(empty_broker.calls.get(), 1);
}

#[test]
fn broker_can_initialize_exactly_the_output_capacity() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let broker = Broker {
        env: &env,
        calls: Cell::new(0),
        result: Ok(8),
        allocates: false,
    };
    let mut out = [MaybeUninit::<u8>::uninit(); 8];
    // SAFETY: all bounded pairs are live and disjoint; output starts uninitialized.
    assert_eq!(
        unsafe {
            in_frame(
                &broker,
                "owner",
                pair(b"owner.insert"),
                2,
                pair(b"input"),
                PpBuffer {
                    ptr: out.as_mut_ptr().cast(),
                    cap: out.len(),
                },
            )
        },
        Ok(8)
    );
    // SAFETY: the broker initialized all eight bytes reported above.
    assert_eq!(
        out.map(|byte| unsafe { byte.assume_init() }),
        *b"rpc\0\0\0\0\0"
    );
}
