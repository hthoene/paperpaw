//! `paperpaw-hyphenation-packs`: builds and checks the hyphenation data plugins.
//!
//! ```text
//! paperpaw-hyphenation-packs build <repo-root> <out-dir>
//! paperpaw-hyphenation-packs check <plugin-folder>...
//! ```

use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage: paperpaw-hyphenation-packs build <repo-root> <out-dir>\n       paperpaw-hyphenation-packs check <plugin-folder>...";

fn run(args: &[String]) -> Result<bool, String> {
    match args {
        [cmd, root, out] if cmd == "build" => {
            let folders = paperpaw_hyphenation_packs::build_all(Path::new(root), Path::new(out))
                .map_err(|e| e.to_string())?;
            for folder in folders {
                println!("{}", folder.display());
            }
            Ok(true)
        }
        [cmd, folders @ ..] if cmd == "check" && !folders.is_empty() => {
            let mut clean = true;
            for folder in folders {
                for problem in paperpaw_hyphenation_packs::check(Path::new(folder)) {
                    println!("{folder}: {problem}");
                    clean = false;
                }
            }
            Ok(clean)
        }
        _ => Err(USAGE.to_string()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
