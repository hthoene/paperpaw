//! Samples of fonts before they are downloaded: for each, a font pack of
//! one size with only the characters of its name and of a sentence in
//! the reader's language, made by the font library on request (a few
//! kilobytes), kept on the storage and drawn as any text. One is fetched
//! at a time; those of the fonts shown are held in memory, as many as
//! [`budget`] allows.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::{
    Command, Commands, Env, FontHandle, FontSet, HttpRequest, Id, MemoryClass, Params, Storage,
};

use crate::PLUGIN;
use crate::catalog::Font;

/// The answer with a sample.
pub(crate) const SAMPLE: Event<HttpReply> = Event::new("fonts-browser.sample");

/// Parameter of a sample's request: the font's id.
const FONT: &str = "font";
/// Largest sample read.
const MAX_SAMPLE: usize = 64 * 1024;
/// Memory for the glyphs of the samples shown.
const CACHE: usize = 4 * 1024;

/// Bytes of samples held at most. Each is read into memory whole (a
/// plugin cannot yet have a pack of its own read a glyph at a time), so
/// where memory is small the fonts past it are named in the menus' font.
fn budget(env: &Env<'_>) -> usize {
    if env.device().memory == MemoryClass::Small {
        48 * 1024
    } else {
        12 * MAX_SAMPLE
    }
}

fn dir() -> String {
    format!("{}/samples", Env::plugin_dir(PLUGIN.as_str()))
}

/// The sentence a font is shown with, in the reader's language.
pub(crate) fn sentence<'a>(env: &Env<'a>) -> &'a str {
    env.i18n().tr("fonts-browser.sample")
}

/// Its characters and the font's name, each once, and `…` for a name
/// cut short.
fn characters(env: &Env<'_>, font: &Font) -> String {
    let mut chars: Vec<char> = font
        .name
        .chars()
        .chain(sentence(env).chars())
        .chain(['…'])
        .collect();
    chars.sort_unstable();
    chars.dedup();
    chars.into_iter().filter(|c| !c.is_control()).collect()
}

/// The samples of fonts shown on a screen.
pub(crate) struct Samples {
    set: FontSet,
    /// Font ids and their sample in the set (`None`: none to be had, or
    /// no room for it).
    held: Vec<(String, Option<usize>)>,
    /// Bytes of the samples in the set.
    bytes: usize,
    /// The font whose sample is on its way.
    fetching: Option<String>,
    /// The size samples are made in, in pixels.
    pub size: u32,
}

impl Samples {
    pub(crate) fn new(style: &UiStyle) -> Self {
        // A little larger than menu text, for the letters to show.
        Self::of_size((style.text() * 1.3 + 0.5) as u32)
    }

    fn of_size(size: u32) -> Self {
        Self {
            set: FontSet::new(CACHE),
            held: Vec::new(),
            bytes: 0,
            fetching: None,
            size,
        }
    }

    /// Lets go of the samples held, e.g. while the screen is hidden;
    /// those kept on the storage are held again when wanted.
    pub(crate) fn clear(&mut self) {
        self.set = FontSet::new(CACHE);
        self.held.clear();
        self.bytes = 0;
    }

    fn is_held(&self, id: &str) -> bool {
        self.held.iter().any(|(held, _)| held == id)
    }

    /// Where the sample of `font` is kept: one for each version, size and
    /// language.
    fn path(&self, env: &Env<'_>, font: &Font) -> String {
        let version: String = font
            .version
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '.')
            .collect();
        format!(
            "{}/{}-{version}-{}-{}.ppfp",
            dir(),
            font.id,
            self.size,
            env.i18n().language()
        )
    }

    /// The sample of font `id`, if it is held.
    pub(crate) fn get(&self, id: &str) -> Option<FontHandle<'_>> {
        let index = self.held.iter().find(|(i, _)| i == id)?.1?;
        self.set.get(index)
    }

    /// Holds the samples of `fonts` kept on the storage, those of other
    /// fonts let go, and asks for the first one missing while there is
    /// room for it; `true` if one more is held.
    pub(crate) fn want(&mut self, env: &Env<'_>, fonts: &[Font], out: &mut Commands) -> bool {
        if self
            .held
            .iter()
            .any(|(id, _)| fonts.iter().all(|f| f.id != *id))
        {
            self.clear();
        }
        let budget = budget(env);
        let mut more = false;
        for font in fonts {
            if self.is_held(&font.id) {
                continue;
            }
            let path = self.path(env, font);
            if env.storage().exists(&path) {
                more |= self.hold(env.storage(), font, &path, budget);
            }
        }
        if self.fetching.is_none()
            && self.bytes < budget
            && let Some(font) = fonts
                .iter()
                .find(|f| !f.sample.is_empty() && !self.is_held(&f.id))
        {
            let separator = if font.sample.contains('?') { '&' } else { '?' };
            let url = format!(
                "{}{separator}size={}&text={}",
                font.sample,
                self.size,
                paperpaw_sdk::web::encode(&characters(env, font))
            );
            let mut request = HttpRequest::new("GET", &url, SAMPLE);
            let _ = env.storage().create_dir(&dir());
            request = request.save_to(self.path(env, font));
            request = request.with(Params::new().with(FONT, font.id.clone()));
            out.push(Command::http(request));
            self.fetching = Some(font.id.clone());
        }
        more
    }

    /// Holds the sample of `font` kept at `path` if it fits in what is
    /// left of `budget`; `true` if it is held. One without room is named
    /// in the menus' font, one that is no sample is removed; neither is
    /// asked for again while it is shown.
    fn hold(&mut self, storage: &dyn Storage, font: &Font, path: &str, budget: usize) -> bool {
        let size = storage.size(path).map_or(usize::MAX, |size| {
            usize::try_from(size).unwrap_or(usize::MAX)
        });
        let room = budget.saturating_sub(self.bytes);
        let loaded = if size > MAX_SAMPLE {
            None
        } else if size > room {
            self.held.push((font.id.clone(), None));
            return false;
        } else {
            paperpaw_sdk::read_file(storage, path, size)
                .ok()
                .and_then(|data| paperpaw_sdk::Font::from_data(data, &[]).ok())
        };
        let Some(loaded) = loaded else {
            let _ = storage.remove(path);
            self.held.push((font.id.clone(), None));
            return false;
        };
        let index = self.set.add(Id::new_static("fonts-browser.sample"), loaded);
        self.held.push((font.id.clone(), Some(index)));
        self.bytes += size;
        true
    }

    /// A sample arrived (or not): the next can be fetched.
    pub(crate) fn received(&mut self, env: &Env<'_>, reply: &HttpReply, fonts: &[Font]) {
        let id = reply.context.get(FONT).unwrap_or_default();
        if self.fetching.as_deref() != Some(id) {
            return;
        }
        self.fetching = None;
        if !(200..300).contains(&reply.status)
            && let Some(font) = fonts.iter().find(|f| f.id == id)
        {
            let _ = env.storage().remove(&self.path(env, font));
            // Not asked for again while it is shown.
            self.held.push((font.id.clone(), None));
        }
    }
}

/// Removes the samples kept of fonts or versions no longer offered.
pub(crate) fn prune(env: &Env<'_>, samples: &Samples, fonts: &[Font]) {
    let keep: Vec<String> = fonts.iter().map(|f| samples.path(env, f)).collect();
    let mut gone: Vec<String> = Vec::new();
    let dir = dir();
    let _ = env.storage().list(&dir, &mut |entry| {
        let path = format!("{dir}/{}", entry.name);
        if !keep.contains(&path) {
            gone.push(path);
        }
    });
    for path in gone {
        let _ = env.storage().remove(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use paperpaw_sdk::MemoryStorage;
    use paperpaw_text::{Glyph, LineMetrics, PackBuilder};

    /// A sample of the letters of `text`.
    fn pack(text: &str) -> Vec<u8> {
        let metrics = LineMetrics::new(20.0, 6.0, 30.0);
        let mut builder = PackBuilder::new();
        builder.add_strike(
            26.0,
            metrics,
            text.chars().map(|c| (c, Glyph::empty(9.0))).collect(),
        );
        builder.build()
    }

    #[test]
    fn samples_are_held_within_their_budget() {
        let sample = pack("Fontabc");
        let storage = MemoryStorage::new();
        let fonts: Vec<Font> = ["a", "b", "c"]
            .map(|id| {
                storage.write(&format!("{id}.ppfp"), &sample).unwrap();
                Font {
                    id: id.into(),
                    ..Font::default()
                }
            })
            .into();
        // Room for two.
        let budget = sample.len() * 5 / 2;
        let mut samples = Samples::of_size(26);
        let held: Vec<bool> = fonts
            .iter()
            .map(|font| samples.hold(&storage, font, &format!("{}.ppfp", font.id), budget))
            .collect();
        assert_eq!(held, [true, true, false]);
        assert!(samples.bytes <= budget);
        assert!(samples.get("a").is_some() && samples.get("b").is_some());
        // Named in the menus' font, kept for when there is room.
        assert!(samples.get("c").is_none());
        assert!(samples.is_held("c"));
        assert!(storage.exists("c.ppfp"));

        // A sample too large for any budget is not read.
        storage.write("large.ppfp", &[0; MAX_SAMPLE + 1]).unwrap();
        samples.clear();
        assert!(!samples.hold(&storage, &fonts[0], "large.ppfp", usize::MAX));
        assert_eq!(samples.bytes, 0);
    }
}
