//! What File manager declares to the host. The settings are pinned byte for
//! byte (`testdata/declarations.bin.v1`, as the build script checks them) and,
//! with the screen, the action, the menu entry and the icon, compared with
//! what interface 6 registered in `parity.rs`.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "the build script also uses the builders")]
mod declarations;
use paperpaw_core::native_host::registrations::records_decode;

type Build = fn(
    &mut dyn FnMut(&[u8]) -> Result<(), i32>,
) -> Result<(), paperpaw_sdk::native::declare::DeclareError>;

fn batches(build: Build) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    build(&mut |batch: &[u8]| {
        out.push(batch.to_vec());
        Ok(())
    })
    .unwrap();
    out
}

#[test]
fn the_settings_are_the_golden() {
    let mut framed = Vec::new();
    for batch in batches(|sink| declarations::settings(sink)) {
        framed.extend_from_slice(&u32::try_from(batch.len()).unwrap().to_le_bytes());
        framed.extend_from_slice(&batch);
    }
    assert!(
        framed == include_bytes!("../testdata/declarations.bin.v1"),
        "the settings changed: read the difference and bless the golden on purpose"
    );
}

/// Every record is well formed for the host, and the host admits every one of
/// them: none is refused as unsupported.
#[test]
fn every_record_is_valid_for_the_host_and_admitted() {
    use paperpaw_core::native_host::declarations::validate_record;
    use paperpaw_native::host_glue::ERR_UNSUPPORTED;
    let builders: [(&str, Build); 2] = [
        ("settings", |sink| declarations::settings(sink)),
        ("ui", |sink| declarations::ui(sink)),
    ];
    let mut waiting = Vec::new();
    for (name, build) in builders {
        for batch in batches(build) {
            for record in records_decode(&batch).unwrap().iter() {
                match validate_record("file-manager", &record) {
                    Ok(()) => {}
                    Err(ERR_UNSUPPORTED) => {
                        let text = format!("{record:?}");
                        waiting.push(format!("{name}: {}", text.split('(').next().unwrap()));
                    }
                    Err(error) => panic!("{name}: {record:?}: {error}"),
                }
            }
        }
    }
    // The menu entry and the icon, as for every plugin of this port.
    assert!(
        waiting.is_empty(),
        "the host admits every record now: {waiting:?}"
    );
}

/// The screen, the action, the Tools menu entry (order 45) and the icon are as
/// interface 6 registered them (the values were compared before it was removed).
#[test]
fn the_screen_action_menu_entry_and_icon_are_as_interface_6_registered_them() {
    use paperpaw_core::native_host::registrations::Record;
    let (mut screens, mut actions, mut menus, mut icons) = (vec![], vec![], vec![], vec![]);
    for batch in batches(|sink| declarations::ui(sink)) {
        for record in records_decode(&batch).unwrap().iter() {
            match record {
                Record::ScreenSpec(s) => {
                    screens.push((format!("file-manager.{}", s.name), s.title.to_string()));
                }
                Record::ActionSpec(a) => {
                    actions.push((format!("file-manager.{}", a.name), a.title.to_string()));
                }
                Record::MenuEntry(e) => {
                    menus.push((e.menu.to_string(), e.item.to_string(), e.order));
                }
                Record::IconSpec(i) => icons.push((i.item.to_string(), i.icon.to_string())),
                other => panic!("{other:?}"),
            }
        }
    }
    assert_eq!(
        screens,
        [(
            "file-manager.browser".to_string(),
            "file-manager.title".to_string()
        )]
    );
    assert_eq!(
        actions,
        [(
            "file-manager.open".to_string(),
            "file-manager.title".to_string()
        )]
    );
    assert_eq!(
        menus,
        [(
            "core.tools".to_string(),
            "file-manager.open".to_string(),
            45
        )]
    );
    assert_eq!(
        icons,
        [("file-manager.open".to_string(), "folder".to_string())]
    );
}
