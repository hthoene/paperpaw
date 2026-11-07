//! `paperpaw-fontgen`: converts fonts into PaperPaw bitmap font packs.
//!
//! Devices without runtime font rendering (ESP32-C3) read `.ppfp` packs
//! directly from flash or storage. Packs are rendered with the same
//! rasterizer PaperPaw uses at runtime, so text looks identical.

use std::process::ExitCode;

use paperpaw_fontgen::{FaceFont, STYLES, make_family, ranges_of};

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
    let mut fonts: [Option<FaceFont>; 4] = [None, None, None, None];
    for (slot, file) in fonts.iter_mut().zip(faces) {
        if let Some(file) = file {
            let data = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
            *slot = Some(FaceFont {
                data,
                label: file.clone(),
            });
        }
    }
    let regular = faces[0].clone().unwrap_or_default();
    let made = make_family(name, fonts, sizes, ranges, axes, hinting)?;
    let dir = std::path::Path::new(folder).join(&made.id);
    let packs = dir.join("fonts");
    std::fs::create_dir_all(&packs).map_err(|e| format!("{}: {e}", packs.display()))?;
    let mut total = 0;
    for (style, pack) in &made.packs {
        let path = packs.join(format!("{style}.ppfp"));
        std::fs::write(&path, pack).map_err(|e| format!("{}: {e}", path.display()))?;
        total += pack.len();
        println!("{style}: {:.1} KiB", pack.len() as f64 / 1024.0);
        if *style == "regular"
            && let Some((has, asked)) = made.coverage
        {
            println!("{regular}: has {has} of the {asked} characters asked for");
            if has == 0 {
                println!(
                    "warning: none of them: the pack will draw nothing; --chars names the letters a book needs"
                );
            }
        }
    }
    std::fs::write(dir.join("plugin.ini"), &made.ini).map_err(|e| format!("plugin.ini: {e}"))?;
    println!(
        "{}: {:.1} MiB. Copy the folder into the plugins folder of the card, restart, and choose\n\
         `{name}` in Settings, under Reading, as Font.",
        dir.display(),
        total as f64 / 1_048_576.0
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use paperpaw_fontgen::{listed, slug};

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
