//! The wireless library's native declarations (slice 1): the setting that
//! says where Calibre is when it is not found by itself. The screen, the
//! tools entry and the icon follow with the screen (`PORTING-7.md`).
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{Declarations, DeclareError, Kind, SpecDecl};

/// Calibre's address, where it is not found by itself.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let group = groups::LIBRARY;
    let mut address = SpecDecl::new(
        "address",
        Kind::Text(""),
        group.as_str(),
        "calibre.setting.address",
    );
    address.help = "calibre.setting.address.help";
    address.advanced = true;
    d.spec(address)?;
    d.submit()?;
    Ok(())
}
