//! PaperPaw for Android e-readers (Boox and others).
//!
//! A native activity in Rust (its only Java code is the speech listener,
//! see `speech`): it runs the kernel with the
//! standard plugins, turns touch and key events into PaperPaw input and
//! copies rendered frames into the activity's window.
//!
//! Books live in the shared `Books` folder once the user allows access
//! (the home screen offers it; see `access`). Until then, the library is the
//! app's own folder (`Android/data/org.paperpaw.app/files/books`)
//! with the bundled sample books. Settings and caches always stay in the
//! app's folder.
//!
//! On Onyx Boox devices, PaperPaw's refresh requests choose the e-ink
//! waveform (see `epd`); elsewhere the device refreshes on its own.

mod code;
pub mod keys;
pub mod pixels;
pub mod private;

#[cfg(target_os = "android")]
mod access;
#[cfg(target_os = "android")]
mod app;
#[cfg(target_os = "android")]
mod clipboard;
#[cfg(target_os = "android")]
mod epd;
#[cfg(target_os = "android")]
mod lights;
#[cfg(target_os = "android")]
mod native;
#[cfg(target_os = "android")]
mod net;
#[cfg(target_os = "android")]
mod pdf;
#[cfg(target_os = "android")]
mod plugin;
#[cfg(target_os = "android")]
mod speech;
#[cfg(target_os = "android")]
mod status;

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
