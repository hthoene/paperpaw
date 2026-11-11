//! Hosted acceptance kit: run a linked native plugin on a real kernel, open one of its screens,
//! press keys, and compare the frame with a golden (`NATIVE-UI.md` lists what the production
//! loader adds). In process: no `.so`, no card. Build on `hosted.rs`; the including test crate
//! needs `png` as a dev-dependency.
//!
//! ```ignore
//! #[path = "../../test-support/hosted.rs"]
//! mod hosted;
//! #[path = "../../test-support/acceptance.rs"]
//! mod acceptance;
//!
//! let mut run = acceptance::Session::start(
//!     hosted::Setup::x4().config("language = de"),
//!     paperpaw_plugin_x::NATIVE_ENTRY.entry(),
//!     "x",
//!     &["core.ui"],
//! )?;
//! run.open("x.screen")?.keys(&["page-forward", "back"])?;
//! run.frame().matches_golden(&golden_path)?;
//! ```
//!
//! A step the host cannot do yet (the screen does not open, a key is not taken) is an `Err`
//! naming it, never a frame that happens to match.
use super::hosted::{self, Installed, Setup};
use paperpaw_sdk::{ButtonState, Id, InputEvent, Instant, Params};
use std::path::{Path, PathBuf};

/// The screen as grays, one byte a pixel, rows top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub gray: Vec<u8>,
}

#[allow(dead_code, reason = "each including test crate uses part of the kit")]
impl Frame {
    /// Compare with the PNG `golden` exactly (a color golden by its luma, as the desktop
    /// snapshots read them; a golden with an alpha channel is refused). A difference writes
    /// `<name>.actual.png` and `<name>.diff.png` into `$PAPERPAW_ACCEPTANCE_EVIDENCE`
    /// (default: the system temp folder) and names them.
    pub fn matches_golden(&self, golden: &Path) -> Result<(), String> {
        let folder = std::env::var_os("PAPERPAW_ACCEPTANCE_EVIDENCE")
            .map_or_else(std::env::temp_dir, PathBuf::from);
        self.matches_golden_in(golden, &folder)
    }

    /// [`Self::matches_golden`] with the folder for the evidence given.
    pub fn matches_golden_in(&self, golden: &Path, folder: &Path) -> Result<(), String> {
        let (width, height, expected) = read_png(golden)?;
        if (width, height) != (self.width, self.height) {
            return Err(format!(
                "{}: the golden is {width}x{height}, the frame {}x{}",
                golden.display(),
                self.width,
                self.height
            ));
        }
        let diff: Vec<u8> = self
            .gray
            .iter()
            .zip(&expected)
            .map(|(a, b)| if a == b { 255 } else { 0 })
            .collect();
        let differing = diff.iter().filter(|p| **p == 0).count();
        if differing == 0 {
            return Ok(());
        }
        let stem = golden
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("frame");
        let actual = folder.join(format!("{stem}.actual.png"));
        let marked = folder.join(format!("{stem}.diff.png"));
        let _ = std::fs::create_dir_all(folder);
        write_png(&actual, self.width, self.height, &self.gray);
        write_png(&marked, self.width, self.height, &diff);
        Err(format!(
            "{}: {differing} pixels differ (frame: {}, diff: {})",
            golden.display(),
            actual.display(),
            marked.display()
        ))
    }
}

fn read_png(path: &Path) -> Result<(usize, usize, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    // Palettes and grays below 8 bits widen to 8-bit color or gray; 16 bits narrow to 8.
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut data = vec![0; reader.output_buffer_size().ok_or("a golden too large")?];
    let info = reader
        .next_frame(&mut data)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let data = &data[..info.buffer_size()];
    if info.bit_depth != png::BitDepth::Eight {
        return Err(format!(
            "{}: {:?} after widening",
            path.display(),
            info.bit_depth
        ));
    }
    let gray = match info.color_type {
        png::ColorType::Grayscale => data.to_vec(),
        // As `Pixel::luma` rounds.
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| {
                ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114 + 500)
                    / 1000) as u8
            })
            .collect(),
        other => return Err(format!("{}: unexpected {other:?}", path.display())),
    };
    Ok((info.width as usize, info.height as usize, gray))
}

fn write_png(path: &Path, width: usize, height: usize, gray: &[u8]) {
    let file = std::fs::File::create(path).expect("evidence file");
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        u32::try_from(width).unwrap(),
        u32::try_from(height).unwrap(),
    );
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(gray))
        .expect("evidence png");
}

/// A plugin installed in a kernel that is started, with the screen as it was drawn so far.
pub struct Session {
    installed: Installed,
    now: u64,
    frame: Frame,
}

#[allow(dead_code, reason = "each including test crate uses part of the kit")]
impl Session {
    /// Install `entry` as plugin `name` (see [`hosted::install_in`]), start the kernel and draw.
    pub fn start(
        setup: Setup,
        entry: &'static paperpaw_native::host_glue::PpEntry,
        name: &str,
        services: &[&'static str],
    ) -> Result<Self, String> {
        let mut installed = hosted::install_in(&setup, entry, name, services)?;
        let display = installed.kernel().device().display;
        let (width, height) = (usize::from(display.width), usize::from(display.height));
        let mut session = Self {
            installed,
            now: 100,
            frame: Frame {
                width,
                height,
                gray: vec![255; width * height],
            },
        };
        session.installed.kernel().start(Instant(session.now));
        session.draw();
        Ok(session)
    }

    /// Let the kernel go on with what the start began: a few seconds of ticks, each with the
    /// work the kernel withheld, so that a plugin's callbacks (`Started` and what it staged
    /// there: service calls, commands) have run against the real host before the test looks.
    /// A call the host refused (a reply buffer smaller than the op's `max_reply` is
    /// `ERR_NO_SPACE`, as on a device) lands in [`Self::dropped`] or [`Self::warnings`].
    pub fn settle(&mut self) -> &mut Self {
        for _ in 0..4 {
            self.now += 1_000;
            self.installed.kernel().proceed(Instant(self.now));
            self.installed.kernel().tick(Instant(self.now));
        }
        self.draw();
        self
    }

    /// Open the screen `id` as a command would, and draw. `Err` where it is not the screen on
    /// top afterwards (the host refused it, or the plugin did not declare it).
    pub fn open(&mut self, id: &str) -> Result<&mut Self, String> {
        let screen = Id::try_parse(id).map_err(|e| format!("{id}: {e:?}"))?;
        self.now += 100;
        self.installed
            .kernel()
            .open_screen(Instant(self.now), screen, Params::default());
        self.draw();
        match self.installed.kernel().active_screen() {
            Some(top) if top.as_str() == id => Ok(self),
            top => Err(format!("{id} did not open; on top: {top:?}")),
        }
    }

    /// Press and release each button in turn, and draw. `Err` naming a button the device does
    /// not have (nothing is pressed then, not even the buttons before it).
    pub fn keys(&mut self, keys: &[&'static str]) -> Result<&mut Self, String> {
        let known = self.installed.kernel().device().input.keys;
        if let Some(name) = keys
            .iter()
            .find(|name| !known.is_empty() && !known.contains(name))
        {
            return Err(format!("{name}: the device has no such button ({known:?})"));
        }
        for &name in keys {
            for state in [ButtonState::Pressed, ButtonState::Released] {
                self.now += 100;
                self.installed
                    .kernel()
                    .handle(Instant(self.now), InputEvent::Button { name, state });
                self.draw();
            }
        }
        Ok(self)
    }

    /// The id of the screen on top of the kernel's stack, if any.
    pub fn top(&mut self) -> Option<String> {
        self.installed
            .kernel()
            .active_screen()
            .map(|id| id.as_str().to_string())
    }

    /// The screen as drawn so far.
    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    /// What the kernel warned about so far, as text: a refusal of the plugin lands here.
    pub fn warnings(&mut self) -> Vec<String> {
        self.installed
            .kernel()
            .take_warnings()
            .iter()
            .map(|w| format!("{w:?}"))
            .collect()
    }

    /// The callbacks the host dropped so far as (token, status), e.g. a draw whose service
    /// answered `State`: the reason a frame stays blank.
    pub fn dropped(&self) -> Vec<(u32, i32)> {
        self.installed.dropped()
    }

    fn draw(&mut self) {
        let frame = &mut self.frame;
        self.installed.kernel().render(&mut |top, view| {
            for row in 0..view.height {
                for x in 0..view.width {
                    let at = (usize::from(top) + usize::from(row)) * frame.width + usize::from(x);
                    if let Some(pixel) = frame.gray.get_mut(at) {
                        *pixel = view.pixel(x, row).luma();
                    }
                }
            }
        });
    }
}
