//! The plugin store's native declarations (slice 1): its two settings, the
//! library's address (for developers and libraries of one's own) and whether
//! it lists community plugins (guarded: no other plugin's defaults, presets or
//! commands switch it on). The screens, the actions, the menu entry, the
//! downloads, the install and the first-run offer follow with their host pieces
//! (`PORTING-7.md`).
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{Declarations, DeclareError, Kind, SpecDecl};

/// PaperPaw's own plugin library.
pub const OFFICIAL_LIST: &str = "https://paperpaw.org/api/v1/plugins.json";

/// The library's address, and the switch for community plugins.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let developer = groups::DEVELOPER;
    let system = groups::SYSTEM;
    let mut catalog = SpecDecl::new(
        "catalog",
        Kind::Text(OFFICIAL_LIST),
        developer.as_str(),
        "store.setting.catalog",
    );
    catalog.help = "store.setting.catalog.help";
    d.spec(catalog)?;
    let mut community = SpecDecl::new(
        "community",
        Kind::Bool(false),
        system.as_str(),
        "store.setting.community",
    );
    community.help = "store.setting.community.help";
    community.guarded = true;
    d.spec(community)?;
    d.submit()?;
    Ok(())
}
