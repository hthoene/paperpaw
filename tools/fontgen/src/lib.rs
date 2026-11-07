//! The conversion behind `paperpaw-fontgen`, as a library: fonts in, packs
//! out, all in memory. The command line (`main.rs`) reads and writes the
//! files; paperpaw.org's font page compiles this to WebAssembly and runs it in
//! the browser, so a font never leaves the person's computer and the packs
//! are the same bytes as the command's.

use std::ops::RangeInclusive;

/// The first line of a file of code points: its format, so a later one
/// (names inside files, exclusions) can be told apart.
pub const SET_FILE: &str = "# paperpaw-charset 1";

/// The faces of a family, as the plugin's `[family]` names them, with the
/// weight each is drawn at where its font is a variable one.
pub const STYLES: [(&str, f32); 4] = [
    ("regular", 400.0),
    ("bold", 700.0),
    ("italic", 400.0),
    ("bold-italic", 700.0),
];

/// The code points of character `sets`: names, code points or `@FILE`.
pub fn ranges_of(sets: &[String]) -> Result<Vec<RangeInclusive<char>>, String> {
    let mut ranges = Vec::new();
    for set in sets {
        if let Some(named) = paperpaw_text::charset::named(set) {
            ranges.extend_from_slice(named);
        } else if let Some(points) = paperpaw_text::charset::code_points(set) {
            ranges.push(points);
        } else if let Some(path) = set.strip_prefix('@') {
            ranges.extend(listed(path)?);
        } else {
            return Err(format!("unknown character set `{set}`"));
        }
    }
    Ok(ranges)
}

/// A name as a plugin's id: lower case letters and digits, one `-` between.
pub fn slug(name: &str) -> String {
    let mut id = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c.to_ascii_lowercase());
        } else if !id.ends_with('-') && !id.is_empty() {
            id.push('-');
        }
    }
    id.trim_end_matches('-').to_string()
}

/// One face of a family: its font and the name to say it by.
#[derive(Debug)]
pub struct FaceFont {
    /// The font file (TrueType or OpenType).
    pub data: Vec<u8>,
    /// What the messages call it (a file name).
    pub label: String,
}

/// A family made: the plugin folder's files, and what to tell the person.
#[derive(Debug, Clone, PartialEq)]
pub struct Family {
    /// The plugin's id, `font-<slug>`.
    pub id: String,
    /// `plugin.ini`.
    pub ini: String,
    /// A pack for each face given: its style (`regular` ...) and its bytes.
    pub packs: Vec<(&'static str, Vec<u8>)>,
    /// How many of the characters asked for the regular face has, of how many.
    pub coverage: Option<(usize, usize)>,
}

impl Family {
    /// The path of each file under the plugin's folder, with its bytes.
    pub fn files(&self) -> Vec<(String, Vec<u8>)> {
        let mut files = vec![(
            format!("{}/plugin.ini", self.id),
            self.ini.clone().into_bytes(),
        )];
        for (style, pack) in &self.packs {
            files.push((format!("{}/fonts/{style}.ppfp", self.id), pack.clone()));
        }
        files
    }
}

/// Makes the plugin of a family of your own from its fonts: the same
/// packs and `plugin.ini` as `paperpaw-fontgen --family`.
pub fn make_family(
    name: &str,
    faces: [Option<FaceFont>; 4],
    sizes: &[f32],
    ranges: &[RangeInclusive<char>],
    axes: &[(String, f32)],
    hinting: paperpaw_text::Hinting,
) -> Result<Family, String> {
    let id = slug(name);
    if id.is_empty() || name.contains(['\n', '\r']) {
        return Err(format!(
            "the name `{name}` needs letters (a to z) or digits"
        ));
    }
    if faces[0].is_none() {
        return Err("a family needs a regular face: add --regular <font>".into());
    }
    let chars = paperpaw_text::charset::chars(ranges);
    let mut ini = format!(
        "[plugin]\nid = font-{id}\nname = {name}\n\
         description = Your own font, made with paperpaw-fontgen.\nversion = 1.0.0\n\n\
         [family]\nname = {name}\n"
    );
    let mut packs = Vec::new();
    let mut coverage = None;
    for ((style, weight), face) in STYLES.iter().zip(faces) {
        let Some(face) = face else { continue };
        let mut variations = vec![("wght", *weight)];
        variations.extend(axes.iter().map(|(t, v)| (t.as_str(), *v)));
        let font = paperpaw_text::OutlineFont::new(face.data, &variations).map_err(|e| {
            format!(
                "{}: {e} (a TrueType or OpenType font is needed)",
                face.label
            )
        })?;
        let pack = paperpaw_text::build_pack_hinted(&font, sizes, chars.clone(), hinting);
        ini.push_str(&format!("{style} = {style}.ppfp\n"));
        if *style == "regular" {
            coverage = Some(covered(&font, ranges));
        }
        packs.push((*style, pack));
    }
    Ok(Family {
        id: format!("font-{id}"),
        ini,
        packs,
        coverage,
    })
}

/// How many of the characters asked for `font` has, of how many were asked.
pub fn covered(
    font: &paperpaw_text::OutlineFont,
    ranges: &[RangeInclusive<char>],
) -> (usize, usize) {
    let all: Vec<char> = paperpaw_text::charset::chars(ranges).collect();
    let has = all.iter().filter(|&&c| font.has_char(c)).count();
    (has, all.len())
}

/// The code points a file lists (`U+AC00` or `U+AC00-AC01`, separated by
/// spaces, commas or lines; `#` starts a comment): sets as data, without
/// a new version of the tool.
pub fn listed(path: &str) -> Result<Vec<RangeInclusive<char>>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    if text.lines().next().map(str::trim_end) != Some(SET_FILE) {
        return Err(format!(
            "{path}: not a set file: its first line must be `{SET_FILE}`"
        ));
    }
    let mut ranges = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("");
        for item in line.split([' ', ',', '\t']).filter(|item| !item.is_empty()) {
            let range = paperpaw_text::charset::code_points(item)
                .ok_or_else(|| format!("{path}:{}: not a code point: `{item}`", number + 1))?;
            ranges.push(range);
        }
    }
    Ok(ranges)
}
