//! Text chosen on the page and what is done with it: looked up,
//! highlighted, noted, copied, read aloud, passed to plugins.

use super::*;

impl ReaderScreen {
    /// The bar's items for the current selection.
    pub(super) fn items(&self, env: &Env<'_>, words: &[WordBox]) -> Vec<Item> {
        // The highlight under the selection, if any, and whether it has a
        // note.
        let highlight = match (&self.book, self.selection) {
            (Ok(book), Some(selection)) => {
                let (lo, hi) = selection.range();
                words.get(lo).zip(words.get(hi)).and_then(|(a, b)| {
                    book.marks
                        .iter()
                        .find(|m| m.kind == Kind::Highlight && m.overlaps(a.start, b.end))
                        .map(|m| m.noted)
                })
            }
            _ => None,
        };
        selection::items(env, highlight)
    }

    /// Changes the selection and redraws what it covered and covers.
    pub(super) fn set_selection(&mut self, to: Option<Selection>, cx: &mut Cx<'_, '_>) {
        let bar_before = self.selection.is_some_and(|s| s.mode == Mode::Bar);
        let bar_after = to.is_some_and(|s| s.mode == Mode::Bar);
        self.selection = to;
        if let Ok(book) = &self.book {
            self.selected_on = book.page.start;
        }
        // Fast refreshes are black and white: they would hide the gray of
        // highlights.
        let highlights = self.book.as_ref().is_ok_and(|book| {
            book.marks
                .iter()
                .any(|m| m.kind == Kind::Highlight && m.overlaps(book.page.start, book.page.end))
        });
        cx.request_redraw();
        cx.request_refresh(if bar_before != bar_after || highlights {
            RefreshMode::Partial
        } else {
            RefreshMode::Fast
        });
    }

    /// Does what a bar item stands for. Items of other plugins are
    /// announced with the selected text (event `core.text-chosen`).
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    pub(super) fn activate(&mut self, item: &Id, cx: &mut Cx<'_, '_>) {
        if *item == crate::actions::READ_FROM
            && let (Ok(book), Some(selection)) = (&self.book, self.selection)
        {
            // From the first word chosen.
            self.resume = Some((book.page.start, selection.range().0));
            self.set_selection(None, cx);
            self.start_aloud(cx);
            return;
        }
        let env = *cx.env();
        let (Ok(book), Some(selection)) = (&mut self.book, self.selection) else {
            return;
        };
        let words = book.words(&env);
        let (lo, hi) = selection.range();
        let (Some(first), Some(last)) = (words.get(lo), words.get(hi)) else {
            self.set_selection(None, cx);
            return;
        };
        let (start, end) = (first.start, last.end);
        let text = words[lo..=hi]
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if *item == crate::actions::LOOK_UP {
            let phrase = text.trim_matches(|c: char| !c.is_alphanumeric());
            if !phrase.is_empty() {
                cx.command(Command::OpenSlot(
                    slots::LOOKUP,
                    Params::new()
                        .with(params::WORD, phrase.to_string())
                        .with(params::LANGUAGE, book.language.clone()),
                ));
                cx.command(events::WORD_LOOKED_UP.emit(events::WordLookedUp::new(
                    phrase.to_string(),
                    book.language.clone(),
                    sentence(words, lo, hi),
                    book.path.clone(),
                    book.title.clone(),
                    start.section,
                    start.offset,
                )));
            }
            return;
        }
        if *item == crate::actions::MORE {
            // The whole selection menu as a list; the chosen action comes
            // back to `on_action` while the text stays selected.
            // As in the bar: what does not apply here is left out.
            let env = *cx.env();
            let mut hide = Vec::new();
            if !crate::selection::translates(&env) {
                hide.push(crate::actions::TRANSLATE.as_str().to_string());
            }
            hide.push(if env.device().input.touch {
                crate::actions::EXTEND.as_str().to_string()
            } else {
                crate::actions::CANCEL.as_str().to_string()
            });
            let hide = hide.join(",");
            cx.command(Command::OpenSlot(
                slots::MENU,
                Params::new()
                    .with(params::MENU, crate::SELECTION_MENU.as_str())
                    .with(params::HIDE, hide),
            ));
            return;
        }
        if *item == crate::actions::EXTEND {
            let to = Selection {
                mode: Mode::Extending,
                focus: None,
                ..selection
            };
            self.set_selection(Some(to), cx);
            return;
        }
        if *item == crate::actions::COPY {
            cx.command(Command::SetClipboard(text.clone()));
        } else if *item == crate::actions::TRANSLATE {
            cx.command(Command::OpenSlot(
                slots::TRANSLATE,
                Params::new()
                    .with(params::TEXT, text.clone())
                    .with(params::LANGUAGE, book.language.clone()),
            ));
        } else if *item == crate::actions::HIGHLIGHT {
            let highlight = |m: &Mark| m.kind == Kind::Highlight && m.overlaps(start, end);
            // On highlighted text the item removes the highlight.
            if !matches!(
                annotations::remove(env.storage(), &book.path, highlight),
                Ok(true)
            ) {
                let annotation = Annotation::new(
                    Mark::new(Kind::Highlight, start, end, false),
                    (book.document.progress(start) * 100.0 + 0.5) as u8,
                    paperpaw_sdk::time::unix_seconds(&env).unwrap_or(0),
                    0,
                    annotations::snippet(&text),
                    String::new(),
                );
                if annotations::add(env.storage(), &book.path, annotation).is_err() {
                    marks_not_saved(cx);
                }
            }
            book.reload_marks(&env);
            cx.command(annotations::changed(&book.path));
        } else if *item == crate::actions::NOTE {
            // On the highlight there (made if there is none), typed on
            // the keyboard.
            let existing = book
                .marks
                .iter()
                .find(|m| m.kind == Kind::Highlight && m.overlaps(start, end))
                .map(|m| m.start);
            let at = match existing {
                Some(at) => at,
                None => {
                    let highlight = Annotation::new(
                        Mark::new(Kind::Highlight, start, end, false),
                        (book.document.progress(start) * 100.0 + 0.5) as u8,
                        paperpaw_sdk::time::unix_seconds(&env).unwrap_or(0),
                        0,
                        annotations::snippet(&text),
                        String::new(),
                    );
                    if annotations::add(env.storage(), &book.path, highlight).is_err() {
                        marks_not_saved(cx);
                    }
                    start
                }
            };
            book.reload_marks(&env);
            cx.command(annotations::changed(&book.path));
            let note = annotations::note_of(env.storage(), &book.path, at);
            cx.command(
                ask::text(env.i18n().tr("reader.note.prompt"), &crate::NOTE_ENTERED)
                    .context(&crate::toc::locator_params(&book.path, at))
                    .initial(note)
                    .command(),
            );
        } else if *item != crate::actions::CANCEL {
            cx.command(events::TEXT_CHOSEN.emit(events::TextChosen::new(
                item.clone(),
                text,
                book.language.clone(),
                book.path.clone(),
                start.section,
                start.offset,
                end.section,
                end.offset,
            )));
            // Then the action runs as usual (after the selection is gone),
            // so any action works here, e.g. the night mode.
            cx.command(Command::run(item.clone()));
        }
        self.set_selection(None, cx);
        cx.request_refresh(RefreshMode::Partial);
    }

    pub(super) fn on_selection(
        &mut self,
        action: &Id,
        selection: Selection,
        cx: &mut Cx<'_, '_>,
    ) -> Handled {
        let env = *cx.env();
        let Ok(book) = &self.book else {
            return Handled::No;
        };
        let words = book.words(&env);
        if words.is_empty() {
            self.set_selection(None, cx);
            return Handled::No;
        }
        let a = action;
        if *a == actions::BACK || *a == actions::MENU {
            self.set_selection(None, cx);
            return Handled::Yes;
        }
        // An action of the selection menu, e.g. chosen in its list.
        if env
            .menus()
            .items(crate::SELECTION_MENU.as_str())
            .iter()
            .any(|item| item.id == a)
        {
            self.activate(a, cx);
            return Handled::Yes;
        }
        match selection.mode {
            Mode::Choosing | Mode::Extending => {
                if *a == actions::CONFIRM {
                    let to = Selection {
                        mode: Mode::Bar,
                        focus: Some(0),
                        ..selection
                    };
                    self.set_selection(Some(to), cx);
                } else if let Some(cursor) = selection.moved(a, words) {
                    let anchor = if selection.mode == Mode::Choosing {
                        cursor
                    } else {
                        selection.anchor
                    };
                    let to = Selection {
                        anchor,
                        cursor,
                        ..selection
                    };
                    self.set_selection(Some(to), cx);
                } else {
                    return Handled::No;
                }
            }
            Mode::Bar => {
                let items = self.items(&env, words);
                let focus = selection.focus.unwrap_or(0);
                if *a == actions::CONFIRM {
                    if let Some(item) = items.get(focus) {
                        self.activate(&item.id.clone(), cx);
                    }
                } else if [
                    actions::LEFT,
                    actions::UP,
                    actions::PREVIOUS,
                    actions::PREV_PAGE,
                ]
                .contains(a)
                {
                    let to = Selection {
                        focus: Some(ui::step_focus(focus, -1, items.len())),
                        ..selection
                    };
                    self.set_selection(Some(to), cx);
                } else if [
                    actions::RIGHT,
                    actions::DOWN,
                    actions::NEXT,
                    actions::NEXT_PAGE,
                ]
                .contains(a)
                {
                    let to = Selection {
                        focus: Some(ui::step_focus(focus, 1, items.len())),
                        ..selection
                    };
                    self.set_selection(Some(to), cx);
                } else {
                    return Handled::No;
                }
            }
        }
        Handled::Yes
    }
}
