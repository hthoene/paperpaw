//! Card messages are reclaimed only when no callback can still borrow them.

#![allow(clippy::unwrap_used, reason = "tests fail on unexpected errors")]

use alloc::{format, rc::Rc, string::String, vec::Vec};
use paperpaw_i18n::{Catalog, CatalogFile, MAX_READ};
use paperpaw_sdk::{ActionSpec, Command, Id, Manifest, Plugin, RegisterError, Registrar, Version};

use crate::{Kernel, tests::DEVICE};

struct File(Vec<u8>);

impl CatalogFile for File {
    fn size(&self) -> u32 {
        self.0.len() as u32
    }

    fn read_exact_at(&self, offset: u32, buf: &mut [u8]) -> bool {
        let Some(bytes) = self.0.get(offset as usize..offset as usize + buf.len()) else {
            return false;
        };
        buf.copy_from_slice(bytes);
        true
    }
}

const FILL: Id = Id::new_static("cache.fill");
const CHECK: Id = Id::new_static("cache.check");

struct Reader;

impl Plugin for Reader {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("cache"), "Cache", Version::new(1, 0, 0))
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_action(ActionSpec::new(FILL, "Fill").handler(|env, _, _| {
            for n in 0..1000 {
                let _ = env.i18n().tr(&format!("filler.{n}"));
            }
            assert!(env.i18n().read_bytes() >= MAX_READ);
        }))?;
        r.add_action(ActionSpec::new(CHECK, "Check").handler(|env, _, out| {
            assert_eq!(env.i18n().tr("late.notice"), "Translated notice");
            assert_eq!(env.i18n().tr("late.button"), "Translated button");
            out.push(Command::Notify(env.i18n().tr("late.notice").into()));
        }))
    }
}

fn kernel() -> Kernel {
    let mut source = String::from(
        "[meta]\nlanguage = es\n[messages]\nlate.notice = Translated notice\nlate.button = Translated button\n",
    );
    for n in 0..1000 {
        source.push_str(&format!(
            "filler.{n} = A translated message with enough text to fill the read cache\n"
        ));
    }
    let bytes = Catalog::compile(&source).unwrap();
    let mut kernel = Kernel::builder(DEVICE).plugin(Reader).build().unwrap();
    kernel
        .shared
        .translator
        .add(Catalog::open(Rc::new(File(bytes))).unwrap());
    kernel.shared.translator.set_language("es");
    kernel
}

#[test]
fn later_commands_translate_after_an_earlier_callback_fills_the_cache() {
    let mut kernel = kernel();
    kernel
        .output
        .commands
        .extend([Command::run(FILL), Command::run(CHECK)]);
    kernel.run_commands();
}

#[test]
fn a_cache_below_three_quarters_is_kept_between_commands() {
    let mut kernel = kernel();
    assert_eq!(kernel.translator().tr("late.notice"), "Translated notice");
    let bytes = kernel.translator().read_bytes();
    assert!(bytes > 0 && bytes < MAX_READ / 4 * 3);
    kernel.run_commands();
    assert_eq!(kernel.translator().read_bytes(), bytes);
    // Already read text retains its allocation as well as its value.
    let pointer = kernel.translator().tr("late.notice").as_ptr();
    kernel.run_commands();
    assert_eq!(kernel.translator().tr("late.notice").as_ptr(), pointer);
}

#[test]
fn a_full_cache_is_reclaimed_before_the_next_command_round() {
    let mut kernel = kernel();
    for n in 0..1000 {
        let _ = kernel.translator().tr(&format!("filler.{n}"));
    }
    assert!(kernel.translator().read_bytes() >= MAX_READ);
    // No command is needed: on_show and other work after the preceding
    // callback must start with room for translations too.
    kernel.run_commands();
    assert_eq!(kernel.translator().read_bytes(), 0);
    assert_eq!(kernel.translator().tr("late.button"), "Translated button");
}
