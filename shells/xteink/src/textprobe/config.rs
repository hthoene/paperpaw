//! Bounded test command grammar; no user text or file paths are accepted.
#[derive(Debug, PartialEq, Eq)]
pub struct Config<'a> {
    pub case: &'a str,
    pub path: &'a str,
    pub cache: &'a str,
    pub heap: &'a str,
    pub repeats: u8,
    pub mode: &'a str,
}

impl<'a> Config<'a> {
    pub fn parse(line: &'a str) -> Option<Self> {
        let mut fields = line.split_ascii_whitespace();
        if fields.next()? != "textprobe" {
            return None;
        }
        let mut value = Self {
            repeats: 1,
            case: fields.next()?,
            path: fields.next()?,
            cache: fields.next()?,
            heap: fields.next()?,
            mode: fields.next()?,
        };
        if let Some(option) = fields.next() {
            if option != "repeat10" || value.cache != "warm" {
                return None;
            }
            value.repeats = 10;
        }
        (fields.next().is_none()
            && matches!(
                value.case,
                "ascii" | "hebrew" | "rlm-ascii" | "rlm-alternating" | "bitmap-ascii" | "bitmap-A"
            )
            && matches!(value.path, "direct" | "adapter" | "control")
            && matches!(value.cache, "cold" | "warm")
            && matches!(value.heap, "clean" | "fragmented")
            && matches!(value.mode, "raster" | "describing" | "blind" | "rtl-ink"))
        .then_some(value)
    }

    pub fn font(&self) -> &'static str {
        match self.case {
            "rlm-alternating" => "split-ascii-stored",
            "bitmap-ascii" | "bitmap-A" => "stored-fixture",
            _ => "synthetic-all",
        }
    }
}
