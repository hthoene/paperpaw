use paperpaw_core::native_host::events::{self, PageTurn};
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{Declarations, DeclareError, Kind, SpecDecl};

pub fn settings(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    let mut enabled = SpecDecl::new(
        "enabled",
        Kind::Bool(true),
        "power.group",
        "battery.setting.enabled",
    );
    enabled.help = "battery.setting.enabled.help";
    declarations.spec(enabled)?;
    let developer_group = groups::DEVELOPER;
    let mut by_plugin = SpecDecl::new(
        "by-plugin",
        Kind::Bool(true),
        developer_group.as_str(),
        "battery.setting.by-plugin",
    );
    by_plugin.help = "battery.setting.by-plugin.help";
    declarations.spec(by_plugin)?;
    declarations.submit()
}

pub struct EventFixture {
    pub name: &'static str,
    pub bytes: Vec<u8>,
}

type Encoder = fn(u64, &mut [u8]) -> Result<usize, i32>;

pub fn lifecycle(now_ms: u64) -> Result<Vec<EventFixture>, i32> {
    let encoders: [(&str, Encoder); 5] = [
        ("started", events::encode_started),
        ("resume", events::encode_resume),
        ("suspend", events::encode_suspend),
        ("stopping", events::encode_stopping),
        ("battery-measured", events::encode_battery_measured),
    ];
    encoders
        .into_iter()
        .map(|(name, encode)| {
            let mut bytes = [0; 32];
            let length = encode(now_ms, &mut bytes)?;
            Ok(EventFixture {
                name,
                bytes: bytes[..length].to_vec(),
            })
        })
        .collect()
}

pub fn page_turn(now_ms: u64, page: Option<u32>, pages: Option<u32>) -> Result<Vec<u8>, i32> {
    let reading = PageTurn {
        now_ms,
        section: 0,
        offset: 12,
        progress_ppm: 500_000,
        path: "books/fixture.txt",
        title: "Fixture",
        chapter: "",
        page,
        pages,
    };
    let mut bytes = [0; 4096];
    let length = events::encode_page_turned(&reading, &mut bytes)?;
    Ok(bytes[..length].to_vec())
}
