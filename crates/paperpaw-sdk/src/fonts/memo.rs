//! The fonts bound to the built-in roles, found once: a role is looked up
//! through the user's choice, the chosen family, the proposals and the
//! extensions of the font, and every text call asks for one. What was found
//! is forgotten by every change of the fonts, so it is never stale.

use core::cell::Cell;

use super::{FontHandle, Fonts, OTHERS, roles};

/// The roles that are remembered.
const ROLES: usize = 7;

/// Of a handle: everything but the references into the fonts.
#[derive(Clone, Copy)]
pub(super) struct Parts {
    index: u16,
    others: [Option<u16>; OTHERS],
    larger: Option<u16>,
    display: bool,
    kerning: bool,
    ligatures: bool,
    ink: paperpaw_text::TextInk,
}

#[derive(Clone, Copy)]
pub(super) enum Memo {
    Unknown,
    Missing,
    Found(Parts),
}

/// What the kernel found of the fonts of the built-in roles ([`Fonts::role_in`],
/// [`Fonts::bound_in`]): kept by the kernel beside the fonts, not in them, so
/// that the fonts are the same for every plugin. It forgets with
/// [`RoleMemo::forget`], which the kernel calls on every change of the fonts.
pub struct RoleMemo([Cell<Memo>; 2 * ROLES]);

impl core::fmt::Debug for RoleMemo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("RoleMemo")
    }
}

impl Default for RoleMemo {
    fn default() -> Self {
        Self::new()
    }
}

fn slot(role: &str) -> Option<usize> {
    [
        roles::UI,
        roles::READING,
        roles::READING_BOLD,
        roles::READING_ITALIC,
        roles::READING_BOLD_ITALIC,
        roles::STATUS,
        roles::ICONS,
    ]
    .iter()
    .position(|known| known.as_str() == role)
}

impl RoleMemo {
    /// A memo that knows nothing.
    pub const fn new() -> Self {
        #[allow(
            clippy::declare_interior_mutable_const,
            reason = "only the value of an array repeat"
        )]
        const UNKNOWN: Cell<Memo> = Cell::new(Memo::Unknown);
        Self([UNKNOWN; 2 * ROLES])
    }

    /// Forgets what was found: after every change of the fonts, the roles,
    /// the family or the files open.
    pub fn forget(&self) {
        for memo in &self.0 {
            memo.set(Memo::Unknown);
        }
    }

    /// `make()`, done once per role until the next change; `bound` tells
    /// the two questions apart ([`Fonts::bound`], [`Fonts::role`]).
    pub(super) fn found<'a>(
        &self,
        fonts: &'a Fonts,
        bound: bool,
        role: &str,
        make: impl FnOnce() -> Option<FontHandle<'a>>,
    ) -> Option<FontHandle<'a>> {
        let Some(slot) = slot(role) else {
            return make();
        };
        let cell = &self.0[usize::from(bound) * ROLES + slot];
        match cell.get() {
            Memo::Missing => return None,
            Memo::Found(parts) => {
                if let Some(handle) = fonts.rebuilt(parts) {
                    return Some(handle);
                }
            }
            Memo::Unknown => {}
        }
        let found = make();
        cell.set(found.as_ref().map_or(Memo::Missing, |handle| {
            Memo::Found(Parts {
                index: handle.index,
                others: handle.others.map(|other| other.map(|(_, n)| n)),
                larger: handle.larger.map(|(_, n)| n),
                display: handle.display,
                kerning: handle.kerning,
                ligatures: handle.ligatures,
                ink: handle.ink,
            })
        }));
        found
    }
}

impl Fonts {
    fn rebuilt(&self, parts: Parts) -> Option<FontHandle<'_>> {
        let entry = self.entries.get(usize::from(parts.index))?;
        let font = |n: u16| self.entries.get(usize::from(n))?.font.as_ref();
        let mut others = [None; OTHERS];
        for (slot, n) in others.iter_mut().zip(parts.others) {
            *slot = n.map(|n| font(n).map(|f| (f, n))).unwrap_or(None);
            if n.is_some() && slot.is_none() {
                return None;
            }
        }
        let larger = match parts.larger {
            Some(n) => Some((font(n)?, n)),
            None => None,
        };
        Some(FontHandle {
            id: &entry.id,
            font: entry.font.as_ref()?,
            cache: &self.cache,
            drawing: &self.drawing,
            index: parts.index,
            others,
            larger,
            display: parts.display,
            kerning: parts.kerning,
            ligatures: parts.ligatures,
            ink: parts.ink,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::{FontSpec, Id};
    use crate::{Priority, builtin::fonts as roles};
    use paperpaw_text::{Font, Glyph, LineMetrics, PackBuilder};

    fn font(advance: f32) -> Font {
        let mut builder = PackBuilder::new();
        builder.add_strike(
            10.0,
            LineMetrics::new(8.0, 2.0, 12.0),
            [('a', Glyph::empty(advance))].into(),
        );
        Font::from_data(builder.build(), &[]).unwrap()
    }

    fn memoized(memo: &RoleMemo, role: &str) -> bool {
        let at = slot(role).unwrap();
        [at, ROLES + at]
            .iter()
            .any(|at| matches!(memo.0[*at].get(), Memo::Found(_) | Memo::Missing))
    }

    /// A role is looked up once, answered from then on exactly as it was
    /// found, and looked up again after the memo is forgotten (as the
    /// kernel does with every change of the fonts).
    #[test]
    fn a_role_is_found_once_and_forgotten_on_change() {
        let mut fonts = Fonts::new(4096);
        let memo = RoleMemo::new();
        fonts
            .add(FontSpec::new(Id::new_static("a.one"), font(5.0)))
            .unwrap();
        fonts
            .add(FontSpec::new(Id::new_static("a.two"), font(6.0)))
            .unwrap();
        fonts.propose(roles::UI, Id::new_static("a.one"), Priority::DEFAULT);
        let binding = roles::UI;
        let ui = binding.as_str();
        assert!(!memoized(&memo, ui));
        let first = fonts.role_in(&memo, ui).unwrap();
        assert!(memoized(&memo, ui));
        let again = fonts.role_in(&memo, ui).unwrap();
        assert_eq!(first.id(), again.id());
        assert_eq!(first.measure(10.0, "a"), again.measure(10.0, "a"));
        assert_eq!(fonts.role(ui).unwrap().id(), again.id());
        assert!(fonts.bound_in(&memo, roles::ICONS.as_str()).is_none());
        assert!(memoized(&memo, roles::ICONS.as_str()), "a miss is kept");
        // A change: the user binds the other font.
        fonts.bind_user(roles::UI, Id::new_static("a.two"));
        memo.forget();
        assert!(!memoized(&memo, ui));
        assert_eq!(fonts.role_in(&memo, ui).unwrap().id().as_str(), "a.two");
        // A font proposed later is found where nothing was before.
        assert!(
            fonts
                .bound_in(&memo, roles::READING_BOLD.as_str())
                .is_none()
        );
        fonts.propose(
            roles::READING_BOLD,
            Id::new_static("a.one"),
            Priority::DEFAULT,
        );
        memo.forget();
        assert!(!memo.0.iter().any(|m| matches!(m.get(), Memo::Found(_))));
        assert!(
            fonts
                .bound_in(&memo, roles::READING_BOLD.as_str())
                .is_some()
        );
    }
}
