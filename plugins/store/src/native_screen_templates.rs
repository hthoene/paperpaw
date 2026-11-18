//! The setup templates' page (#72), a step of the setup guide: a few ready-made
//! sets of plugins and settings made by PaperPaw
//! ([`native_template`](crate::native_template)), and a last row that keeps
//! the defaults. Choosing a row applies it at once and closes the page (the
//! guide goes on); *Back* leaves everything as it is.
//!
//! Memory: the words are read once when the page opens (five rows, one hint);
//! a frame reads nothing.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error,
    callbacks::{DrawFrame, Input},
};
use paperpaw_sdk::{Handled, Storage};
use paperpaw_ui_kit::mem::push;
use paperpaw_ui_kit::{Footer, Item, ItemList, ItemStep, RowItem, text};

use crate::native_screen_list::{HostSay, Say};
use crate::native_screen_offer::have_of;
use crate::native_screens::{Ctx, Installs, Next, Screen};
use crate::native_template::{TEMPLATES, Template, chosen, keep, plan};

/// The words of the page, read once.
#[derive(Debug, Default)]
pub struct Texts {
    /// The title of the page.
    pub title: String,
    /// The name and the one line of each template, in the order of the rows.
    pub rows: Vec<(String, String)>,
    /// The value of the template chosen before.
    pub chosen: String,
    /// The last row: nothing changes.
    pub keep: String,
    /// The hint under the rows.
    pub hint: String,
}

/// Reads the words of the page.
///
/// # Errors
/// `NoMemory`, or what the catalog says.
pub fn texts(say: &mut dyn Say) -> Result<Texts, Error> {
    let mut rows = Vec::new();
    rows.try_reserve_exact(TEMPLATES.len())
        .map_err(|_| Error::NoMemory)?;
    for template in &TEMPLATES {
        rows.push((say.copy(template.name)?, say.copy(template.help)?));
    }
    Ok(Texts {
        title: say.copy("store.template.title")?,
        rows,
        chosen: say.copy("store.template.chosen")?,
        keep: say.copy("store.template.keep")?,
        hint: say.copy("store.template.hint")?,
    })
}

/// The rows: each template (what it is below its name, *Chosen* at the right
/// where it was chosen before), then the one that keeps the defaults.
///
/// # Errors
/// `NoMemory`.
pub fn items(before: Option<&Template>, texts: &Texts) -> Result<Vec<Item>, Error> {
    let mut items = Vec::new();
    items
        .try_reserve_exact(texts.rows.len() + 1)
        .map_err(|_| Error::NoMemory)?;
    for (template, (name, help)) in TEMPLATES.iter().zip(&texts.rows) {
        let here = before.is_some_and(|before| before.id == template.id);
        push(
            &mut items,
            Item::Row(RowItem {
                title: text::copy(name)?,
                value: if here {
                    text::copy(&texts.chosen)?
                } else {
                    String::new()
                },
                note: text::copy(help)?,
                ..RowItem::default()
            }),
        )?;
    }
    push(
        &mut items,
        Item::Row(RowItem {
            title: text::copy(&texts.keep)?,
            ..RowItem::default()
        }),
    )?;
    Ok(items)
}

/// Chooses `template`: its plugins and the settings it sets are kept for the
/// plugin to carry out.
///
/// # Errors
/// `NoMemory`; `Io` where the card refuses the queue.
pub fn choose_template(
    installs: &dyn Installs,
    data: &dyn Storage,
    template: &'static Template,
) -> Result<Next, Error> {
    let have = |id: &str| have_of(installs.have(id));
    let plan = plan(template, &have, installs.can_install(), installs.online())?;
    keep(data, template, &plan)?;
    Ok(Next::Templated {
        settings: template.settings,
        switch_on: plan.switch_on,
        queued: !plan.fetch.is_empty() && !plan.later,
        notify: plan.notify,
    })
}

/// The page.
#[derive(Debug)]
pub struct View {
    texts: Texts,
    list: ItemList,
}

impl Screen for View {
    fn open(ctx: &Ctx<'_>, _: &[(&str, &str)]) -> Result<Self, Error> {
        let texts = texts(&mut HostSay::new(ctx.host)?)?;
        let list = ItemList::new(items(chosen(ctx.data)?, &texts)?);
        Ok(Self { texts, list })
    }

    fn draw(&mut self, ctx: &Ctx<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        self.list.draw_with(
            ctx.host,
            frame,
            &self.texts.title,
            true,
            Footer::Note(&self.texts.hint),
        )
    }

    fn input(&mut self, ctx: &Ctx<'_>, input: &Input<'_>) -> Result<(Handled, Next), Error> {
        let step = self.list.input(ctx.host, input)?;
        let next = match step {
            ItemStep::Chosen(index) => match TEMPLATES.get(index) {
                Some(template) => choose_template(ctx.installs, ctx.data, template)?,
                None => Next::Close,
            },
            _ => Next::None,
        };
        Ok((ItemList::handled(step), next))
    }
}
