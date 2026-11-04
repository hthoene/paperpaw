//! The row of hints in a language from the card (a store language pack,
//! ADR 0033, #204): its words are the language's even where the screen
//! below read so much of the catalog when it opened that no more would be
//! read until the messages read are forgotten.

use super::*;
use paperpaw_platform::buttons;
use paperpaw_sdk::{Hint, Params};

const WORDY: Id = Id::new_static("wordy.list");

/// Messages of the catalog a [`Wordy`] screen reads when it opens: more
/// than the most that is read of a catalog on the card at once.
const FILLERS: usize = 600;

/// A screen that reads [`FILLERS`] messages when it is made, as a list of
/// every plugin's name and description does.
struct Wordy;

impl Screen for Wordy {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

struct WordyPlugin;

impl Plugin for WordyPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("wordy"), "Wordy", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            WORDY,
            "wordy",
            Box::new(|env, _| {
                for i in 0..FILLERS {
                    let _ = env.i18n().tr(&alloc::format!("filler.{i:04}"));
                }
                Box::new(Wordy)
            }),
        ))
    }
}

/// A card with the data plugin `pack`: the language `xx`, whose words for
/// *Back* and *Select* are `Atrás` and `Elegir`, and many more messages.
fn card() -> paperpaw_sdk::MemoryStorage {
    let mut source = String::from(
        "[meta]\nlanguage = xx\nname = Xx\n[messages]\n\
         core.hint.core.back = Atrás\ncore.hint.core.confirm = Elegir\n",
    );
    for i in 0..FILLERS {
        source.push_str(&alloc::format!(
            "filler.{i:04} = Un mensaje bastante largo de la interfaz, el número {i}\n"
        ));
    }
    let catalog = paperpaw_i18n::Catalog::compile(&source).unwrap();
    paperpaw_sdk::MemoryStorage::new()
        .with(
            "plugins/pack/plugin.ini",
            b"[plugin]\nname = Pack\nformats = 4\n".to_vec(),
        )
        .with("plugins/pack/i18n/xx.ppcat", catalog)
}

fn kernel(language: &str) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(WordyPlugin)
        .storage(card())
        .button_row(&[
            buttons::BACK,
            buttons::CONFIRM,
            buttons::LEFT,
            buttons::RIGHT,
        ])
        .user_config(alloc::format!("[core]\nlanguage = {language}\n"))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

/// The words the row of hints shows.
fn words(kernel: &Kernel) -> Vec<&str> {
    kernel
        .hint
        .shown
        .iter()
        .filter_map(|hint| match hint {
            Hint::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_row_of_hints_is_in_the_language_after_a_screen_that_read_much() {
    let mut kernel = kernel("xx");
    kernel.open_screen(Instant(10), WORDY, Params::new());
    assert_eq!(words(&kernel), ["Atrás", "Elegir"]);
}

#[test]
fn the_row_of_hints_follows_a_language_chosen_while_running() {
    let mut kernel = kernel("en");
    kernel.open_screen(Instant(10), WORDY, Params::new());
    assert_eq!(words(&kernel), ["Back", "Select"]);
    kernel
        .set_setting(Instant(20), settings::LANGUAGE.as_str(), "xx")
        .unwrap();
    assert_eq!(words(&kernel), ["Atrás", "Elegir"]);
    kernel
        .set_setting(Instant(30), settings::LANGUAGE.as_str(), "de")
        .unwrap();
    assert_eq!(words(&kernel), ["Zurück", "Wählen"]);
}
