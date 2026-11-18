//! What the reading aids declare, compared with the interface-6 registration
//! by the declaration tests. The reader applies the rules (the `reader.filter`
//! stage, `READER-FILTER.md`); the plugin only declares the settings.
use paperpaw_sdk::native::declare::{Declarations, DeclareError, Kind, SpecDecl, SubscribeDecl};

/// The picture choices, in the order the page lists them.
pub const PICTURES: [&str; 3] = [
    "aids.pictures.show",
    "aids.pictures.caption",
    "aids.pictures.hide",
];

/// Bold word beginnings, words replaced, pictures.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let group = "core.group.reading-aids";
    let mut bold = SpecDecl::new(
        "bold-beginnings",
        Kind::Bool(false),
        group,
        "aids.setting.bold-beginnings",
    );
    bold.help = "aids.setting.bold-beginnings.help";
    d.spec(bold)?;
    let mut replace = SpecDecl::new("replace", Kind::Text(""), group, "aids.setting.replace");
    replace.help = "aids.setting.replace.help";
    replace.advanced = true;
    replace.kept_on_reset = true;
    d.spec(replace)?;
    let mut pictures = SpecDecl::new(
        "pictures",
        Kind::Choice {
            default: "aids.pictures.show",
            choices: &PICTURES,
        },
        group,
        "aids.setting.pictures",
    );
    pictures.help = "aids.setting.pictures.help";
    d.spec(pictures)?;
    d.submit()
}

/// The token of the subscription to the reader's rules stage.
pub const FILTER_TOKEN: u32 = 1;

/// The stage the reader asks for the rules of the three settings (see
/// `READER-FILTER.md`). The signature is the reply's own, which changes with
/// the settings; the subscription's stays 0.
pub fn subscriptions<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe(SubscribeDecl::new("reader.filter", FILTER_TOKEN))?;
    d.submit()
}
