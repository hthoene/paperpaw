//! The kernel's side of the developer link (`developer_link.rs` has the
//! rules): the question that switches it on, the lines it answers, and
//! every way it ends.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use paperpaw_platform::{ButtonState, InputEvent};
use paperpaw_sdk::builtin::{settings, slots};
use paperpaw_sdk::{Id, Params, SettingValue};

use crate::developer_link::{
    self as link, Command, DATA, DENIED, MAX_GET, MAX_PUT, Put, QUIET_MS, Root, Use,
};
use crate::native_host::firmware_confirm::{self, Texts};
use crate::{Effects, Instant, Kernel};

/// What follows the target's name for a file being put.
const TEMP: &str = ".pp-put";

/// Says `text` as the answer to command `n`.
fn say(out: &mut dyn FnMut(&str), n: u32, text: &str) {
    out(&format!("{}{n} {text}", link::PREFIX));
}

impl Kernel {
    /// Whether the developer link is on.
    pub fn developer_on(&self) -> bool {
        self.developer.on
    }

    /// Why the link ended, once, for the shell to tell the computer
    /// (`DEV/1 0 closed <why>`).
    pub fn take_developer_notice(&mut self) -> Option<&'static str> {
        self.developer.notice.take()
    }

    /// The reader switched the row on or off ([`link::SETTING`]): off ends
    /// the link; on asks first, on the device (rule 1). The setting itself
    /// is never stored: its value mirrors the link.
    pub(crate) fn developer_switch(&mut self, value: &SettingValue) {
        if *value != SettingValue::Bool(true) {
            self.developer_off("switched off");
            self.developer.notice = None;
            return;
        }
        // Only the reader's own press asks, never a plugin's command.
        if self.developer.on || self.developer.asked.is_some() || !self.shared.reader {
            return;
        }
        let mut bytes = [0u8; 16];
        if self.guard().is_some() || !self.shared.random.fill(&mut bytes) {
            return;
        }
        let nonce = u128::from_le_bytes(bytes);
        let tr = &self.shared.translator;
        let said = Texts {
            title: String::from(tr.tr("core.usb-link.title")),
            body: String::from(tr.tr("core.usb-link.text")),
            yes: String::from(tr.tr("core.usb-link.yes")),
            no: String::from(tr.tr("core.usb-link.no")),
            data: None,
        };
        let params = firmware_confirm::open_params(&said, nonce);
        let before = self.stack.len();
        self.open(firmware_confirm::SCREEN, None, &params);
        if self.stack.len() > before {
            self.developer.asked = Some(nonce);
        }
    }

    /// The answer of the question, which only its screen gives, with its
    /// nonce, once (see `screens.rs`); `false` if it is not the link's.
    pub(crate) fn developer_answer(&mut self, params: &Params) -> bool {
        let Some((nonce, answer)) =
            firmware_confirm::read_answer(&firmware_confirm::ANSWERED, params)
        else {
            return false;
        };
        if self.developer.asked != Some(nonce) {
            return false;
        }
        self.developer.asked = None;
        if answer == crate::native_host::firmware_install::Answer::Yes {
            self.developer.switch_on(self.shared.now.0);
            self.developer_row(true);
        }
        true
    }

    /// The device sleeps or stops: the link ends and a question shown is
    /// dismissed (rule 4).
    pub(crate) fn developer_suspend(&mut self) {
        if let Some(nonce) = self.developer.asked.take() {
            self.firmware_close_screen(nonce);
        }
        self.developer_off("sleep");
    }

    /// Ends the link for `why` (rule 4), e.g. the cable was pulled: what a
    /// put had written is taken away, the row and the mark follow.
    pub fn developer_close(&mut self, now: Instant, why: &'static str) -> Effects {
        self.shared.now = now;
        self.developer_off(why);
        self.run_commands();
        self.finish()
    }

    fn developer_off(&mut self, why: &'static str) {
        let was_on = self.developer.on;
        self.developer_drop_put();
        self.developer.switch_off(why);
        if was_on {
            self.developer_row(false);
        }
    }

    /// The row says what the link is; not stored, not told to plugins as a
    /// change they may act on beyond the usual event.
    fn developer_row(&mut self, on: bool) {
        if self
            .shared
            .settings
            .set(link::SETTING, SettingValue::Bool(on))
            .is_ok()
            && let Ok(id) = Id::parse(link::SETTING)
        {
            let changed = paperpaw_sdk::builtin::events::SettingChanged::new(id);
            self.output.commands.push(crate::Ask::emit(
                &paperpaw_sdk::builtin::events::SETTING_CHANGED,
                changed,
            ));
        }
        let (mark, _) = self.developer_mark_area();
        self.invalidate(mark);
        self.output
            .request_refresh(paperpaw_platform::RefreshMode::Fast);
    }

    /// Time passes: a link quiet for ten minutes ends, a put with no data
    /// for a while is given up (rules 4 and 8). Cheap; asked while on only.
    pub fn developer_poll(&mut self, now: Instant) -> Effects {
        if self.developer.idle(now.0) {
            return self.developer_close(now, "idle");
        }
        if self
            .developer
            .put
            .as_ref()
            .is_some_and(|put| now.0.saturating_sub(put.last) >= QUIET_MS)
        {
            self.developer_drop_put();
        }
        Effects::default()
    }

    /// A put under way is given up, its temporary file removed.
    fn developer_drop_put(&mut self) {
        if let Some(put) = self.developer.put.take() {
            let _ = self.shared.storage.remove(&put.temp);
        }
    }

    /// A line of the developer link: the rules decide, the answer goes to
    /// `out` a line at a time (`heap` is what the shell says of its heap:
    /// used and free). The effects are those of a key or an action.
    pub fn developer_line(
        &mut self,
        now: Instant,
        line: &str,
        heap: (usize, usize),
        out: &mut dyn FnMut(&str),
    ) -> Effects {
        let Some((n, command)) = link::parse(line) else {
            return Effects::default();
        };
        // Off: nothing is done and little is said (rule 1).
        if !self.developer.on {
            if self.developer.may_say_off(now.0) {
                say(out, n, "err off");
            }
            return Effects::default();
        }
        if matches!(command, Ok(Command::Data(_))) && self.developer.put.is_some() {
            self.developer.touch(now.0);
        } else if !self.developer.admit(now.0) {
            say(out, n, "err busy");
            return Effects::default();
        }
        self.shared.now = now;
        let command = match command {
            Ok(command) => command,
            Err(why) => {
                say(out, n, &format!("err {why}"));
                return Effects::default();
            }
        };
        let mut effects = Effects::default();
        match self.developer_run(now, n, command, heap, out, &mut effects) {
            Ok(()) => {}
            Err(why) => {
                say(out, n, &format!("err {why}"));
                if why == DENIED && self.developer.refused(now.0) {
                    self.developer_off("probing");
                    self.run_commands();
                    effects = self.finish();
                }
            }
        }
        effects
    }

    /// Whether a question or a lock is on the screen: only the reader's
    /// own hand answers it (rule 7).
    fn developer_question_up(&self) -> bool {
        let confirm = self.shared.registry.slot_screen(slots::CONFIRM.as_str());
        self.guard().is_some()
            || self
                .stack
                .last()
                .is_some_and(|top| Some(&top.id) == confirm || top.id == firmware_confirm::SCREEN)
    }

    fn developer_library(&self) -> String {
        String::from(self.shared.settings.read(&settings::LIBRARY_FOLDER))
    }

    fn developer_run(
        &mut self,
        now: Instant,
        n: u32,
        command: Command<'_>,
        heap: (usize, usize),
        out: &mut dyn FnMut(&str),
        effects: &mut Effects,
    ) -> Result<(), &'static str> {
        if self.developer.put.is_some() && !matches!(command, Command::Data(_)) {
            self.developer_drop_put();
            return Err("put aborted");
        }
        match command {
            Command::Hello | Command::Ping => say(out, n, "ok"),
            Command::Close => {
                self.developer_off("closed");
                self.developer.notice = None;
                self.run_commands();
                *effects = self.finish();
                say(out, n, "ok");
            }
            Command::Screen => {
                let id = self.active_screen().map_or("", |id| id.as_str());
                say(out, n, &format!("ok {id}"));
            }
            Command::Heap => say(out, n, &format!("ok {} {}", heap.0, heap.1)),
            Command::Key(name) => {
                if self.developer_question_up() {
                    return Err("question");
                }
                let down = InputEvent::Button {
                    name,
                    state: ButtonState::Pressed,
                };
                let up = InputEvent::Button {
                    name,
                    state: ButtonState::Released,
                };
                let pressed = self.handle(now, down);
                *effects = pressed.and(self.handle(now, up));
                say(out, n, "ok");
            }
            Command::Run(action) => {
                if self.developer_question_up() {
                    return Err("question");
                }
                let id = Id::parse(action).map_err(|_| "bad id")?;
                *effects = self.run_action(now, id);
                say(out, n, "ok");
            }
            Command::Shot => self.developer_shot(n, out)?,
            Command::Describe => self.developer_describe(n, out),
            Command::Ls(path) => self.developer_ls(n, path, out)?,
            Command::Get(path) => self.developer_get(n, path, out)?,
            Command::Md5(path) => self.developer_md5(n, path, out)?,
            Command::Rm(path) => self.developer_rm(n, path, out)?,
            Command::Put { size, path } => self.developer_put(now, n, size, path, out)?,
            Command::Data(digits) => self.developer_data(now, digits, out)?,
        }
        Ok(())
    }

    /// The whole screen, as turned: `frame <w> <h> <bits>`, `row <y> <runs>`
    /// (a count and a byte, in hexadecimal, as the test remote's), `end`.
    fn developer_shot(&mut self, n: u32, out: &mut dyn FnMut(&str)) -> Result<(), &'static str> {
        let display = self.shared.device.display;
        let header = (
            display.width,
            display.height,
            display.format.bits_per_pixel(),
        );
        let mut started = false;
        let mut text = String::new();
        let done = self.capture(&mut |top, rows| {
            if !started {
                say(
                    out,
                    n,
                    &format!("frame {} {} {}", header.0, header.1, header.2),
                );
                started = true;
            }
            for y in 0..rows.height {
                let at = usize::from(y) * rows.stride;
                let row = rows.data.get(at..at + rows.stride).unwrap_or(&[]);
                text.clear();
                let _ = write!(text, "row {} ", top + y);
                let mut rest = row;
                while let Some(&byte) = rest.first() {
                    let count = rest.iter().take(255).take_while(|b| **b == byte).count();
                    let _ = write!(text, "{count:02x}{byte:02x}");
                    rest = &rest[count..];
                }
                say(out, n, &text);
            }
        });
        if !done {
            return Err("busy");
        }
        say(out, n, "ok");
        Ok(())
    }

    /// What the screen shows, part by part; where it is secret (a PIN being
    /// typed, the device locked) what the parts say is left out.
    fn developer_describe(&mut self, n: u32, out: &mut dyn FnMut(&str)) {
        let secret = self.is_secret();
        for part in self.describe() {
            let mut state = Vec::new();
            for (yes, word) in [
                (part.focused, "focused"),
                (part.on == Some(true), "on"),
                (part.on == Some(false), "off"),
                (part.disabled, "disabled"),
            ] {
                if yes {
                    state.push(word);
                }
            }
            let state = if state.is_empty() {
                String::from("-")
            } else {
                state.join("+")
            };
            let a = part.area;
            let label = if secret {
                String::from("•")
            } else {
                part.label.replace('\n', " ")
            };
            say(
                out,
                n,
                &format!(
                    "part {:?} {} {} {} {} {state} {label}",
                    part.role, a.x, a.y, a.width, a.height
                ),
            );
        }
        say(out, n, "ok");
    }

    fn developer_ls(
        &self,
        n: u32,
        raw: &str,
        out: &mut dyn FnMut(&str),
    ) -> Result<(), &'static str> {
        let library = self.developer_library();
        let (root, path) = link::path(raw, &library, Use::Read)?;
        if root == Root::Top {
            for name in [library.as_str(), "plugins", "screenshots"] {
                say(out, n, &format!("entry d 0 {name}"));
            }
            say(out, n, "ok");
            return Ok(());
        }
        self.shared
            .storage
            .list(&path, &mut |entry| {
                if !link::secret(entry.name) {
                    let kind = if entry.is_dir { "d" } else { "f" };
                    say(
                        out,
                        n,
                        &format!("entry {kind} {} {}", entry.size, entry.name),
                    );
                }
            })
            .map_err(|_| "not found")?;
        say(out, n, "ok");
        Ok(())
    }

    fn developer_get(
        &self,
        n: u32,
        raw: &str,
        out: &mut dyn FnMut(&str),
    ) -> Result<(), &'static str> {
        let (_, path) = link::path(raw, &self.developer_library(), Use::Read)?;
        let storage = &*self.shared.storage;
        let size = storage.size(&path).map_err(|_| "not found")?;
        if size > MAX_GET {
            return Err("too big");
        }
        say(out, n, &format!("size {size}"));
        let mut buf = [0u8; 256];
        let mut text = String::with_capacity(buf.len() * 2 + 8);
        let mut at = 0u64;
        while at < size {
            let read = storage
                .read_at(&path, at, &mut buf)
                .map_err(|_| "unreadable")?;
            if read == 0 {
                break;
            }
            text.clear();
            text.push_str("data ");
            for byte in &buf[..read] {
                let _ = write!(text, "{byte:02x}");
            }
            say(out, n, &text);
            at += read as u64;
        }
        say(out, n, "ok");
        Ok(())
    }

    fn developer_md5(
        &self,
        n: u32,
        raw: &str,
        out: &mut dyn FnMut(&str),
    ) -> Result<(), &'static str> {
        let (_, path) = link::path(raw, &self.developer_library(), Use::Read)?;
        let storage = &*self.shared.storage;
        let mut digest = paperpaw_digest::md5::Md5::new();
        let mut buf = [0u8; 512];
        let mut at = 0u64;
        loop {
            match storage.read_at(&path, at, &mut buf) {
                Ok(0) => break,
                Ok(read) => {
                    digest.update(&buf[..read]);
                    at += read as u64;
                }
                Err(_) => return Err("not found"),
            }
        }
        say(out, n, &format!("ok {} {at}", digest.hex()));
        Ok(())
    }

    /// Removes a file of a plugin's folder that the link itself wrote.
    fn developer_rm(
        &mut self,
        n: u32,
        raw: &str,
        out: &mut dyn FnMut(&str),
    ) -> Result<(), &'static str> {
        let (_, path) = link::path(raw, &self.developer_library(), Use::Remove)?;
        if !self.developer.wrote.contains(&path) {
            return Err(DENIED);
        }
        let storage = &*self.shared.storage;
        storage
            .remove(&path)
            .and_then(|()| storage.sync())
            .map_err(|_| "not removed")?;
        self.developer.wrote.retain(|wrote| *wrote != path);
        say(out, n, "ok");
        Ok(())
    }

    /// Begins a file: written beside its target, moved over it when whole.
    fn developer_put(
        &mut self,
        now: Instant,
        n: u32,
        size: u64,
        raw: &str,
        out: &mut dyn FnMut(&str),
    ) -> Result<(), &'static str> {
        let (root, path) = link::path(raw, &self.developer_library(), Use::Write)?;
        let storage = &*self.shared.storage;
        // A code plugin's folder is its own: nothing is put beside its code.
        if root == Root::Plugins
            && link::plugin_folder(&path)
                .is_some_and(|id| storage.exists(&format!("plugins/{id}/code.ppn")))
        {
            return Err(DENIED);
        }
        if size > MAX_PUT {
            return Err("too big");
        }
        if root == Root::Library && storage.exists(&path) {
            return Err("exists");
        }
        if storage.free_space().is_some_and(|free| size > free) {
            return Err("no room");
        }
        let temp = format!("{path}{TEMP}");
        storage.write(&temp, &[]).map_err(|_| "not writable")?;
        self.developer.put = Some(Put {
            n,
            path,
            temp,
            root,
            size,
            got: 0,
            last: now.0,
        });
        say(out, n, "ready");
        if size == 0 {
            return self.developer_data(now, "", out);
        }
        Ok(())
    }

    /// The next piece of the file being put; the whole one is moved into
    /// place after its size is checked.
    fn developer_data(
        &mut self,
        now: Instant,
        digits: &str,
        out: &mut dyn FnMut(&str),
    ) -> Result<(), &'static str> {
        let Some(mut put) = self.developer.put.take() else {
            return Err("no put");
        };
        let storage = &*self.shared.storage;
        let fail = |put: &Put, why| {
            let _ = storage.remove(&put.temp);
            Err(why)
        };
        if digits.len() > DATA * 2 {
            return fail(&put, "bad data");
        }
        let Ok(Some(bytes)) = paperpaw_sdk::digest::hex::try_decode(digits) else {
            return fail(&put, "bad data");
        };
        put.got += bytes.len() as u64;
        if put.got > put.size {
            return fail(&put, "too much");
        }
        if !bytes.is_empty() && storage.append(&put.temp, &bytes).is_err() {
            return fail(&put, "not writable");
        }
        put.last = now.0;
        if put.got < put.size {
            self.developer.put = Some(put);
            return Ok(());
        }
        // Whole: on the card, and never over a book that appeared meanwhile.
        let placed = storage
            .sync()
            .ok()
            .filter(|()| storage.size(&put.temp).ok() == Some(put.size))
            .is_some()
            && !(put.root == Root::Library && storage.exists(&put.path))
            && storage.rename(&put.temp, &put.path).is_ok()
            && storage.sync().is_ok();
        if !placed {
            return fail(&put, "not placed");
        }
        if put.root == Root::Plugins {
            self.developer.wrote(put.path.clone());
        }
        say(out, put.n, &format!("ok {}", put.got));
        Ok(())
    }

    /// The answer of the confirmation screen: the link's question, else the
    /// firmware's.
    pub(crate) fn confirm_answer(&mut self, params: &Params) {
        if !self.developer_answer(params) {
            self.firmware_answer(params);
        }
    }

    /// Draws the `DEV` mark in `mark`, where it is within `region`.
    pub(crate) fn draw_developer_mark(
        canvas: &mut paperpaw_sdk::Canvas<'_>,
        env: &crate::Place<'_>,
        mark: paperpaw_sdk::Rect,
        region: &paperpaw_sdk::Rect,
    ) {
        if mark.intersection(region).is_none() {
            return;
        }
        let style = crate::kit::ui::UiStyle::new(env);
        let mut canvas = canvas.clipped(mark);
        crate::kit::ui::fill_round(&mut canvas, &style, mark, paperpaw_sdk::Color::BLACK);
        if let Some(font) = style.font(env) {
            let size = f32::from(mark.height) * 0.6;
            let x = f32::from(mark.x) + (f32::from(mark.width) - font.measure(size, "DEV")) / 2.0;
            crate::kit::ui::draw_text(
                &mut canvas,
                &font,
                None,
                size,
                x,
                mark,
                "DEV",
                paperpaw_sdk::Color::WHITE,
            );
        }
    }

    /// The mark `DEV` shown while the link is on: top left, where the busy
    /// mark (top right) is not.
    pub(crate) fn developer_mark_area(&self) -> (paperpaw_sdk::Rect, Option<usize>) {
        let metrics = paperpaw_sdk::Metrics::new(self.shared.device.display.dpi);
        let width = (metrics.mm(11) as u16).min(self.framebuffer.bounds().width / 3);
        (
            paperpaw_sdk::Rect::new(0, 0, width, (metrics.mm(5) as u16).max(12)),
            None,
        )
    }
}
