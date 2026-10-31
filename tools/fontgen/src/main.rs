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

OPTIONS:
    --sizes <px,...>      Pixel sizes to render (default: 20,24,28,32,36,40,48)
    --axis <tag=value>    Set a variation axis, e.g. --axis wght=700 (repeatable)
    --chars <set,...>     Character sets: latin and ligatures (default),
                          cyrillic, greek, arabic, hebrew, hangul, cjk-symbols,
                          code points: U+EA01, U+EA01-EA0F (e.g. icons), or
                          @FILE: code points listed in a file (e.g. sets/*.txt)
    -h, --help            Show this help
";

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
    let mut sizes = vec![20.0, 24.0, 28.0, 32.0, 36.0, 40.0, 48.0];
    let mut axes: Vec<(String, f32)> = Vec::new();
    let mut sets = vec![String::from("latin"), String::from("ligatures")];
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
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
    let [input, output] = positional.as_slice() else {
        return Err(format!("expected an input and an output file\n\n{USAGE}"));
    };

    let data = std::fs::read(input).map_err(|e| format!("{input}: {e}"))?;
    let variations: Vec<(&str, f32)> = axes.iter().map(|(t, v)| (t.as_str(), *v)).collect();
    let font =
        paperpaw_text::OutlineFont::new(data, &variations).map_err(|e| format!("{input}: {e}"))?;
    let mut ranges = Vec::new();
    for set in &sets {
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
    let chars = paperpaw_text::charset::chars(&ranges);
    let pack = paperpaw_text::build_pack(&font, &sizes, chars);
    std::fs::write(output, &pack).map_err(|e| format!("{output}: {e}"))?;
    println!(
        "{output}: {} sizes, {:.1} KiB",
        sizes.len(),
        pack.len() as f64 / 1024.0
    );
    Ok(())
}

/// The code points a file lists (`U+AC00` or `U+AC00-AC01`, separated by
/// spaces, commas or lines; `#` starts a comment): sets as data, without
/// a new version of the tool.
fn listed(path: &str) -> Result<Vec<std::ops::RangeInclusive<char>>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
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
