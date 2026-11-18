//! Choosing a folder for a setting: the editor slot `core.choose-folder`
//! with the setting's id. The folders of the shown one come from `core.files`.
use super::*;
use crate::native_choose::{self, MOST_FOLDERS, Pick};
use paperpaw_sdk::native::files;

/// Pages of a folder read at most (a page holds up to 32 entries).
const MOST_PAGES: usize = 64;

impl NativeFiles {
    /// The folders right inside `dir` (the top for an empty one), by name:
    /// folders only, hidden ones left out, at most [`MOST_FOLDERS`].
    fn list_folders(host: &Host<'_>, dir: &str) -> Result<Vec<String>, Error> {
        let service = match Files::open(host) {
            Ok(service) => service,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(error) => return Err(error),
        };
        let (mut input, mut out) = (buffer(files::INPUT_BYTES)?, buffer(files::OUTPUT_BYTES)?);
        let mut names = Vec::new();
        let read = (|| {
            let mut cursor = 0;
            for _ in 0..MOST_PAGES {
                let page = service.list(host, Root::Files, dir, cursor, &mut input, &mut out)?;
                for entry in page.entries() {
                    let name = entry.name();
                    if entry.is_folder()
                        && !entry.unaddressable()
                        && !name.starts_with('.')
                        && names.len() < MOST_FOLDERS
                    {
                        push(&mut names, copy(name)?)?;
                    }
                }
                match page.next() {
                    Some(next) if next != cursor && names.len() < MOST_FOLDERS => cursor = next,
                    _ => break,
                }
            }
            Ok(())
        })();
        // A handle that cannot be released is no reason to lose the folders.
        service.release(host).ok();
        read?;
        native_choose::sort(&mut names);
        Ok(names)
    }

    /// The chooser at `dir` with its folders, the focus on row `focus`.
    fn chooser_at(
        host: &Host<'_>,
        setting: String,
        dir: String,
        names: Vec<String>,
        focus: usize,
    ) -> Result<Chooser, Error> {
        let mut say = Messages::new()?;
        let picks = native_choose::picks(&dir, names.len())?;
        let mut items = Vec::new();
        for pick in &picks {
            let item = match pick {
                Pick::Use => Item::noted(&say.copy(host, "files.choose.use")?, "", "")?,
                Pick::Up => Item::noted(
                    &paperpaw_ui_kit::text::concat(&["‹ ", &say.copy(host, "files.choose.up")?])?,
                    "",
                    "",
                )?,
                Pick::New => Item::noted(&say.copy(host, "files.choose.new")?, "+", "")?,
                Pick::Folder(at) => {
                    let name = names.get(*at).map_or("", String::as_str);
                    Item::noted(&paperpaw_ui_kit::text::concat(&[name, "/"])?, "›", "")?
                }
            };
            push(&mut items, item)?;
        }
        let mut list = ItemList::new(items);
        list.set_focus(focus);
        Ok(Chooser {
            setting,
            dir,
            names,
            picks,
            list,
        })
    }

    /// Opens the chooser for the setting the hand-over names, on the folder the
    /// setting has now (the top where it is gone).
    pub(super) fn open_chooser(
        &self,
        host: &Host<'_>,
        handover: Option<HandoverId>,
    ) -> Result<Chooser, Error> {
        let [setting, _] = self.handover_params(host, handover, ["id", VALUE])?;
        let mut scratch = [0; 512];
        let current = match settings::read_setting(host, &setting, &mut scratch) {
            Ok(SettingValue::Text(text)) => copy(text.trim_matches('/'))?,
            Ok(_) | Err(Error::Denied | Error::NotFound | Error::NoSpace | Error::Invalid) => {
                copy("")?
            }
            Err(error) => return Err(error),
        };
        let (dir, names) = match Self::list_folders(host, &current) {
            Ok(names) => (current, names),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // A folder that is gone: the top.
            Err(_) => (copy("")?, Self::list_folders(host, "")?),
        };
        Self::chooser_at(host, setting, dir, names, 0)
    }

    /// Shows the folder `dir` on the chooser of `token`, the focus on the
    /// folder `from` (the one just left going up) when there is one.
    fn go_to(
        &mut self,
        host: &Host<'_>,
        token: u32,
        dir: String,
        from: Option<&str>,
    ) -> Result<(), Error> {
        let Some(View::Chooser(old)) = self.screens.top(token) else {
            return Ok(());
        };
        let setting = copy(&old.setting)?;
        let names = match Self::list_folders(host, &dir) {
            Ok(names) => names,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // A folder the host will not read lists nothing.
            Err(_) => Vec::new(),
        };
        let focus = from
            .and_then(|from| names.iter().position(|name| name == from))
            .map_or(0, |at| at + if dir.is_empty() { 2 } else { 3 });
        let fresh = Self::chooser_at(host, setting, dir, names, focus)?;
        if let Some(View::Chooser(old)) = self.screens.top(token) {
            *old = fresh;
        }
        redraw(host)
    }

    /// Back goes up a folder first and leaves the chooser at the top; whether it
    /// went up.
    pub(super) fn chooser_back(&mut self, host: &Host<'_>, token: u32) -> Result<bool, Error> {
        let Some(View::Chooser(view)) = self.screens.top(token) else {
            return Ok(false);
        };
        if view.dir.is_empty() {
            return Ok(false);
        }
        let from = copy(native_choose::last(&view.dir))?;
        let dir = copy(native_choose::parent(&view.dir))?;
        self.go_to(host, token, dir, Some(&from))?;
        Ok(true)
    }

    /// The row at `index` of the chooser on `token` was chosen.
    pub(super) fn chose_folder(
        &mut self,
        host: &Host<'_>,
        token: u32,
        index: usize,
    ) -> Result<(), Error> {
        let Some(View::Chooser(view)) = self.screens.top(token) else {
            return Ok(());
        };
        let Some(pick) = view.picks.get(index).copied() else {
            return Ok(());
        };
        match pick {
            Pick::Use => {
                let mut scratch = buffer(1024)?;
                let mut sender = CommandSender::new(host, &mut scratch);
                if !view.setting.is_empty() {
                    sender.set_setting(&view.setting, SettingValue::Text(&view.dir))?;
                }
                sender.back()
            }
            Pick::Up => self.chooser_back(host, token).map(|_| ()),
            Pick::Folder(at) => {
                let Some(name) = view.names.get(at) else {
                    return Ok(());
                };
                let dir = native_choose::join(&view.dir, name)?;
                self.go_to(host, token, dir, None)
            }
            Pick::New => self.ask_folder_name(host),
        }
    }

    /// Asks for the name of a new folder in the one shown.
    fn ask_folder_name(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.ensure(host)?;
        let title = Messages::new()?.copy(host, "files.choose.new")?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = self.ui.as_ref().ok_or(Error::State)?.ask(
            host,
            Question::text(&title, "")?,
            &mut input,
            &mut out,
        )?;
        self.asked = Some((request, Asked::NewFolder));
        Ok(())
    }

    /// The name typed: the folder is made and shown. A name no file system
    /// takes, or a folder that cannot be made, changes nothing.
    pub(super) fn named_folder(
        &mut self,
        host: &Host<'_>,
        request: RequestId,
    ) -> Result<(), Error> {
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let typed = match uitext::read_all(ui, host, request.text(), 255) {
            Ok(typed) => typed,
            Err(Error::NoSpace) => return Ok(()),
            Err(error) => return Err(error),
        };
        let Some(name) = native_choose::folder_name(&typed)? else {
            return Ok(());
        };
        let Some(View::Chooser(view)) = self.screens.top(CHOOSE_TOKEN) else {
            return Ok(());
        };
        let path = native_choose::join(&view.dir, &name)?;
        let service = match Files::open(host) {
            Ok(service) => service,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(()),
        };
        let (mut input, mut out) = (buffer(files::INPUT_BYTES)?, buffer(files::OUTPUT_BYTES)?);
        let made = service.make_dir(host, Root::Files, &path, &mut input, &mut out);
        service.release(host).ok();
        match made {
            Ok(()) => self.go_to(host, CHOOSE_TOKEN, path, None),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(()),
        }
    }
}
