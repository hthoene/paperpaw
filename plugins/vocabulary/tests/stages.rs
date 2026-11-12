//! The stage the reader runs when a word is looked up, as Vocabulary subscribes
//! to it: the schema it decodes with is the reader's, byte for byte, and the
//! subscription is declared.
#![allow(clippy::unwrap_used, reason = "assertions")]

use std::fs;

const OURS: &str = "stages/subscribed/reader.word-looked-up@1.def";
const READERS: &str = "../reader/stages/reader.word-looked-up@1.def";

#[test]
fn the_schema_is_the_readers_byte_for_byte() {
    let ours = fs::read(OURS).unwrap();
    let readers = fs::read(READERS).unwrap();
    assert_eq!(
        ours, readers,
        "copy plugins/reader/stages/reader.word-looked-up@1.def and run tools/stage-codec.py"
    );
}

#[test]
fn the_codec_is_made_from_that_schema() {
    // `--check` refuses stale output and writes nothing.
    let status = std::process::Command::new("python3")
        .args([
            "-I",
            "../../tools/stage-codec.py",
            "stages/subscribed",
            "src/stages_subscribed.rs",
            "--check",
        ])
        .status();
    // No Python here: the tools' own checks cover it.
    if let Ok(status) = status {
        assert!(status.success(), "the generated codec is stale");
    }
}

const DECORATIONS: &str = "stages/decorations/reader.decorations@1.def";
const READERS_DECORATIONS: &str = "../reader/stages/reader.decorations@1.def";

#[test]
fn the_decorations_schema_is_the_readers_byte_for_byte() {
    let ours = fs::read(DECORATIONS).unwrap();
    let readers = fs::read(READERS_DECORATIONS).unwrap();
    assert_eq!(
        ours, readers,
        "copy plugins/reader/stages/reader.decorations@1.def and run tools/stage-codec.py stages/decorations src/stages_decorations.rs"
    );
}

#[test]
fn the_decorations_codec_is_made_from_that_schema() {
    let status = std::process::Command::new("python3")
        .args([
            "-I",
            "../../tools/stage-codec.py",
            "stages/decorations",
            "src/stages_decorations.rs",
            "--check",
        ])
        .status();
    if let Ok(status) = status {
        assert!(status.success(), "the generated codec is stale");
    }
}
