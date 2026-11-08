//! Protected value decisions use host identity and preserve output on refusals.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::{
    context::{Frame, FrameSlot},
    queries::settings_read_for,
    settings_visibility::*,
};
use crate::abi7::settings_wire::{SettingValueRef, settings_decode};
use core::pin::pin;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_STATE, ERR_UNSUPPORTED, PpBuffer, PpBytes};
use paperpaw_sdk::{Id, SettingExtra, SettingSpec, SettingValue, builtin};

struct View<'a> {
    env: &'a paperpaw_sdk::Env<'a>,
    editor: Option<&'a str>,
}
impl Frame for View<'_> {
    fn settings(&self) -> &paperpaw_sdk::Settings {
        self.env.settings()
    }
    fn translator(&self) -> &paperpaw_i18n::Translator {
        self.env.i18n()
    }
    fn settings_editor(&self) -> Option<&str> {
        self.editor
    }
}

fn read(slot: &FrameSlot, caller: &Id, key: &str, out: &mut [u8]) -> i32 {
    // SAFETY: borrowed input/output are disjoint and live for the synchronous read.
    unsafe {
        settings_read_for(
            slot,
            caller,
            PpBytes {
                ptr: key.as_ptr(),
                len: key.len(),
            },
            PpBuffer {
                ptr: out.as_mut_ptr(),
                cap: out.len(),
            },
        )
    }
}

#[test]
fn predicate_redacts_every_secret_and_hides_grants_except_from_host_editor() {
    for caller in [
        Id::new_static("own"),
        Id::new_static("other"),
        Id::new_static("editor"),
    ] {
        for key in ["core.grant.style.own", "core.grant.style.other"] {
            let expected = if caller.as_str() == "editor" {
                Visibility::Readable
            } else {
                Visibility::Hidden
            };
            assert_eq!(visibility(&caller, key, false, Some("editor")), expected);
        }
        for key in ["own.secret", "other.secret"] {
            assert_eq!(
                visibility(&caller, key, true, Some("editor")),
                Visibility::Redacted
            );
        }
        assert_eq!(
            visibility(&caller, "own.public", false, Some("editor")),
            Visibility::Readable
        );
    }
    assert_eq!(
        visibility(
            &Id::new_static("core"),
            "core.grant.style.own",
            false,
            Some("editor")
        ),
        Visibility::Hidden
    );
}

#[test]
fn slot_five_uses_identity_for_short_grants_and_keeps_own_secret_behavior() {
    static SECRET: SettingExtra = SettingExtra::new().secret();
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    for spec in [
        SettingSpec::switch(
            "core.grant.style.own",
            "grant",
            false,
            builtin::groups::OTHER,
        ),
        SettingSpec::switch(
            "core.grant.style.other",
            "grant",
            false,
            builtin::groups::OTHER,
        ),
        SettingSpec::switch("own.public", "public", false, builtin::groups::OTHER),
        SettingSpec::text(
            "own.secret",
            "secret",
            "own-default",
            builtin::groups::OTHER,
        )
        .with_extra(&SECRET),
        SettingSpec::text(
            "other.secret",
            "secret",
            "other-default",
            builtin::groups::OTHER,
        )
        .with_extra(&SECRET),
    ] {
        kernel.shared.settings.define(spec).unwrap();
    }
    kernel
        .shared
        .settings
        .set("core.grant.style.own", SettingValue::Bool(true))
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let frame = View {
        env: &env,
        editor: Some("editor"),
    };
    let slot = pin!(FrameSlot::new());
    let own = Id::new_static("own");
    let other = Id::new_static("other");
    let mut out = [71; 128];
    assert_eq!(read(&slot, &own, "own.public", &mut out), ERR_STATE);
    assert_eq!(out, [71; 128]);
    slot.as_ref()
        .scope(&frame, || {
            for (caller, key, expected) in [
                (&own, "core.grant.style", SettingValueRef::Bool(true)),
                (&other, "core.grant.style", SettingValueRef::Bool(false)),
                (&own, "own.secret", SettingValueRef::Text("own-default")),
                (
                    &other,
                    "other.secret",
                    SettingValueRef::Text("other-default"),
                ),
                (&other, "own.public", SettingValueRef::Bool(false)),
            ] {
                let len = usize::try_from(read(&slot, caller, key, &mut out)).unwrap();
                assert_eq!(settings_decode(&out[..len]).unwrap(), expected);
            }
            for (key, expected) in [
                ("core.grant.style.own", ERR_DENIED),
                ("core.grant.style.other", ERR_DENIED),
                ("other.secret", ERR_DENIED),
                ("core.grant.unknown", ERR_UNSUPPORTED),
            ] {
                out.fill(71);
                assert_eq!(read(&slot, &own, key, &mut out), expected);
                assert_eq!(out, [71; 128]);
            }
            let editor = Id::new_static("editor");
            let len =
                usize::try_from(read(&slot, &editor, "core.grant.style.own", &mut out)).unwrap();
            assert_eq!(
                settings_decode(&out[..len]).unwrap(),
                SettingValueRef::Bool(true)
            );
            out.fill(71);
            assert_eq!(read(&slot, &editor, "own.secret", &mut out), ERR_DENIED);
            assert_eq!(out, [71; 128]);
        })
        .unwrap();
}

#[test]
fn placed_setting_identity_cannot_alias_grants_or_foreign_secrets() {
    use paperpaw_native::host_glue::ERR_INVALID;
    static SECRET: SettingExtra = SettingExtra::new().secret();
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    for spec in [
        SettingSpec::text("own.secret", "Secret", "own-value", builtin::groups::OTHER)
            .with_extra(&SECRET),
        SettingSpec::text(
            "other.secret",
            "Secret",
            "foreign-value",
            builtin::groups::OTHER,
        )
        .with_extra(&SECRET),
        SettingSpec::switch("own.public", "Public", false, builtin::groups::OTHER),
        SettingSpec::switch(
            "core.grant.style.own",
            "Grant",
            true,
            builtin::groups::OTHER,
        ),
        SettingSpec::switch(
            "core.grant.style.other",
            "Grant",
            true,
            builtin::groups::OTHER,
        ),
    ] {
        kernel.shared.settings.define(spec).unwrap();
    }
    kernel
        .shared
        .settings
        .set("own@1.secret", SettingValue::Text("placed-value".into()))
        .unwrap();
    kernel
        .shared
        .settings
        .set("own.public@2", SettingValue::Bool(true))
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let frame = View {
        env: &env,
        editor: None,
    };
    let slot = pin!(FrameSlot::new());
    let caller = Id::new_static("own");
    slot.as_ref()
        .scope(&frame, || {
            for (key, expected) in [
                ("core.grant.style.other", ERR_DENIED),
                ("core@1.grant.style.other", ERR_DENIED),
                ("core.grant@1.style.other", ERR_DENIED),
                ("other.secret", ERR_DENIED),
                ("other@1.secret", ERR_DENIED),
                ("other.secret@2", ERR_DENIED),
                ("other@1.secret@2", ERR_INVALID),
                ("own@1.secret@2", ERR_INVALID),
                ("own@1.public@2", ERR_INVALID),
                ("core@1.grant@2.style.other", ERR_INVALID),
                ("own@.secret", ERR_INVALID),
                ("own.secret@1@2", ERR_INVALID),
            ] {
                let mut out = [71; 128];
                assert_eq!(read(&slot, &caller, key, &mut out), expected, "{key}");
                assert_eq!(out, [71; 128], "{key}");
            }
            for (key, expected) in [
                ("own@1.secret", SettingValueRef::Text("placed-value")),
                ("own.secret@2", SettingValueRef::Text("own-value")),
                ("own.public@2", SettingValueRef::Bool(true)),
                ("own@2.public", SettingValueRef::Bool(false)),
                ("core@1.grant.style", SettingValueRef::Bool(true)),
            ] {
                let mut out = [71; 128];
                let len = usize::try_from(read(&slot, &caller, key, &mut out)).unwrap();
                assert_eq!(settings_decode(&out[..len]).unwrap(), expected, "{key}");
            }
        })
        .unwrap();
}
