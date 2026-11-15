//! `check-image`: whether a whole flash image (as the web flasher writes
//! it) can take updates over the air once flashed. The release runs it on
//! every device's image and fails where it cannot:
//!
//! - its partition table, read with the firmware's own code
//!   (`paperpaw_x4::partitions`), has the OTA data and two app slots;
//! - the app in the first slot is an image the bootloader starts, with
//!   one data segment (DROM), the application descriptor first in it where
//!   the segment is mapped: the bootloader maps only the last of several
//!   data segments, so what the firmware reads in the others (its
//!   descriptor, its list of plugins) comes from the wrong flash page;
//! - the app fits either slot, and CrossPoint's (PaperPaw runs in
//!   CrossPoint's partitions when installed from its menu; not where the
//!   image installs by flashing only, [`FLASH_ONLY`]), with at least
//!   [`FAIL_BELOW`] to spare for the updates to come; it warns below
//!   [`WARN_BELOW`], and prints the margin either way.
//!
//! `check-app <app.bin>` holds the first rule alone to an app image
//! without table (the install image a build makes).

use paperpaw_update::PLUGINS_AT;
use paperpaw_x4::partitions::{Slots, TABLE_AT, TABLE_LEN};

/// The magic byte of an app image.
const MAGIC: u8 = 0xE9;
/// An image's header, with its extension: then its segments.
const HEADER: usize = 24;
/// Where data (DROM) is mapped, on the ESP32-C3 and the ESP32-S3.
const DROM: core::ops::Range<u32> = 0x3C00_0000..0x3E00_0000;
/// The room of an app slot in CrossPoint's partition table.
const CROSSPOINT_SLOT: u32 = 0x64_0000;
/// The magic word that begins the application descriptor (`esp_app_desc_t`).
const DESC_MAGIC: u32 = 0xABCD_5432;
/// Where the descriptor is in the first data segment's address space: the
/// segment's address (the image header's 24 bytes and the segment's head
/// of 8 bytes are before it in the file).
const DESC_AT: u32 = 0x3C00_0020;
/// Where the project's name is in the descriptor (32 bytes).
const DESC_PROJECT: core::ops::Range<usize> = 48..80;
/// The images that install by flashing only, by the project name in their
/// descriptor (`ota::PROJECT` in the firmware), with why: CrossPoint's menu
/// takes no image over its own slot, so the slot does not bound these.
/// Each line is a decision, not a convenience: add one only with its number.
pub const FLASH_ONLY: &[(&str, &str)] = &[(
    "paperpaw-xteink-pro",
    "Decision 184: the X4 Pro's image is over CrossPoint's slot until it is shrunk; \
     it installs by flashing (Decision 179)",
)];
/// The least room an app leaves in the smallest slot: the next updates
/// grow, and one that no longer fits cannot be installed over the air.
/// It follows the size limit (`shells/xteink/size-budget.txt`): an image at the
/// hard limit of 6,400,000 bytes leaves 153,600 bytes in CrossPoint's slot of
/// 6,553,600, which this passes; 256 KB failed every image near the limit (#275).
pub const FAIL_BELOW: u32 = 128 * 1024;
/// The hard size limit of an image (Decision 184 allows native images above it
/// until they are shrunk; the x4-pro by [`FLASH_ONLY`]).
#[cfg(test)]
const SIZE_LIMIT: u32 = 6_400_000;
/// Below this margin the release warns: time to make room.
pub const WARN_BELOW: u32 = 512 * 1024;

/// What an app image is made of.
#[derive(Debug, PartialEq, Eq)]
pub struct App {
    /// Its length, with its checksum and hash.
    pub len: u32,
    /// Its segments mapped as data.
    pub drom: usize,
    /// Whether the first segment is data at [`DESC_AT`] and begins with the
    /// application descriptor.
    pub descriptor: bool,
    /// The project named in the descriptor, where there is one.
    pub project: String,
    /// The plugin crates the list after the descriptor names (0 where there
    /// is no list).
    pub plugins: usize,
}

/// The app image at the start of `bytes`, as ESP-IDF lays it out: the
/// header, the segments (each its address, length and bytes), padding to
/// 16 bytes with the checksum last, and the SHA-256 where the header says.
pub fn app(bytes: &[u8]) -> Result<App, String> {
    if bytes.len() < HEADER || bytes[0] != MAGIC {
        return Err(String::from("no app image (no magic byte)"));
    }
    let segments = bytes[1];
    let hashed = bytes[23] == 1;
    let mut at = HEADER;
    let mut drom = 0;
    let mut descriptor = false;
    let mut project = String::new();
    let mut plugins = 0;
    for n in 0..segments {
        let head = bytes
            .get(at..at + 8)
            .ok_or_else(|| format!("segment {n} cut short"))?;
        let address = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        if DROM.contains(&address) {
            drom += 1;
        }
        if n == 0 && address == DESC_AT {
            let data = bytes.get(at + 8..(at + 8 + len).min(bytes.len()));
            if let Some(data) = data.filter(|d| d.len() >= DESC_PROJECT.end) {
                descriptor = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) == DESC_MAGIC;
                let name = &data[DESC_PROJECT];
                let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
                project = String::from_utf8_lossy(&name[..end]).into_owned();
                // The list sits right after the descriptor (`PLUGINS_AT` is
                // counted from the image's start: its header and the segment's).
                plugins = data
                    .get(PLUGINS_AT - (HEADER + 8)..)
                    .and_then(paperpaw_update::Plugins::read)
                    .map_or(0, |list| list.iter().count());
            }
        }
        at += 8 + len;
        if at > bytes.len() {
            return Err(format!("segment {n} cut short"));
        }
    }
    let mut len = (at + 1).next_multiple_of(16);
    if hashed {
        len += 32;
    }
    if len > bytes.len() {
        return Err(String::from("the image's checksum or hash cut off"));
    }
    Ok(App {
        len: len as u32,
        drom,
        descriptor,
        project,
        plugins,
    })
}

/// Why the app's data would be read from the wrong flash page, if it
/// would: more than one data segment, or no descriptor at the start of it.
fn one_data_segment(app: &App) -> Result<(), String> {
    if app.drom != 1 {
        return Err(format!(
            "the app has {} data segments: the bootloader maps only the last, so the \
             firmware reads its descriptor and its list of plugins (the section \
             `.flash.appdesc`) from the wrong flash page and cannot tell which slot it \
             runs in by them; a gap before `.rodata` splits the segment (build.rs pads the \
             list so that there is none, and the link fails where one is left)",
            app.drom
        ));
    }
    if !app.descriptor {
        return Err(format!(
            "the app's first segment is not data at {DESC_AT:#x} that begins with the \
             application descriptor (magic {DESC_MAGIC:#x})"
        ));
    }
    if app.plugins == 0 {
        return Err(String::from(
            "the list of plugins built in, right after the application descriptor, is \
             missing or empty: the firmware reads it for the update check and About",
        ));
    }
    Ok(())
}

/// The project's flash-only reason, for the images that install by
/// flashing only.
fn flash_only(app: &App) -> Option<&'static str> {
    FLASH_ONLY
        .iter()
        .find_map(|&(project, why)| (project == app.project).then_some(why))
}

/// The room of the smallest slot the app has to fit: either of its own,
/// and CrossPoint's unless the image installs by flashing only.
fn room(slots: &Slots, app: &App) -> u32 {
    let own = slots.apps[0].1.min(slots.apps[1].1);
    if flash_only(app).is_some() {
        own
    } else {
        own.min(CROSSPOINT_SLOT)
    }
}

/// What keeps the flash image `bytes` from taking updates; its slots and
/// app where nothing does.
pub fn check(bytes: &[u8]) -> Result<(Slots, App), String> {
    let table = bytes
        .get(TABLE_AT as usize..TABLE_AT as usize + TABLE_LEN)
        .ok_or("no partition table: the image ends before it")?;
    let slots = Slots::find(table)
        .map_err(|why| format!("the partition table leaves updates no slot: {why:?}"))?;
    let (first, _) = slots.apps[0];
    let app = app(bytes.get(first as usize..).unwrap_or_default())
        .map_err(|why| format!("the app in the first slot: {why}"))?;
    one_data_segment(&app)?;
    let room = room(&slots, &app);
    if app.len > room {
        return Err(format!(
            "the app ({} bytes) does not fit a slot of {room} bytes (its own or CrossPoint's),              {} KB over",
            app.len,
            (app.len - room).div_ceil(1024)
        ));
    }
    if room - app.len < FAIL_BELOW {
        return Err(format!(
            "the app ({} bytes) leaves {} KB of a slot of {room} bytes (its own or              CrossPoint's); the next updates need at least {} KB",
            app.len,
            (room - app.len) / 1024,
            FAIL_BELOW / 1024
        ));
    }
    Ok((slots, app))
}

/// The room the app leaves in the smallest slot, in bytes.
pub fn margin(slots: &Slots, app: &App) -> u32 {
    room(slots, app) - app.len
}

/// `check-app <app.bin>`: the app image a build makes (no table, no
/// bootloader) has one data segment with the descriptor first.
pub fn check_app_file(path: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let app = app(&bytes).map_err(|why| format!("{path}: {why}"))?;
    one_data_segment(&app).map_err(|why| format!("{path}: {why}"))?;
    println!(
        "{path}: one data segment, the descriptor of {:?} first in it",
        app.project
    );
    Ok(())
}

/// `check-image <flash.bin>`.
pub fn check_file(path: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let (slots, app) = check(&bytes).map_err(|why| format!("{path}: {why}"))?;
    let [(a, a_len), (b, b_len)] = slots.apps;
    println!(
        "{path}: updates go between {a:#x} ({a_len} bytes) and {b:#x} ({b_len} bytes), \
         OTA data at {:#x}; the app takes {} bytes, one data segment",
        slots.data, app.len
    );
    let margin = margin(&slots, &app);
    println!(
        "{path}: margin {} KB in the smallest slot ({} bytes)",
        margin / 1024,
        room(&slots, &app)
    );
    if let Some(why) = flash_only(&app) {
        println!("{path}: CrossPoint's slot is not counted: {why}");
    }
    if margin < WARN_BELOW {
        // An annotation on the release's run where it runs in GitHub Actions.
        let prefix = match std::env::var("GITHUB_ACTIONS").as_deref() {
            Ok("true") => "::warning::",
            _ => "warning: ",
        };
        println!(
            "{prefix}{path}: only {} KB margin left, less than {} KB: make room before the \
             next releases",
            margin / 1024,
            WARN_BELOW / 1024
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        /// Set by a test that wants an image without the plugin list.
        static NO_LIST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// An app image of `segments` (address, length), its checksum padded,
    /// with a hash.
    fn image(segments: &[(u32, usize)]) -> Vec<u8> {
        image_of("paperpaw-xteink", segments)
    }

    /// The same, the application descriptor of `project` at the start of a
    /// first segment at [`DESC_AT`].
    fn image_of(project: &str, segments: &[(u32, usize)]) -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER];
        bytes[0] = MAGIC;
        bytes[1] = segments.len() as u8;
        bytes[23] = 1;
        for &(address, len) in segments {
            bytes.extend_from_slice(&address.to_le_bytes());
            bytes.extend_from_slice(&(len as u32).to_le_bytes());
            let first = bytes.len();
            bytes.extend(std::iter::repeat_n(0x5A, len));
            if address == DESC_AT && len >= DESC_PROJECT.end {
                bytes[first..first + 4].copy_from_slice(&DESC_MAGIC.to_le_bytes());
                bytes[first + DESC_PROJECT.start..first + DESC_PROJECT.end].fill(0);
                let name = project.as_bytes();
                bytes[first + DESC_PROJECT.start..][..name.len()].copy_from_slice(name);
                // The list of plugins after the descriptor (256 bytes).
                let list = paperpaw_update::Plugins::record([paperpaw_update::PluginCrate {
                    name: "paperpaw-plugin-about",
                    version: "1.0.0",
                }])
                .unwrap();
                if len >= 256 + list.len() && !NO_LIST.with(std::cell::Cell::get) {
                    bytes[first + 256..first + 256 + list.len()].copy_from_slice(&list);
                }
            }
        }
        let padded = (bytes.len() + 1).next_multiple_of(16);
        bytes.resize(padded + 32, 0);
        bytes
    }

    /// A flash image: the X4's table at 0x8000, `app` at 0x10000.
    fn flash(app: &[u8], second_slot: bool) -> Vec<u8> {
        let mut bytes = vec![0xFF; 0x10000];
        let mut table = Vec::new();
        let mut entry = |kind: u8, subtype: u8, offset: u32, size: u32| {
            table.extend_from_slice(&[0xAA, 0x50, kind, subtype]);
            table.extend_from_slice(&offset.to_le_bytes());
            table.extend_from_slice(&size.to_le_bytes());
            table.extend_from_slice(&[0; 20]);
        };
        entry(1, 0x00, 0xd000, 0x2000);
        entry(0, 0x10, 0x10000, 0x7F_0000);
        if second_slot {
            entry(0, 0x11, 0x80_0000, 0x7F_0000);
        }
        table.extend_from_slice(&[0xEB, 0xEB]);
        bytes[0x8000..0x8000 + table.len()].copy_from_slice(&table);
        bytes.extend_from_slice(app);
        bytes
    }

    #[test]
    fn an_image_like_the_releases_passes() {
        let app = image(&[(0x3C00_0020, 1000), (0x3FC9_4440, 64), (0x4200_0020, 2000)]);
        let (slots, found) = check(&flash(&app, true)).unwrap();
        assert_eq!(slots.apps[1].0, 0x80_0000);
        assert_eq!(found.len as usize, app.len());
        assert_eq!(found.drom, 1);
    }

    #[test]
    fn an_image_with_two_data_segments_fails() {
        // As the builds with the feature remote were, on the ESP32-S3.
        let app = image(&[(0x3C00_0020, 0x464), (0x3FC9_4440, 64), (0x3C00_0488, 2000)]);
        let why = check(&flash(&app, true)).unwrap_err();
        assert!(why.contains("2 data segments"), "{why}");
    }

    #[test]
    fn the_s3_image_with_the_gap_before_rodata_fails() {
        // As x4-pro and x4-classic were built from Beta 4's trains on: the
        // descriptor's section (1,164 bytes with the plugin list) ends 4
        // bytes short of the 8 that `.rodata` is aligned to, and `espflash`
        // splits the data there; the two RAM segments in between are the
        // padding it puts so the last data segment sits on its page offset.
        let app = image(&[
            (0x3C00_0020, 1164),
            (0x3FC9_4440, 35_120),
            (0x4037_8000, 30_396),
            (0x3C00_04B0, 4000),
            (0x4218_0020, 5000),
        ]);
        let why = check(&flash(&app, true)).unwrap_err();
        assert!(why.contains("2 data segments"), "{why}");
        assert!(why.contains("wrong flash page"), "{why}");
        // The same image with the section padded to the alignment: one.
        let app = image(&[
            (0x3C00_0020, 1168),
            (0x3FC9_4440, 35_120),
            (0x4037_8000, 30_396),
            (0x4218_0020, 5000),
        ]);
        assert_eq!(check(&flash(&app, true)).unwrap().1.drom, 1);
    }

    #[test]
    fn an_image_without_the_plugin_list_fails() {
        NO_LIST.with(|n| n.set(true));
        let app = image(&[(0x3C00_0020, 1168), (0x4200_0020, 2000)]);
        NO_LIST.with(|n| n.set(false));
        let why = check(&flash(&app, true)).unwrap_err();
        assert!(why.contains("list of plugins"), "{why}");
    }

    #[test]
    fn a_data_segment_without_the_descriptor_first_fails() {
        // One segment, but not at the address the descriptor is read from.
        let app = image(&[(0x3C00_0488, 2000), (0x4200_0020, 2000)]);
        let why = check(&flash(&app, true)).unwrap_err();
        assert!(why.contains("application descriptor"), "{why}");
        // At it, but without the magic.
        let mut app = image(&[(0x3C00_0020, 2000), (0x4200_0020, 2000)]);
        app[HEADER + 8] ^= 0xFF;
        let why = check(&flash(&app, true)).unwrap_err();
        assert!(why.contains("application descriptor"), "{why}");
    }

    #[test]
    fn the_x4_pro_is_held_to_its_own_slot_by_decision_184() {
        // 100 KB over CrossPoint's slot, well inside its own (0x7F0000).
        let big = CROSSPOINT_SLOT as usize + 100 * 1024;
        let pro = image_of("paperpaw-xteink-pro", &[(0x3C00_0020, big)]);
        let (slots, app) = check(&flash(&pro, true)).unwrap();
        assert!(flash_only(&app).is_some());
        assert_eq!(margin(&slots, &app), slots.apps[0].1 - app.len);
        // Every other project is not: the same size fails.
        for project in [
            "paperpaw-xteink",
            "paperpaw-xteink-classic",
            "paperpaw-xteink-x3",
        ] {
            let other = image_of(project, &[(0x3C00_0020, big)]);
            let why = check(&flash(&other, true)).unwrap_err();
            assert!(why.contains("does not fit"), "{project}: {why}");
        }
        // The x4-pro still has to fit its own slot.
        let huge = image_of("paperpaw-xteink-pro", &[(0x3C00_0020, 0x7F_0000)]);
        let why = check(&flash(&huge, true)).unwrap_err();
        assert!(why.contains("does not fit"), "{why}");
    }

    #[test]
    fn a_table_without_a_second_slot_fails() {
        let app = image(&[(0x3C00_0020, 1000)]);
        let why = check(&flash(&app, false)).unwrap_err();
        assert!(why.contains("NoSecondSlot"), "{why}");
    }

    #[test]
    fn an_app_too_large_for_crosspoints_slot_fails() {
        let app = image(&[(0x3C00_0020, CROSSPOINT_SLOT as usize)]);
        let why = check(&flash(&app, true)).unwrap_err();
        assert!(why.contains("does not fit"), "{why}");
    }

    #[test]
    fn an_app_that_leaves_too_little_margin_fails() {
        // The header, segment head, checksum and hash: 80 bytes besides.
        let fits = (CROSSPOINT_SLOT - FAIL_BELOW) as usize - 80;
        let (slots, app) = check(&flash(&image(&[(0x3C00_0020, fits)]), true)).unwrap();
        assert_eq!(margin(&slots, &app), FAIL_BELOW);
        let why = check(&flash(&image(&[(0x3C00_0020, fits + 16)]), true)).unwrap_err();
        assert!(why.contains("need at least 128 KB"), "{why}");
    }

    #[test]
    fn an_app_at_the_size_limit_passes_in_crosspoints_slot() {
        // 80 bytes of header, segment head, checksum and hash, 16 of padding.
        let at_limit = image(&[(0x3C00_0020, SIZE_LIMIT as usize - 96)]);
        let (slots, app) = check(&flash(&at_limit, true)).unwrap();
        assert!(app.len <= SIZE_LIMIT && SIZE_LIMIT - app.len < 16 + 32);
        assert_eq!(room(&slots, &app), CROSSPOINT_SLOT);
        assert!(margin(&slots, &app) >= CROSSPOINT_SLOT - SIZE_LIMIT);
        assert!(margin(&slots, &app) >= FAIL_BELOW);
        // Past the limit by what the rule leaves, it fails.
        let past = image(&[(0x3C00_0020, (CROSSPOINT_SLOT - FAIL_BELOW) as usize - 64)]);
        assert!(check(&flash(&past, true)).is_err());
    }
}
