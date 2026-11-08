//! The screens whose hints were shown, kept in `seen.txt`: a screen's id a line.
//! Every read and write goes through the host's storage with fallible allocation.
use alloc::string::String;
use paperpaw_sdk::native::{Error, Host, storage, storage::Root};
use paperpaw_ui_kit::files;

const FILE: &str = "seen.txt";
const MAX: usize = 64 * 1024;

fn read(host: &Host<'_>) -> Result<Option<String>, Error> {
    match files::read_all(host, Root::Data, FILE, MAX)? {
        Some(bytes) => Ok(String::from_utf8(bytes).ok()),
        None => Ok(None),
    }
}

/// Whether the hints of `screen` were shown on this device. Where what was
/// shown cannot be read (the memory card taken out, #145) they count as
/// shown: a card PaperPaw cannot read is not one it never saw.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn seen(host: &Host<'_>, screen: &str) -> Result<bool, Error> {
    match read(host) {
        Ok(Some(text)) => Ok(lines_have(&text, screen)),
        Ok(None) => Ok(false),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(true),
    }
}

fn lines_have(text: &str, screen: &str) -> bool {
    text.lines().any(|line| line.trim() == screen)
}

/// The hints of `screen` were shown; whether that is kept.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn mark_seen(host: &Host<'_>, screen: &str) -> Result<bool, Error> {
    if seen(host, screen)? {
        return Ok(true);
    }
    let line = paperpaw_ui_kit::text::concat(&[screen, "\n"])?;
    Ok(storage::append(host, Root::Data, FILE, line.as_bytes()).is_ok())
}

/// The hints of `screen` count as not shown again: they were covered before the
/// reader could read them.
///
/// # Errors
/// [`Error::NoMemory`]; a card that cannot be written is ignored.
pub fn forget_seen(host: &Host<'_>, screen: &str) -> Result<(), Error> {
    let Ok(Some(text)) = read(host) else {
        return Ok(());
    };
    let kept = without(&text, screen)?;
    let _ = files::replace(host, Root::Data, FILE, kept.as_bytes());
    Ok(())
}

fn without(text: &str, screen: &str) -> Result<String, Error> {
    let mut kept = String::new();
    for line in text.lines().filter(|line| line.trim() != screen) {
        kept.try_reserve(line.len() + 1)
            .map_err(|_| Error::NoMemory)?;
        kept.push_str(line);
        kept.push('\n');
    }
    Ok(kept)
}

/// Has every screen's hints shown again.
///
/// # Errors
/// The host's refusal other than a missing file.
pub fn again(host: &Host<'_>) -> Result<(), Error> {
    match storage::remove(host, Root::Data, FILE) {
        Ok(()) | Err(Error::NotFound) => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_is_seen_by_its_line_whatever_its_spaces() {
        let text = "reader.screen\n  files.list \nbattery.screen\n";
        assert!(lines_have(text, "files.list"));
        assert!(lines_have(text, "reader.screen"));
        assert!(!lines_have(text, "files"));
        assert!(!lines_have("", "files.list"));
    }

    #[test]
    fn forgetting_keeps_the_other_lines_and_ends_each_with_a_newline() {
        let text = "a.b\n c.d \ne.f";
        assert_eq!(without(text, "c.d").unwrap(), "a.b\ne.f\n");
        assert_eq!(without("", "c.d").unwrap(), "");
    }

    #[test]
    fn forgetting_survives_every_refused_allocation() {
        crate::fail_alloc::sweep(|| without("a.b\nc.d\ne.f\n", "c.d").map(|t| t.len()));
    }
}
