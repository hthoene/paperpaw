//! What the plugin does with the host for the picture widget: lists the folder
//! (`core.files`), asks for the picture chosen (`image_prepare`), holds and gives
//! back the pictures the host answers with, and tells the widget to draw again.
//! What to choose and what an answer means is `native_picture`'s.
use crate::native_picture::{self, Folder, Next, PATH_MAX, Refusal};
use crate::native_plugin::NativeWidgets;
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    files::Files,
    image::{ImageId, ImageReady, PrepareImage, PrepareRoot},
    snapshot::capture_snapshot,
    storage::Root,
};
use paperpaw_ui_kit::{mem, text::concat, widget::Settings};

/// A folder on the card, listed a page at a time.
struct OnCard<'a, 'h> {
    host: &'a Host<'h>,
    files: &'a Files,
    path: &'a str,
    input: &'a mut [u8],
    out: &'a mut [u8],
}

impl Folder for OnCard<'_, '_> {
    fn each(&mut self, visit: &mut dyn FnMut(&str, bool)) -> Result<(), Error> {
        let mut cursor = 0;
        loop {
            let page = self.files.list(
                self.host,
                Root::Files,
                self.path,
                cursor,
                &mut *self.input,
                &mut *self.out,
            )?;
            for entry in page.entries() {
                // A name that cannot be used in a path cannot be opened.
                if !entry.unaddressable() {
                    visit(entry.name(), entry.is_folder());
                }
            }
            match page.next() {
                Some(next) => cursor = next,
                None => return Ok(()),
            }
        }
    }
}

/// Gives a picture back; a refusal leaves it with the host, which ends it with
/// the plugin.
fn release(host: &Host<'_>, image: ImageId) {
    CommandSender::new(host, &mut [0; 64])
        .image_release(image)
        .ok();
}

impl NativeWidgets {
    /// Tells the maker of widget `instance` that it shows something new.
    fn picture_changed(&self, host: &Host<'_>, instance: u32) {
        if let Some(item) = self
            .items
            .iter()
            .find(|item| item.instance.get() == instance)
        {
            CommandSender::new(host, &mut [0; 64])
                .widget_changed(item.instance)
                .ok();
        }
    }

    /// A picture widget was made: its picture is chosen.
    pub(crate) fn picture_made(&mut self, host: &Host<'_>, instance: u32) -> Result<(), Error> {
        self.pictures.made(instance)?;
        // A failure is tried again at the next minute.
        self.choose_picture(host, instance).ok();
        Ok(())
    }

    /// A picture widget is gone: its picture is given back.
    pub(crate) fn picture_dropped(&mut self, host: &Host<'_>, instance: u32) {
        if let Some(image) = self.pictures.dropped(instance) {
            release(host, image);
        }
    }

    /// Chooses the picture of `instance` again where what it depends on
    /// changed (the day, a turn, the folder), asks the host for it, and leaves
    /// the one shown until the new is ready.
    ///
    /// # Errors
    /// [`Error::NoMemory`]: the widget is chosen for again at the next minute.
    pub(crate) fn choose_picture(&mut self, host: &Host<'_>, instance: u32) -> Result<(), Error> {
        let Some(widget) = self
            .items
            .iter()
            .find(|item| item.instance.get() == instance)
            .map(|item| paperpaw_ui_kit::text::copy(item.widget()))
            .transpose()?
        else {
            return Ok(());
        };
        let mark = native_picture::mark(host, &widget, &self.picture_showings)?;
        if !self.pictures.wants(instance, mark) {
            return Ok(());
        }
        let position = native_picture::position(host, &widget, &self.picture_showings)?;
        let folder = native_picture::folder_of(&Settings::new(host, &widget))?;
        let chosen = Self::list_pictures(host, &folder, position);
        let name = match chosen {
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // A folder that is missing or not granted has none; a busy card or
            // service is tried again at the next minute (nothing is recorded).
            Err(error) => {
                return match native_picture::refusal(error) {
                    Refusal::None => self.no_picture(host, instance, mark),
                    Refusal::Later => Ok(()),
                };
            }
            Ok(None) => return self.no_picture(host, instance, mark),
            Ok(Some(name)) => name,
        };
        let path = if folder.is_empty() {
            paperpaw_ui_kit::text::copy(&name)?
        } else {
            concat(&[&folder, "/", &name])?
        };
        if path.len() > PATH_MAX {
            return self.no_picture(host, instance, mark);
        }
        let caption = native_picture::caption_of(&name)?;
        let Some(cookie) = self.pictures.asking(instance, mark, caption) else {
            return Ok(());
        };
        let mut request = PrepareImage::new(PrepareRoot::Library, &path, cookie);
        if capture_snapshot(host)?.display_is_colour() {
            request = request.colour();
        }
        let mut scratch = mem::buffer(PATH_MAX + 128)?;
        match CommandSender::new(host, &mut scratch).image_prepare(&request) {
            Ok(()) => Ok(()),
            Err(error) => match native_picture::refusal(error) {
                // The card is not granted, or the request cannot be made.
                Refusal::None => self.no_picture(host, instance, mark),
                // No room for another image now, or busy (or no memory):
                // chosen again at the next minute.
                Refusal::Later => {
                    self.pictures.refused(instance);
                    Ok(())
                }
            },
        }
    }

    fn list_pictures(
        host: &Host<'_>,
        folder: &str,
        position: impl Fn(usize) -> usize,
    ) -> Result<Option<alloc::string::String>, Error> {
        let files = Files::open(host)?;
        let listed = (|| {
            let (mut input, mut out) = native_picture::buffers()?;
            let mut card = OnCard {
                host,
                files: &files,
                path: folder,
                input: &mut input,
                out: &mut out,
            };
            native_picture::picture_at(&mut card, position)
        })();
        let released = files.release(host);
        let name = listed?;
        released?;
        Ok(name)
    }

    fn no_picture(&mut self, host: &Host<'_>, instance: u32, mark: u64) -> Result<(), Error> {
        if let Some(image) = self.pictures.none(instance, mark) {
            release(host, image);
        }
        self.picture_changed(host, instance);
        Ok(())
    }

    /// The host's answer to a picture asked for.
    pub(crate) fn picture_ready(
        &mut self,
        host: &Host<'_>,
        ready: &ImageReady,
    ) -> Result<(), Error> {
        let size = (ready.width(), ready.height());
        match self
            .pictures
            .ready((ready.cookie(), ready.status(), ready.image(), size))
        {
            // The widget is gone, or chose another picture meanwhile: this one
            // is given back.
            Next::Ignore => {
                if let Some(image) = ready.image() {
                    release(host, image);
                }
            }
            Next::Ready {
                instance,
                release: old,
            } => {
                if let Some(old) = old {
                    release(host, old);
                }
                self.picture_changed(host, instance);
            }
            Next::Gone {
                instance,
                release: old,
            } => {
                if let Some(old) = old {
                    release(host, old);
                }
                if let Some(image) = ready.image() {
                    release(host, image);
                }
                self.picture_changed(host, instance);
            }
            Next::Retry => {}
        }
        Ok(())
    }

    /// Each minute: the pictures of the widgets whose day, turn or folder
    /// changed are chosen again, and those refused for want of memory.
    pub(crate) fn tick_pictures(&mut self, host: &Host<'_>) {
        let mut instances = alloc::vec::Vec::new();
        for item in &self.items {
            if item.token == native_picture::TOKEN && instances.try_reserve(1).is_ok() {
                instances.push(item.instance.get());
            }
        }
        for instance in instances {
            self.choose_picture(host, instance).ok();
        }
    }
}
