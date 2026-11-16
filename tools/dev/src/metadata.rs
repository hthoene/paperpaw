//! `metadata`: the record of what a plugin with code calls on the device.
//!
//! A plugin's `native/metadata.json` lists the device's functions its code
//! calls and the callbacks it has; the packer puts the record made from it
//! (`native/metadata.bin`, for the X4 Pro and X4 Classic `metadata.s3.bin`)
//! into the plugin's file, and a device reads from it whether it can run the
//! plugin; the emulator reads `metadata.host.bin` from the plugin built for
//! the computer. In PaperPaw's repository `tools/abi-metadata.py` makes the record
//! from the interface's schema; an author has neither. This makes the same
//! bytes from a table of that schema the tool carries (`metadata/table.txt`,
//! written by `tools/abi-metadata-table.py`).
//!
//! - `metadata [<plugin's folder>]` writes the records beside the list;
//! - `metadata --check [<plugin's folder>]` fails if one is missing or is not
//!   what the list gives.

use std::path::{Path, PathBuf};

use paperpaw_json::stream::{Event, Parser};

/// The table of the interface's schema this tool was built with.
const TABLE: &str = include_str!("metadata/table.txt");
/// The X4's and X3's chip: what a plugin's list is written for.
pub const X4_TARGET: &str = "esp32c3-riscv32imc-ilp32-soft-le";
/// The X4 Pro's and X4 Classic's chip: the same list, its own record.
pub const S3_TARGET: &str = "esp32s3-xtensa-lx7-ilp32-windowed-le";
/// The emulator on a computer (Linux, x86-64): the same list again.
pub const HOST_TARGET: &str = "linux-x86_64-sysv64-le";
/// The plugin's list, and the records made from it.
pub const LIST: &str = "native/metadata.json";
const RECORDS: [(&str, &str); 3] = [
    ("native/metadata.bin", X4_TARGET),
    ("native/metadata.s3.bin", S3_TARGET),
    ("native/metadata.host.bin", HOST_TARGET),
];

/// A name with numbers: a profile, an export, a service, a feature.
struct Row {
    name: String,
    values: Vec<(String, String)>,
}

impl Row {
    fn text(&self, key: &str) -> Result<&str, String> {
        self.values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .ok_or_else(|| format!("the table has no {key} for {}", self.name))
    }

    fn number(&self, key: &str) -> Result<u64, String> {
        let text = self.text(key)?;
        text.parse()
            .map_err(|_| format!("the table's {key} of {} is no number", self.name))
    }
}

/// The schema's part the record is made from.
#[derive(Default)]
struct Table {
    major: u64,
    minor: u64,
    limits: Vec<(String, String)>,
    /// `prefix`, `export`, `service`, `feature`: fields in order, each with
    /// its width in bytes.
    layouts: Vec<(String, Vec<(String, usize)>)>,
    profiles: Vec<Row>,
    exports: Vec<Row>,
    services: Vec<Row>,
    features: Vec<Row>,
    /// `host <name>`, `callback <name>`, `record <name>`,
    /// `field <record> <name>`, each with the minor it needs.
    needs: Vec<(String, u64)>,
}

fn pairs(parts: &[&str]) -> Vec<(String, String)> {
    parts
        .iter()
        .filter_map(|part| part.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

impl Table {
    fn built_in() -> Result<Self, String> {
        let broken = |line: &str| format!("the tool's own table is broken at `{line}`");
        let mut table = Self::default();
        for line in TABLE.lines().filter(|l| !l.starts_with('#')) {
            let parts: Vec<&str> = line.split(' ').collect();
            let row = |at: usize| Row {
                name: parts[at].to_string(),
                values: pairs(&parts[at + 1..]),
            };
            match (parts[0], parts.len()) {
                ("abi", 3) => {
                    table.major = parts[1].parse().map_err(|_| broken(line))?;
                    table.minor = parts[2].parse().map_err(|_| broken(line))?;
                }
                ("metadata", _) => table.limits = pairs(&parts[1..]),
                ("layout", 3..) => {
                    let mut fields = Vec::new();
                    for field in &parts[2..] {
                        let (name, kind) = field.split_once(':').ok_or_else(|| broken(line))?;
                        let width = match kind {
                            "u8" => 1,
                            "u16" => 2,
                            "u32" | "bytes4" => 4,
                            _ => return Err(broken(line)),
                        };
                        fields.push((name.to_string(), width));
                    }
                    table.layouts.push((parts[1].to_string(), fields));
                }
                ("profile", 2..) => table.profiles.push(row(1)),
                ("export", 2..) => table.exports.push(row(1)),
                ("service", 2..) => table.services.push(row(1)),
                ("feature", 2..) => table.features.push(row(1)),
                ("host" | "callback" | "record", 3) | ("field", 4) => {
                    let (name, minor) = line.rsplit_once(' ').ok_or_else(|| broken(line))?;
                    let minor = minor.parse().map_err(|_| broken(line))?;
                    table.needs.push((name.to_string(), minor));
                }
                _ => return Err(broken(line)),
            }
        }
        Ok(table)
    }

    fn limit(&self, key: &str) -> Result<usize, String> {
        self.limits
            .iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.parse().ok())
            .ok_or_else(|| format!("the tool's own table has no {key}"))
    }

    fn layout(&self, kind: &str) -> Result<&[(String, usize)], String> {
        self.layouts
            .iter()
            .find(|(name, _)| name == kind)
            .map(|(_, fields)| fields.as_slice())
            .ok_or_else(|| format!("the tool's own table has no layout of {kind}"))
    }

    /// The fields of `kind` in order, little-endian, each from `value`.
    fn pack(
        &self,
        kind: &str,
        value: &dyn Fn(&str) -> Result<u64, String>,
    ) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        for (name, width) in self.layout(kind)? {
            let number = value(name)?;
            if *width < 8 && number >> (8 * width) != 0 {
                return Err(format!(
                    "{name} of {kind} is {number}: too large for the record"
                ));
            }
            bytes.extend_from_slice(&number.to_le_bytes()[..*width]);
        }
        Ok(bytes)
    }

    fn size(&self, kind: &str) -> Result<usize, String> {
        Ok(self.layout(kind)?.iter().map(|(_, width)| width).sum())
    }
}

/// One line of a list's `used`.
#[derive(Default)]
struct Used {
    kind: String,
    name: String,
    record: Option<String>,
}

/// A plugin's list as it is written.
#[derive(Default)]
struct List {
    target: Option<String>,
    floor: Option<String>,
    used: Vec<Used>,
    services: Vec<String>,
    /// A feature and whether the plugin cannot run without it.
    features: Vec<(String, bool)>,
}

/// Whether `text` is one complete JSON value and nothing else. The stream
/// reader below takes what it gets and says nothing of a file that is cut
/// off or broken; a list read so would give a shorter record.
fn complete(text: &str) -> bool {
    fn space(s: &[u8], mut at: usize) -> usize {
        while s.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        at
    }
    fn string(s: &[u8], mut at: usize) -> Option<usize> {
        (*s.get(at)? == b'"').then_some(())?;
        loop {
            at += 1;
            match *s.get(at)? {
                b'"' => return Some(at + 1),
                b'\\' => at += 1,
                0..0x20 => return None,
                _ => {}
            }
        }
    }
    /// The end of the value at `at`; `depth` bounds the nesting.
    fn value(s: &[u8], at: usize, depth: usize) -> Option<usize> {
        let at = space(s, at);
        let (open, close) = match *s.get(at)? {
            b'"' => return string(s, at),
            b'{' => (b'{', b'}'),
            b'[' => (b'[', b']'),
            _ => {
                let end = at
                    + s[at..]
                        .iter()
                        .take_while(|b| !b" \t\r\n,]}".contains(b))
                        .count();
                let word = core::str::from_utf8(&s[at..end]).ok()?;
                let number = word
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b"+-.eE".contains(&b))
                    && word.parse::<f64>().is_ok();
                let known = matches!(word, "true" | "false" | "null") || number;
                return known.then_some(end);
            }
        };
        let mut at = space(s, at + 1);
        if depth == 0 {
            return None;
        }
        if *s.get(at)? == close {
            return Some(at + 1);
        }
        loop {
            if open == b'{' {
                at = space(s, string(s, space(s, at))?);
                (*s.get(at)? == b':').then_some(())?;
                at += 1;
            }
            at = space(s, value(s, at, depth - 1)?);
            match *s.get(at)? {
                b',' => at += 1,
                end if end == close => return Some(at + 1),
                _ => return None,
            }
        }
    }
    let s = text.as_bytes();
    value(s, 0, 16).is_some_and(|end| space(s, end) == s.len())
}

impl List {
    fn read(text: &str) -> Result<Self, String> {
        if !complete(text) {
            return Err(
                "the list is not one complete JSON value: it is cut off or broken".to_string(),
            );
        }
        let mut list = Self::default();
        let mut wrong = None;
        let (mut used, mut feature) = (Used::default(), (String::new(), false));
        let mut parser = Parser::default();
        let mut take = |event: Event<'_>| match event {
            Event::Value(at, text) => {
                let at: Vec<&str> = at.iter().map(String::as_str).collect();
                match at.as_slice() {
                    ["target"] => list.target = Some(text.to_string()),
                    ["floor"] => list.floor = Some(text.to_string()),
                    ["used", "[]", "kind"] => used.kind = text.to_string(),
                    ["used", "[]", "name"] => used.name = text.to_string(),
                    ["used", "[]", "record"] => used.record = Some(text.to_string()),
                    ["services", "[]"] => list.services.push(text.to_string()),
                    ["features", "[]"] => list.features.push((text.to_string(), false)),
                    ["features", "[]", "key"] => feature.0 = text.to_string(),
                    ["features", "[]", "required"] if text == "true" || text == "false" => {
                        feature.1 = text == "true";
                    }
                    _ => wrong = wrong.take().or(Some(at.join("."))),
                }
            }
            Event::Close(at) if at == ["used", "[]"] => {
                list.used.push(core::mem::take(&mut used));
            }
            Event::Close(at) if at == ["features", "[]"] => {
                list.features.push(core::mem::take(&mut feature));
            }
            Event::Close(_) | Event::Open(_) => {}
        };
        parser.feed(text.as_bytes(), &mut take);
        parser.finish(&mut take);
        match wrong {
            Some(at) => Err(format!(
                "`{at}` is not part of a list (or has a wrong value)"
            )),
            None => Ok(list),
        }
    }
}

/// A name or key as the record holds it: not empty, within the cap.
fn text<'a>(table: &Table, value: &'a str, what: &str) -> Result<&'a [u8], String> {
    let max = table.limit("max_key_bytes")?;
    if value.is_empty() || value.len() > max {
        return Err(format!("{what} `{value}` must be 1 to {max} bytes long"));
    }
    Ok(value.as_bytes())
}

/// The minor the list needs: its `floor` and what each used line needs.
fn needs_minor(table: &Table, list: &List) -> Result<u64, String> {
    let floor: u64 = list
        .floor
        .as_deref()
        .and_then(|floor| floor.parse().ok())
        .filter(|floor| *floor <= table.minor)
        .ok_or_else(|| format!("`floor` must be a number from 0 to {}", table.minor))?;
    if list.used.is_empty() && floor != table.minor {
        return Err(format!(
            "an empty `used` needs `floor` {}: nothing says what the code calls",
            table.minor
        ));
    }
    let mut seen: Vec<String> = Vec::new();
    let mut needs = floor;
    for used in &list.used {
        let line = match (used.kind.as_str(), &used.record) {
            ("host" | "record" | "callback", None) => format!("{} {}", used.kind, used.name),
            ("field", Some(record)) => format!("field {record} {}", used.name),
            _ => {
                return Err(format!(
                    "a line of `used` needs `kind` (host, callback, record or field) and \
                     `name`, a field also `record`; found kind `{}`",
                    used.kind
                ));
            }
        };
        let Some((_, minor)) = table.needs.iter().find(|(name, _)| *name == line) else {
            return Err(format!(
                "`{}` is no {} of plugin interface {}.{} (this tool's)",
                used.name,
                match used.kind.as_str() {
                    "host" => "function of the device",
                    "callback" => "callback of a plugin",
                    "record" => "record",
                    _ => "field of that record",
                },
                table.major,
                table.minor
            ));
        };
        if seen.contains(&line) {
            return Err(format!("`{}` is listed twice in `used`", used.name));
        }
        seen.push(line);
        needs = needs.max(*minor);
    }
    Ok(needs)
}

/// The rows of `catalog` the list names, in the order of their keys, with
/// the flags of each (a feature the plugin cannot run without).
fn selected<'a>(
    table: &Table,
    catalog: &'a [Row],
    keys: &[(String, bool)],
    what: &str,
) -> Result<Vec<(&'a Row, u64)>, String> {
    if keys.len() > table.limit(&format!("max_{what}s"))? {
        return Err(format!("too many {what}s"));
    }
    let mut rows = Vec::new();
    for (key, required) in keys {
        let row = catalog
            .iter()
            .find(|row| row.name == *key)
            .ok_or_else(|| format!("`{key}` is no {what} of this interface"))?;
        if rows.iter().any(|(seen, _): &(&Row, u64)| seen.name == *key) {
            return Err(format!("the {what} `{key}` is listed twice"));
        }
        rows.push((row, u64::from(*required)));
    }
    rows.sort_by(|a, b| a.0.name.cmp(&b.0.name));
    Ok(rows)
}

/// The record of `list` for the chip `target`: the bytes
/// `tools/abi-metadata.py` writes for the same list.
fn record(table: &Table, list: &List, target: &str) -> Result<Vec<u8>, String> {
    let profile = table
        .profiles
        .iter()
        .find(|profile| profile.name == target)
        .ok_or_else(|| format!("`{target}` is no chip this tool knows"))?;
    let services: Vec<(String, bool)> = list.services.iter().map(|k| (k.clone(), false)).collect();
    let services = selected(table, &table.services, &services, "service")?;
    let features = selected(table, &table.features, &list.features, "feature")?;
    let mut needs = needs_minor(table, list)?;
    for (row, _) in services.iter().chain(&features) {
        needs = needs.max(row.number("since")?);
    }
    let mut body = text(table, target, "the target")?.to_vec();
    for export in &table.exports {
        let name = text(table, &export.name, "the export")?;
        let expected = match export.text("size_source")? {
            "entry_bytes" => profile.number("entry_bytes")?,
            _ => profile.number("pointer_bits")? / 8,
        };
        let size = table.size("export")?;
        body.extend(table.pack("export", &|field| {
            Ok(match field {
                "record_bytes" => (size + name.len()) as u64,
                "name_bytes" => name.len() as u64,
                "expected_bytes" => expected,
                "reserved" => 0,
                other => export.number(other)?,
            })
        })?);
        body.extend_from_slice(name);
    }
    for (kind, rows) in [("service", &services), ("feature", &features)] {
        let size = table.size(kind)?;
        for (row, flags) in rows {
            let key = text(table, &row.name, kind)?;
            body.extend(table.pack(kind, &|field| {
                Ok(match field {
                    "record_bytes" => (size + key.len()) as u64,
                    "key_bytes" => key.len() as u64,
                    "flags" => *flags,
                    other => row.number(other)?,
                })
            })?);
            body.extend_from_slice(key);
        }
    }
    let prefix = table.size("prefix")?;
    let total = prefix + body.len();
    if total > table.limit("max_bytes")? {
        return Err("the record is over its size limit: fewer services or features".into());
    }
    let magic = table
        .limits
        .iter()
        .find(|(k, _)| k == "magic")
        .map(|(_, v)| v.as_bytes())
        .and_then(|magic| <[u8; 4]>::try_from(magic).ok())
        .ok_or("the tool's own table has no magic")?;
    let mut bytes = table.pack("prefix", &|field| {
        Ok(match field {
            "magic" => u64::from(u32::from_le_bytes(magic)),
            "layout" => table.limit("format")? as u64,
            "prefix_bytes" => prefix as u64,
            "total_bytes" => total as u64,
            "abi_major" => table.major,
            "needs_minor" => needs,
            "flags" => 0,
            "target_bytes" => target.len() as u64,
            "services" => services.len() as u64,
            "features" => features.len() as u64,
            "exports" => table.exports.len() as u64,
            other => profile.number(other)?,
        })
    })?;
    bytes.extend(body);
    Ok(bytes)
}

/// A record and the file it belongs in, from the plugin's folder.
pub type Record = (&'static str, Vec<u8>);

/// The records of the list `text`, each with the file it belongs in
/// (from the plugin's folder), and the interface's minor the plugin needs.
///
/// # Errors
/// What is wrong with the list, in a sentence.
pub fn records(text: &str) -> Result<(Vec<Record>, u64), String> {
    let table = Table::built_in()?;
    let list = List::read(text)?;
    if list.target.as_deref() != Some(X4_TARGET) {
        return Err(format!(
            "`target` must be `{X4_TARGET}`: the list is written once, for the X4, and the \
             X4 Pro's record is made from it"
        ));
    }
    let mut made = Vec::new();
    for (file, target) in RECORDS {
        made.push((file, record(&table, &list, target)?));
    }
    Ok((made, needs_minor(&table, &list)?))
}

/// Writes the records of the plugin in `dir`, or with `check` only compares.
/// `Ok(false)`: a record is missing or not what the list gives.
///
/// # Errors
/// The list cannot be read or is wrong, or a record cannot be written.
pub fn write(dir: &Path, check: bool) -> Result<bool, String> {
    let list = dir.join(LIST);
    let text = std::fs::read_to_string(&list).map_err(|e| format!("{}: {e}", list.display()))?;
    let (made, _) = records(&text).map_err(|e| format!("{}: {e}", list.display()))?;
    let mut current = true;
    for (file, bytes) in made {
        let path = dir.join(file);
        if std::fs::read(&path).is_ok_and(|have| have == bytes) {
            continue;
        }
        if check {
            println!(
                "{}: not what {LIST} gives; run `paperpaw-dev metadata`",
                path.display()
            );
            current = false;
        } else {
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            println!("wrote {}", path.display());
        }
    }
    Ok(current)
}

/// `metadata [--check] [<plugin's folder>]`.
///
/// # Errors
/// A wrong option, or what [`write()`] reports.
pub fn run(args: &[String]) -> Result<bool, String> {
    let (mut check, mut dir) = (false, PathBuf::from("."));
    for arg in args {
        match arg.as_str() {
            "--check" => check = true,
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => dir = PathBuf::from(other),
        }
    }
    write(&dir, check)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every plugin of PaperPaw's own with code, and the template of
    /// `new --code`: the records are byte for byte what the Python tools
    /// wrote beside the list (`tools/abi-metadata.py`,
    /// `tools/plugin-files7/s3-metadata.py`).
    #[test]
    fn the_records_are_the_python_tools_byte_for_byte() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut compared = 0;
        for plugin in std::fs::read_dir(root.join("plugins")).unwrap() {
            let dir = plugin.unwrap().path();
            let Ok(text) = std::fs::read_to_string(dir.join(LIST)) else {
                continue;
            };
            let (made, _) = records(&text).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            for (file, bytes) in made {
                let kept = std::fs::read(dir.join(file)).unwrap();
                assert_eq!(bytes, kept, "{}/{file}", dir.display());
                compared += 1;
            }
        }
        assert!(compared >= 150, "only {compared} records compared");
        let template = root.join("tools/dev/templates/code");
        let text = std::fs::read_to_string(template.join("metadata.json")).unwrap();
        let (made, needs) = records(&text).unwrap();
        assert_eq!(
            made[0].1,
            std::fs::read(template.join("metadata.bin")).unwrap()
        );
        assert!(needs >= 1);
    }

    #[test]
    fn a_wrong_list_is_refused_in_words() {
        let list = |used: &str| {
            format!(
                r#"{{"target": "{X4_TARGET}", "floor": 1, "used": [{used}],
                    "services": [], "features": []}}"#
            )
        };
        let error = |text: String| records(&text).unwrap_err();
        let host = r#"{"kind": "host", "name": "heap.alloc"}"#;
        assert!(records(&list(host)).is_ok());
        // Cut off or broken: refused, not read as far as it goes.
        let whole = list(host);
        for cut in [
            whole.len() - 1,
            whole.len() - 3,
            whole.find("\"services\"").unwrap(),
        ] {
            assert!(
                error(whole[..cut].to_string()).contains("cut off or broken"),
                "{cut}"
            );
        }
        assert!(error(format!("{whole} x")).contains("cut off or broken"));
        assert!(
            error(whole.replace("\"floor\": 1,", "\"floor\": 1")).contains("cut off or broken")
        );
        assert!(error(list(r#"{"kind": "host", "name": "heap.nothing"}"#)).contains("no function"));
        assert!(error(list(&format!("{host}, {host}"))).contains("twice"));
        assert!(error(list(host).replace(X4_TARGET, S3_TARGET)).contains("`target` must be"));
        assert!(error(list(host).replace("\"floor\": 1", "\"floor\": 99")).contains("`floor`"));
        assert!(
            error(list(host).replace("\"services\": []", "\"services\": [\"no.such\"]"))
                .contains("no service")
        );
    }
}
