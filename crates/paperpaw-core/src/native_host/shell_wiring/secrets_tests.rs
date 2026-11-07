//! Production wiring through the private backend; cryptography stays in the shell.
use super::*;
use crate::{
    abi7::{context::Frame, services_wire},
    native_host::{
        admitted_features::AdmittedFeatures,
        host_services::Caller,
        registrations::{self as wire, Record},
        secrets_file,
        secrets_service::{self, SecretsHost},
    },
};
use core::cell::Cell;
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_IO, ERR_NO_SPACE, ERR_UNSUPPORTED};
use paperpaw_platform::{DirEntry, StorageError};
use paperpaw_sdk::Settings;

// A reversible test codec only. Production AES-GCM belongs to Firmware.
struct Codec;
impl ShellSealer for Codec {
    fn seals(&self) -> bool {
        true
    }
    fn seal(&self, aad: &[u8], plain: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        assert_eq!(aad, b"test\0token");
        let out = out.get_mut(..plain.len()).ok_or(ERR_NO_SPACE)?;
        for (out, byte) in out.iter_mut().zip(plain) {
            *out = *byte ^ 0xa5;
        }
        Ok(plain.len())
    }
    fn open(&self, aad: &[u8], sealed: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        self.seal(aad, sealed, out)
    }
}
struct Source(Cell<Result<bool, i32>>);
impl ShellRandom for Source {
    fn fill(&self, out: &mut [u8]) -> Result<bool, i32> {
        out.fill(0x5a);
        self.0.get()
    }
}
struct View {
    resources: HostResources,
    settings: Settings,
    translator: Translator,
}
impl View {
    fn new() -> Self {
        Self {
            resources: HostResources::try_new().unwrap(),
            settings: Settings::new(),
            translator: Translator::new(),
        }
    }
    fn call(&self, op: u32, row: Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
        let schema = services_wire::service_schema("core.secrets", 1).unwrap();
        let operation = schema.op(op).unwrap();
        let mut input = [0; 1280];
        let n = wire::records_encode_counted(&[row], 1, operation.max_input, &mut input)?;
        let who = Caller {
            owner: 1,
            plugin: "test",
            features: AdmittedFeatures::NONE,
            hosts: self.resources.hosts(),
        };
        secrets_service::backend(self, &who, operation, &input[..n], out)
    }
    fn get(&self, out: &mut [u8]) -> Result<usize, i32> {
        self.call(
            1,
            Record::ServiceSecName(wire::ServiceSecName {
                plugin: "",
                name: "token",
            }),
            out,
        )
    }
}
impl Frame for View {
    fn settings(&self) -> &Settings {
        &self.settings
    }
    fn translator(&self) -> &Translator {
        &self.translator
    }
}
#[test]
fn wired_storage_and_shell_sealing_are_used_by_secret_calls() {
    let view = View::new();
    let card = Rc::new(MemoryStorage::new());
    let sealer = Rc::new(Codec);
    let weak = Rc::downgrade(&sealer);
    view.resources
        .wire(HostWiring::new().secrets(SecretsInputs::new(card.clone(), sealer.clone())))
        .unwrap();
    drop(sealer);
    assert!(weak.upgrade().is_some());
    let mut out = [0; 1280];
    assert_eq!(
        view.call(
            3,
            Record::ServiceSecKeep(wire::ServiceSecKeep {
                plugin: "",
                name: "token",
                label: "Token",
                flags: 0,
                value: "value",
            }),
            &mut out
        ),
        Ok(0)
    );
    let mut file = [0; 128];
    let n = card.read_at(secrets_file::FILE, 0, &mut file).unwrap();
    let file = core::str::from_utf8(&file[..n]).unwrap();
    assert!(file.contains("s1:"));
    assert!(!file.contains("value"));
    let n = view.get(&mut out).unwrap();
    assert!(
        matches!(wire::record_decode_one(&out[..n]).unwrap(),Record::ServiceSecValue(v) if v.value=="value" && v.label=="Token")
    );
    drop(view);
    assert!(weak.upgrade().is_none());
}
#[test]
fn veiled_storage_is_explicit_and_has_no_random_fallback() {
    let view = View::new();
    let inputs = SecretsInputs::veiled(storage());
    assert!(!SecretsHost::sealer(&inputs).seals());
    view.resources
        .wire(HostWiring::new().secrets(inputs))
        .unwrap();
    let mut out = [0; 1280];
    assert_eq!(
        view.call(
            5,
            Record::ServiceSecRandom(wire::ServiceSecRandom { len: 8 }),
            &mut out
        ),
        Err(ERR_UNSUPPORTED)
    );
    assert_eq!(
        view.call(
            3,
            Record::ServiceSecKeep(wire::ServiceSecKeep {
                plugin: "",
                name: "token",
                label: "",
                flags: 0,
                value: "legacy",
            }),
            &mut out
        ),
        Ok(0)
    );
    let n = view.get(&mut out).unwrap();
    assert!(
        matches!(wire::record_decode_one(&out[..n]).unwrap(),Record::ServiceSecValue(v) if v.value=="legacy")
    );
}
#[test]
fn random_strength_and_errors_pass_through_without_substitution() {
    let view = View::new();
    let source = Rc::new(Source(Cell::new(Ok(false))));
    view.resources
        .wire(HostWiring::new().secrets(SecretsInputs::veiled(storage()).random(source.clone())))
        .unwrap();
    let mut out = [0; 512];
    for strong in [false, true] {
        source.0.set(Ok(strong));
        let n = view
            .call(
                5,
                Record::ServiceSecRandom(wire::ServiceSecRandom { len: 8 }),
                &mut out,
            )
            .unwrap();
        assert!(
            matches!(wire::record_decode_one(&out[..n]).unwrap(),Record::ServiceSecBytes(v) if v.data==[0x5a;8] && v.flags==u32::from(strong))
        );
    }
    source.0.set(Err(ERR_IO));
    out.fill(0xcc);
    assert_eq!(
        view.call(
            5,
            Record::ServiceSecRandom(wire::ServiceSecRandom { len: 8 }),
            &mut out
        ),
        Err(ERR_IO)
    );
    assert_eq!(out, [0xcc; 512]);
}
#[test]
fn secrets_double_wiring_is_refused_before_any_named_family_is_set() {
    let resources = HostResources::try_new().unwrap();
    resources
        .wire(HostWiring::new().secrets(SecretsInputs::veiled(storage())))
        .unwrap();
    let again = resources.wire(
        HostWiring::new()
            .secrets(SecretsInputs::veiled(storage()))
            .device(Rc::new(Nothing)),
    );
    assert_eq!(again.err(), Some(ERR_STATE));
    assert!(resources.hosts().device().is_none());
    let resources = HostResources::try_new().unwrap();
    resources
        .wire(HostWiring::new().hyphenation(storage()))
        .unwrap();
    let again = resources.wire(
        HostWiring::new()
            .hyphenation(storage())
            .secrets(SecretsInputs::veiled(storage())),
    );
    assert_eq!(again.err(), Some(ERR_STATE));
    assert!(resources.hosts().secrets().is_none());
}
struct Unreadable;
impl Storage for Unreadable {
    fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
        Err(StorageError::Io)
    }
    fn size(&self, _: &str) -> Result<u64, StorageError> {
        Err(StorageError::Io)
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Io)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Io)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Io)
    }
    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Io)
    }
}
#[test]
fn unreadable_notice_is_only_called_once_by_the_service() {
    let view = View::new();
    let notices = Rc::new(Cell::new(0));
    let counted = notices.clone();
    view.resources
        .wire(
            HostWiring::new().secrets(
                SecretsInputs::veiled(Rc::new(Unreadable))
                    .unreadable(Rc::new(move || counted.set(counted.get() + 1))),
            ),
        )
        .unwrap();
    for _ in 0..2 {
        assert_eq!(view.get(&mut [0; 1280]), Err(ERR_IO));
    }
    assert_eq!(notices.get(), 1);
}
struct Refusing;
impl ShellSealer for Refusing {
    fn seals(&self) -> bool {
        true
    }
    fn seal(&self, _: &[u8], _: &[u8], _: &mut [u8]) -> Result<usize, i32> {
        Err(ERR_DENIED)
    }
    fn open(&self, _: &[u8], _: &[u8], _: &mut [u8]) -> Result<usize, i32> {
        Err(ERR_IO)
    }
}
#[test]
fn sealer_refusal_preserves_the_existing_file_and_debug_prints_no_objects() {
    let view = View::new();
    let card = Rc::new(MemoryStorage::new());
    card.write(secrets_file::FILE, b"opaque\n").unwrap();
    let inputs = SecretsInputs::new(card.clone(), Rc::new(Refusing));
    assert_eq!(
        SecretsHost::sealer(&inputs).open(b"", b"", &mut [0; 16]),
        Err(ERR_IO)
    );
    assert_eq!(
        alloc::format!("{inputs:?}"),
        "SecretsInputs { custom_sealer: true, random: false, unreadable: false }"
    );
    view.resources
        .wire(HostWiring::new().secrets(inputs))
        .unwrap();
    assert_eq!(
        view.call(
            3,
            Record::ServiceSecKeep(wire::ServiceSecKeep {
                plugin: "",
                name: "token",
                label: "",
                flags: 0,
                value: "never-save",
            }),
            &mut [0; 1280]
        ),
        Err(ERR_DENIED)
    );
    let mut bytes = [0; 32];
    let n = card.read_at(secrets_file::FILE, 0, &mut bytes).unwrap();
    assert_eq!(&bytes[..n], b"opaque\n");
}
