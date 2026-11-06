use paperpaw_core::native_host::events::{self, PageTurn};
#[path = "../../declarations.rs"]
mod declarations;
pub use declarations::{settings, ui};

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
