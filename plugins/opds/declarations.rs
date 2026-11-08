//! The catalogs' native declarations (slice 1): the two settings in the
//! library's part named after the catalogs (covers in lists, how many books a
//! sync fetches at most). Screens, actions, the menu entry and the sync follow
//! with their host pieces (`PORTING-7.md`).
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{Declarations, DeclareError, ExtraDecl, Kind, SpecDecl};

/// The part of the library settings the catalogs' settings are in.
const SECTION: &str = "opds.section";

/// Covers in lists, and how many books a sync fetches at most.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let group = groups::LIBRARY;
    let mut covers = SpecDecl::new(
        "covers",
        Kind::Bool(true),
        group.as_str(),
        "opds.setting.covers",
    );
    covers.help = "opds.setting.covers.help";
    d.spec(covers)?;
    let mut in_catalogs = ExtraDecl::new("covers");
    in_catalogs.section = SECTION;
    d.extra(in_catalogs)?;
    let mut limit = SpecDecl::new(
        "sync-limit",
        Kind::Int {
            default: 50,
            min: 10,
            max: 1000,
        },
        group.as_str(),
        "opds.setting.limit",
    );
    limit.help = "opds.setting.limit.help";
    d.spec(limit)?;
    let mut in_catalogs = ExtraDecl::new("sync-limit");
    in_catalogs.section = SECTION;
    in_catalogs.step = 10;
    d.extra(in_catalogs)?;
    d.submit()?;
    Ok(())
}
