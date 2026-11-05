//! Scoped frame cleanup while an owner context remains pinned.
#[path = "../src/abi7/context.rs"]
mod context;
use context::{Error, Frame, FrameSlot};
use paperpaw_i18n::Translator;
use paperpaw_sdk::Settings;
use std::{cell::Cell, pin::pin};

struct View<'a> {
    settings: &'a Settings,
    translator: &'a Translator,
}
impl Frame for View<'_> {
    fn settings(&self) -> &Settings {
        self.settings
    }
    fn translator(&self) -> &Translator {
        self.translator
    }
}

#[test]
fn scoped_borrows_end_before_a_new_frame_is_installed() {
    let slot = pin!(FrameSlot::new());
    let settings = Settings::new();
    let translator = Translator::new();
    for _ in 0..3 {
        let view = View {
            settings: &settings,
            translator: &translator,
        };
        assert_eq!(slot.with(|_| ()), Err(Error::NoFrame));
        slot.as_ref()
            .scope(&view, || {
                slot.with(|frame| {
                    assert!(core::ptr::eq(frame.settings(), &settings));
                    assert_eq!(frame.translator().tr("missing.key"), "missing.key");
                })
                .unwrap();
            })
            .unwrap();
        assert_eq!(slot.with(|_| ()), Err(Error::NoFrame));
    }
}

#[test]
fn nested_installation_is_refused_without_running_the_callback() {
    let slot = pin!(FrameSlot::new());
    let settings = Settings::new();
    let translator = Translator::new();
    let view = View {
        settings: &settings,
        translator: &translator,
    };
    let called = Cell::new(false);
    slot.as_ref()
        .scope(&view, || {
            assert_eq!(
                slot.as_ref().scope(&view, || called.set(true)),
                Err(Error::Busy)
            );
            assert!(slot.with(|_| ()).is_ok());
        })
        .unwrap();
    assert!(!called.get());
}

#[test]
fn error_and_unwind_clear_the_frame() {
    let slot = pin!(FrameSlot::new());
    let settings = Settings::new();
    let translator = Translator::new();
    let view = View {
        settings: &settings,
        translator: &translator,
    };
    assert_eq!(slot.as_ref().scope(&view, || Err::<(), _>(7)), Ok(Err(7)));
    assert_eq!(slot.with(|_| ()), Err(Error::NoFrame));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        slot.as_ref()
            .scope(&view, || panic!("test dispatch unwind"))
            .unwrap();
    }));
    assert!(result.is_err());
    assert_eq!(slot.with(|_| ()), Err(Error::NoFrame));
}
