//! The SDK facade allocates nothing; the host fixture uses caller buffers too.
use super::*;
use core::ffi::c_void;
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes};
unsafe extern "C" fn open(_: *mut c_void, _: PpBytes, _: u16, _: u16, out: *mut u32) -> i32 {
    // SAFETY: service_open supplies aligned, exclusive output storage.
    unsafe {
        out.write(7);
    }
    abi::OK
}
unsafe extern "C" fn release(_: *mut c_void, _: u32) -> i32 {
    abi::OK
}
fn encode(record: &dyn wire::WireEncode, out: &mut [u8], used: &mut usize) -> Result<(), i32> {
    *used += wire::record_encode_of(record, &mut out[*used..])?;
    Ok(())
}
fn response(op: u32, out: &mut [u8]) -> Result<usize, i32> {
    let mut used = 0;
    match op {
        schema::OP_GET => encode(
            &wire::ServiceSecValue {
                flags: 1,
                label: "Label",
                value: "secret",
            },
            out,
            &mut used,
        )?,
        schema::OP_LIST => {
            for index in 0..32 {
                let digits = [b'a' + index / 10, b'0' + index % 10];
                let name = core::str::from_utf8(&digits).map_err(|_| abi::ERR_UTF8)?;
                encode(
                    &wire::ServiceSecEntry {
                        flags: 1,
                        plugin: "fixture",
                        name,
                        label: "Label",
                    },
                    out,
                    &mut used,
                )?;
            }
            encode(
                &wire::ServiceEnd {
                    flags: 1,
                    total: 32,
                    generation: 0,
                    next: 32,
                    cursor: None,
                },
                out,
                &mut used,
            )?;
        }
        schema::OP_RANDOM => encode(
            &wire::ServiceSecBytes {
                flags: 0,
                data: &[0x5a; 256],
            },
            out,
            &mut used,
        )?,
        schema::OP_KEEP | schema::OP_REMOVE => {}
        _ => return Err(abi::ERR_UNSUPPORTED),
    }
    Ok(used)
}
unsafe extern "C" fn call(_: *mut c_void, _: u32, op: u32, _: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: the SDK supplies exclusively borrowed output for the duration.
    let out = unsafe { core::slice::from_raw_parts_mut(out.ptr, out.cap) };
    match response(op, out) {
        Ok(n) => i32::try_from(n).unwrap(),
        Err(e) => e,
    }
}
#[test]
fn all_secret_operations_and_maximum_list_and_random_replies_allocate_nothing() {
    let mut table = abi::host_table();
    table.service_open = open;
    table.service_call = call;
    table.handle_release = release;
    let host = Host {
        table: &table,
        ctx: core::ptr::null_mut(),
    };
    let mut input = [0; 1280];
    let mut out = [0; 4096];
    crate::native::commands::menu_alloc_tests::no_alloc(|| {
        let secrets = Secrets::open(&host).unwrap();
        let value = secrets.get(&host, "token", &mut input, &mut out).unwrap();
        assert_eq!(value.value(), "secret");
        let page = secrets
            .list(&host, SecretList::all(), &mut input, &mut out)
            .unwrap();
        assert_eq!(page.count(), 32);
        let write = SecretWrite::new(SecretName::new("token").unwrap(), "value").unwrap();
        secrets.keep(&host, write, &mut input, &mut out).unwrap();
        assert!(input.iter().all(|byte| *byte == 0));
        secrets
            .remove(&host, write.target(), &mut input, &mut out)
            .unwrap();
        let random = secrets.random(&host, 256, &mut input, &mut out).unwrap();
        assert_eq!(random.bytes(), &[0x5a; 256]);
        assert!(!random.strong());
        secrets.release(&host).unwrap();
    });
}
