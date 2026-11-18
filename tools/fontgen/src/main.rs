//! `paperpaw-fontgen`: converts fonts into PaperPaw bitmap font packs.
//!
//! Devices without runtime font rendering (ESP32-C3) read `.ppfp` packs
//! directly from flash or storage. Packs are rendered with the same
//! rasterizer PaperPaw uses at runtime, so text looks identical.

use std::process::ExitCode;

const USAGE: &str = "\
paperpaw-fontgen: build PaperPaw bitmap font packs

USAGE:
    paperpaw-fontgen <font.ttf|otf> <output.ppfp> [OPTIONS]
    paperpaw-fontgen --family <name> --regular <font> [--bold <font>]
        [--italic <font>] [--bold-italic <font>] <folder> [OPTIONS]

The second form makes a whole font plugin of your own: the folder
`<folder>/font-<name>` with a pack for each face (a face you leave out is
drawn in the regular one), to copy into the `plugins` folder of the device's
card. Then choose it in Settings, under Reading, as Font.

OPTIONS:
    --sizes <px,...>      Pixel sizes to render (default: 20,24,28,32,36,40,48)
    --axis <tag=value>    Set a variation axis, e.g. --axis wght=700 (repeatable)
    --hinting <mode>      Fit the letters to the pixel grid: none (default:
                          the outline as drawn), light (even, crisp letters
                          at about the unfitted weight: the look of the
                          default fonts), normal (stems on whole pixels:
                          heavier) or mono (one pixel stems: the crispest,
                          the lightest). Only the ink changes: a book breaks
                          into the same pages. Letters with marks (Arabic,
                          Devanagari ...) are best left at none.
    --chars <set,...>     Character sets (default: latin,ligatures; the names
                          are listed below), code points: U+EA01, U+EA01-EA0F
                          (e.g. icons), or @FILE: code points listed in a file
                          that starts with `# paperpaw-charset 1` (sets/*.txt)
    -h, --help            Show this help

CHARACTER SETS:
";

/// The first line of a file of code points: its format, so a later one
/// (names inside files, exclusions) can be told apart.
const SET_FILE: &str = "# paperpaw-charset 1";

/// The help, with the names of the character sets.
fn usage() -> String {
    let names: Vec<&str> = paperpaw_text::charset::names().collect();
    format!("{USAGE}    {}\n", names.join(", "))
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The faces of a family, as the plugin's `[family]` names them, with the
/// weight each is drawn at where its font is a variable one.
const STYLES: [(&str, f32); 4] = [
    ("regular", 400.0),
    ("bold", 700.0),
    ("italic", 400.0),
    ("bold-italic", 700.0),
];

fn run(args: Vec<String>) -> Result<(), String> {
    let mut positional = Vec::new();
    let mut family: Option<String> = None;
    let mut faces: [Option<String>; 4] = [None, None, None, None];
    let mut sizes = vec![20.0, 24.0, 28.0, 32.0, 36.0, 40.0, 48.0];
    let mut axes: Vec<(String, f32)> = Vec::new();
    let mut sets = vec![String::from("latin"), String::from("ligatures")];
    // Unhinted unless asked: a pack made by a script (marks, other scripts)
    // keeps the outline as drawn. The default fonts are made light.
    let mut hinting = paperpaw_text::Hinting::None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{}", usage());
                return Ok(());
            }
            "--sizes" => {
                let list = args.next().ok_or("--sizes requires a value")?;
                sizes = list
                    .split(',')
                    .map(|s| {
                        s.trim()
                            .parse::<f32>()
                            .map_err(|_| format!("invalid size `{s}`"))
                    })
                    .collect::<Result<_, _>>()?;
            }
            "--hinting" => {
                let mode = args.next().ok_or("--hinting requires a value")?;
                hinting = paperpaw_text::Hinting::parse(&mode)
                    .ok_or_else(|| format!("--hinting: `{mode}` is none, light, normal or mono"))?;
            }
            "--family" => family = Some(args.next().ok_or("--family requires a name")?),
            "--regular" | "--bold" | "--italic" | "--bold-italic" => {
                let style = arg.trim_start_matches('-');
                let at = STYLES.iter().position(|(s, _)| *s == style).unwrap_or(0);
                faces[at] = Some(args.next().ok_or(format!("{arg} requires a font file"))?);
            }
            "--chars" => {
                let list = args.next().ok_or("--chars requires a value")?;
                sets = list.split(',').map(|s| s.trim().to_string()).collect();
            }
            "--axis" => {
                let axis = args.next().ok_or("--axis requires a value")?;
                let (tag, value) = axis
                    .split_once('=')
                    .ok_or_else(|| format!("expected <tag>=<value>, got `{axis}`"))?;
                let value = value
                    .parse()
                    .map_err(|_| format!("invalid axis value `{value}`"))?;
                axes.push((tag.to_string(), value));
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option `{arg}`")),
            _ => positional.push(arg),
        }
    }
    if let Some(name) = family {
        let [folder] = positional.as_slice() else {
            return Err(format!("expected one output folder\n\n{}", usage()));
        };
        let ranges = ranges_of(&sets)?;
        return family_plugin(&name, &faces, folder, &sizes, &ranges, &axes, hinting);
    }
    let [input, output] = positional.as_slice() else {
        return Err(format!(
            "expected an input and an output file\n\n{}",
            usage()
        ));
    };

    let data = std::fs::read(input).map_err(|e| format!("{input}: {e}"))?;
    let variations: Vec<(&str, f32)> = axes.iter().map(|(t, v)| (t.as_str(), *v)).collect();
    let font =
        paperpaw_text::OutlineFont::new(data, &variations).map_err(|e| format!("{input}: {e}"))?;
    let ranges = ranges_of(&sets)?;
    let chars = paperpaw_text::charset::chars(&ranges);
    let pack = paperpaw_text::build_pack_hinted(&font, &sizes, chars, hinting);
    std::fs::write(output, &pack).map_err(|e| format!("{output}: {e}"))?;
    println!(
        "{output}: {} sizes, {:.1} KiB",
        sizes.len(),
        pack.len() as f64 / 1024.0
    );
    Ok(())
}

/// The code points of character `sets`: names, code points or `@FILE`.
fn ranges_of(sets: &[String]) -> Result<Vec<std::ops::RangeInclusive<char>>, String> {
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
fn slug(name: &str) -> String {
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

/// Makes the plugin of a family of your own: `<folder>/font-<name>`.
fn family_plugin(
    name: &str,
    faces: &[Option<String>; 4],
    folder: &str,
    sizes: &[f32],
    ranges: &[std::ops::RangeInclusive<char>],
    axes: &[(String, f32)],
    hinting: paperpaw_text::Hinting,
) -> Result<(), String> {
    let id = slug(name);
    if id.is_empty() || name.contains(['\n', '\r']) {
        return Err(format!(
            "the name `{name}` needs letters (a to z) or digits"
        ));
    }
    let Some(regular) = faces[0].as_deref() else {
        return Err("a family needs a regular face: add --regular <font>".into());
    };
    let dir = std::path::Path::new(folder).join(format!("font-{id}"));
    let fonts = dir.join("fonts");
    std::fs::create_dir_all(&fonts).map_err(|e| format!("{}: {e}", fonts.display()))?;
    let chars = paperpaw_text::charset::chars(ranges);
    let mut ini = format!(
        "[plugin]\nid = font-{id}\nname = {name}\n\
         description = Your own font, made with paperpaw-fontgen.\nversion = 1.0.0\n\n\
         [family]\nname = {name}\n"
    );
    let mut total = 0;
    for ((style, weight), file) in STYLES.iter().zip(faces) {
        let Some(file) = file else { continue };
        let data = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
        let mut variations = vec![("wght", *weight)];
        variations.extend(axes.iter().map(|(t, v)| (t.as_str(), *v)));
        let font = paperpaw_text::OutlineFont::new(data, &variations)
            .map_err(|e| format!("{file}: {e} (a TrueType or OpenType font is needed)"))?;
        let pack = paperpaw_text::build_pack_hinted(&font, sizes, chars.clone(), hinting);
        let path = fonts.join(format!("{style}.ppfp"));
        std::fs::write(&path, &pack).map_err(|e| format!("{}: {e}", path.display()))?;
        ini.push_str(&format!("{style} = {style}.ppfp\n"));
        total += pack.len();
        println!("{style}: {:.1} KiB", pack.len() as f64 / 1024.0);
        if *style == "regular" {
            coverage(&font, ranges, regular);
        }
    }
    std::fs::write(dir.join("plugin.ini"), ini).map_err(|e| format!("plugin.ini: {e}"))?;
    println!(
        "{}: {:.1} MiB. Copy the folder into the plugins folder of the card, restart, and choose\n\
         `{name}` in Settings, under Reading, as Font.",
        dir.display(),
        total as f64 / 1_048_576.0
    );
    Ok(())
}

/// Says how much of the characters asked for the font has, for the book's
/// letters: `file` has none of them where its font lacks the script.
fn coverage(
    font: &paperpaw_text::OutlineFont,
    ranges: &[std::ops::RangeInclusive<char>],
    file: &str,
) {
    let all: Vec<char> = paperpaw_text::charset::chars(ranges).collect();
    let has = all.iter().filter(|&&c| font.has_char(c)).count();
    println!(
        "{file}: has {has} of the {} characters asked for",
        all.len()
    );
    if has == 0 {
        println!(
            "warning: none of them: the pack will draw nothing; --chars names the letters a book needs"
        );
    }
}

/// The code points a file lists (`U+AC00` or `U+AC00-AC01`, separated by
/// spaces, commas or lines; `#` starts a comment): sets as data, without
/// a new version of the tool.
fn listed(path: &str) -> Result<Vec<std::ops::RangeInclusive<char>>, String> {
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    #[test]
    fn a_family_of_your_own_is_a_plugin_folder() {
        assert_eq!(slug("My Font 2!"), "my-font-2");
        assert_eq!(slug("  --Ünï  "), "n");
        let font = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../plugins/fonts-default/fonts/Literata.ttf"
        );
        let out = std::env::temp_dir().join(format!("fontgen-family-{}", std::process::id()));
        let run_with = |args: &[&str]| {
            let mut all: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
            all.push(out.to_string_lossy().into_owned());
            all.extend(["--sizes", "20", "--chars", "latin"].map(String::from));
            run(all)
        };
        // A regular face is needed.
        assert!(
            run_with(&["--family", "Mine"])
                .unwrap_err()
                .contains("regular")
        );
        assert!(run_with(&["--family", "!!", "--regular", font]).is_err());
        run_with(&["--family", "My Font", "--regular", font, "--bold", font]).unwrap();
        let dir = out.join("font-my-font");
        let ini = std::fs::read_to_string(dir.join("plugin.ini")).unwrap();
        assert!(ini.starts_with("[plugin]\nid = font-my-font\nname = My Font\n"));
        assert!(
            ini.ends_with("[family]\nname = My Font\nregular = regular.ppfp\nbold = bold.ppfp\n")
        );
        assert!(dir.join("fonts/bold.ppfp").is_file());
        assert!(!dir.join("fonts/italic.ppfp").exists());
        std::fs::remove_dir_all(out).ok();
    }

    /// Packs are unhinted unless `--hinting` asks (#122): the scripts that
    /// make the script, Literata-style and icon packs rely on it, and say
    /// so themselves.
    #[test]
    fn packs_are_unhinted_unless_asked() {
        let font = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../plugins/fonts-default/fonts/Literata.ttf"
        );
        let out = std::env::temp_dir().join(format!("fontgen-hinting-{}", std::process::id()));
        let make = |name: &str, extra: &[&str]| {
            let file = out.join(format!("{name}.ppfp"));
            let mut args: Vec<String> = [
                font,
                file.to_str().unwrap(),
                "--sizes",
                "20",
                "--chars",
                "latin",
            ]
            .map(String::from)
            .into();
            args.extend(extra.iter().map(|a| (*a).to_string()));
            run(args).unwrap();
            std::fs::read(file).unwrap()
        };
        std::fs::create_dir_all(&out).unwrap();
        let default = make("default", &[]);
        assert_eq!(default, make("none", &["--hinting", "none"]));
        assert_ne!(default, make("light", &["--hinting", "light"]));
        std::fs::remove_dir_all(out).ok();
    }

    /// The scripts that rebuild released packs name the unhinted mode, so
    /// that a changed default cannot turn their bytes under the same
    /// versions.
    #[test]
    fn the_pack_scripts_say_none() {
        for script in ["script-pack.py", "literata-packs.py", "icons/pack.py"] {
            let path = format!("{}/../{script}", env!("CARGO_MANIFEST_DIR"));
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(
                text.contains("\"--hinting\", \"none\""),
                "{script} makes packs without saying `--hinting none`"
            );
        }
    }

    /// The Korean letters come in two packs (#61): `hangul-ks.txt` for the
    /// base pack, `hangul-rest.txt` for the extra one that only adds
    /// letters. A syllable outside KS X 1001 is in the extra pack's set, not
    /// the base pack's, and the font draws it.
    #[test]
    fn rare_syllables_are_the_extra_packs() {
        let sets = concat!(env!("CARGO_MANIFEST_DIR"), "/sets");
        let has = |file: &str, c: char| {
            let set = format!("@{sets}/{file}");
            let ranges = ranges_of(&[set]).unwrap();
            ranges.iter().any(|r| r.contains(&c))
        };
        // 한 is of everyday text; 똠 (U+B620) is in no KS X 1001 row.
        assert!(has("hangul-ks.txt", '한') && !has("hangul-rest.txt", '한'));
        assert!(has("hangul-rest.txt", '똠') && !has("hangul-ks.txt", '똠'));
        // Together, all of them, each once.
        let count = |file: &str| {
            let ranges = ranges_of(&[format!("@{sets}/{file}")]).unwrap();
            ranges
                .iter()
                .map(|r| *r.end() as u32 - *r.start() as u32 + 1)
                .sum::<u32>()
        };
        assert_eq!(count("hangul-ks.txt") + count("hangul-rest.txt"), 11_172);
        // The pack of the rare syllables (here one of them and a size)
        // draws it, with letters in it.
        let font = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../fonts/NanumMyeongjo-Regular.ttf"
        );
        let out = std::env::temp_dir().join(format!("fontgen-rest-{}.ppfp", std::process::id()));
        let args = [
            font,
            out.to_str().unwrap(),
            "--sizes",
            "20",
            "--chars",
            "U+B620",
        ];
        run(args.iter().map(|a| (*a).to_string()).collect()).unwrap();
        let pack = paperpaw_text::BitmapFont::new(std::fs::read(&out).unwrap()).unwrap();
        assert!(pack.has_char('똠'));
        assert!(pack.glyph('똠', 20.0).width > 0);
        assert!(!pack.has_char('한'));
        std::fs::remove_file(out).ok();
    }

    /// Amharic books (#159): the Ethiopic pack as `script-pack.py` asks for
    /// it (here one size) opens within the X4's limits and draws the
    /// syllables, the wordspace and the full stop through the pack as the
    /// device reads it, a glyph at a time; what stays in memory is small.
    #[test]
    fn the_ethiopic_pack_draws_amharic_within_the_x4s_limits() {
        use paperpaw_text::{LARGE_SCRIPT, PackLimits, PackSource, StoredPack};
        struct Memory(Vec<u8>);
        impl PackSource for Memory {
            fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
                let bytes = usize::try_from(at)
                    .ok()
                    .and_then(|at| self.0.get(at..)?.get(..buf.len()));
                bytes.is_some_and(|bytes| {
                    buf.copy_from_slice(bytes);
                    true
                })
            }
        }
        const CHARS: [&str; 3] = ["U+1200-139F", "U+2D80-2DDF", "U+AB00-AB2F"];
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../script-pack.py");
        let listed = CHARS.map(|c| format!("\"{c}\"")).join(", ");
        assert!(
            std::fs::read_to_string(script).unwrap().contains(&listed),
            "script-pack.py's ethiopic set is not {listed}"
        );
        let font = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../fonts/NotoSerifEthiopic.ttf"
        );
        let out = std::env::temp_dir().join(format!("fontgen-ethi-{}.ppfp", std::process::id()));
        let chars = CHARS.join(",");
        let args = [
            font,
            out.to_str().unwrap(),
            "--sizes",
            "20",
            "--chars",
            &chars,
            "--axis",
            "wght=400",
        ];
        run(args.iter().map(|a| (*a).to_string()).collect()).unwrap();
        let data = std::fs::read(&out).unwrap();
        std::fs::remove_file(out).ok();
        let pack = StoredPack::open_within(std::rc::Rc::new(Memory(data)), PackLimits::SMALL)
            .expect("the pack opens within the limits of a 256 KiB heap");
        // Every character of the three ranges the font has: an alphabet's
        // few hundred, so the glyph cache stays the built-in packs'.
        assert_eq!(pack.chars(), 495);
        assert!(pack.chars() < LARGE_SCRIPT);
        // ሰላም፡ዓለም። ("hello, world."), a numeral, and a letter of each of
        // Ethiopic Supplement, Extended and Extended-A.
        for c in "ሰላም፡ዓለም።፩\u{1380}\u{2d80}\u{ab01}".chars() {
            assert!(pack.has_char(c), "{c}");
            assert!(pack.advance(c, 20.0) > 0.0, "{c}");
            assert!(pack.render(c, 20.0).unwrap().width > 0, "{c}");
        }
        // Latin stays the built-in fonts'; a code point the block leaves
        // unassigned is not there.
        assert!(!pack.has_char('a') && !pack.has_char('\u{1249}'));
        // Its index and the advances of the size in use (1.5 KiB).
        assert!(pack.bytes() <= 2048, "{} B held", pack.bytes());
    }

    #[test]
    fn set_files_say_their_format() {
        let dir = std::env::temp_dir().join(format!("fontgen-sets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = |name: &str, text: &str| {
            let path = dir.join(name);
            std::fs::write(&path, text).unwrap();
            path.to_string_lossy().into_owned()
        };
        let good = file(
            "good.txt",
            "# paperpaw-charset 1\n# Hangul\nU+AC00-AC01, U+AC04 # two\n\tU+D7A3\n",
        );
        assert_eq!(
            listed(&good).unwrap(),
            [
                '\u{ac00}'..='\u{ac01}',
                '\u{ac04}'..='\u{ac04}',
                '\u{d7a3}'..='\u{d7a3}'
            ]
        );
        assert!(listed(&file("bare.txt", "U+AC00\n")).is_err());
        assert!(
            listed(&file("bad.txt", "# paperpaw-charset 1\nhangul\n"))
                .unwrap_err()
                .contains(":2:")
        );
        let shipped = concat!(env!("CARGO_MANIFEST_DIR"), "/sets/hangul-ks.txt");
        let ks: usize = listed(shipped)
            .unwrap()
            .iter()
            .map(|r| r.clone().count())
            .sum();
        assert_eq!(ks, 2350);
        assert!(usage().contains("hangul-jamo"));
        std::fs::remove_dir_all(dir).ok();
    }
}
