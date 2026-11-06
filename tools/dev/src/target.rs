//! What is checked: plugins of the app by id (PaperPaw's own, or one a
//! plugin's repository added, see [`crate::main`]), data plugin folders
//! and `.ppn` files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use paperpaw_sdk::install::is_plugin_name;

use crate::AddPlugins;

/// A data plugin, as the device would find it in `plugins`: its files,
/// read from a folder or handed in (by the browser's emulator).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPlugin {
    /// Its id, from `plugin.ini` (else the folder's name).
    pub id: String,
    /// The folder it was read from; `None` for one handed in.
    pub folder: Option<PathBuf>,
    /// Its files by their path in it (with `/`), hidden ones too, shared
    /// by everything that looks at them.
    pub files: BTreeMap<String, Rc<[u8]>>,
}

impl DataPlugin {
    /// The plugin in `folder`, every file read but those of hidden
    /// folders (`.git`); hidden files are read (the installer and the
    /// plugin library see them).
    pub fn read(folder: &Path) -> Result<DataPlugin, String> {
        let mut files = BTreeMap::new();
        read_dir(folder, folder, &mut files)?;
        let name = folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(DataPlugin {
            id: data_plugin_id(&files).unwrap_or(name),
            folder: Some(folder.to_path_buf()),
            files,
        })
    }

    /// A plugin of `files` (path in it, bytes), its id `plugin.ini`'s or
    /// else `id`.
    pub fn new<B: Into<Rc<[u8]>>>(
        id: &str,
        files: impl IntoIterator<Item = (String, B)>,
    ) -> DataPlugin {
        let files = files.into_iter().map(|(p, b)| (p, b.into())).collect();
        DataPlugin {
            id: data_plugin_id(&files).unwrap_or_else(|| id.to_string()),
            folder: None,
            files,
        }
    }

    /// The file at `path` in it.
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(|bytes| &**bytes)
    }

    /// A digest of its files, paths and contents (FNV-1a): whether what
    /// is handed in changed.
    pub fn digest(&self) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for (path, bytes) in &self.files {
            for &b in path
                .as_bytes()
                .iter()
                .chain([0u8].iter())
                .chain(bytes.iter())
            {
                hash = (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        hash
    }

    /// Where it is, for messages: its folder, else its id.
    pub fn place(&self) -> String {
        match &self.folder {
            Some(folder) => folder.display().to_string(),
            None => self.id.clone(),
        }
    }
}

/// Every file under `dir` into `out`, by its path from `root`, but those
/// of hidden folders.
fn read_dir(root: &Path, dir: &Path, out: &mut BTreeMap<String, Rc<[u8]>>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let full = entry.path();
        let meta = entry
            .metadata()
            .map_err(|e| format!("{}: {e}", full.display()))?;
        let hidden = entry.file_name().to_string_lossy().starts_with('.');
        if meta.is_dir() && !hidden {
            read_dir(root, &full, out)?;
        } else if meta.is_file() {
            let relative = full.strip_prefix(root).unwrap_or(&full);
            let bytes = std::fs::read(&full).map_err(|e| format!("{}: {e}", full.display()))?;
            out.insert(relative.to_string_lossy().replace('\\', "/"), bytes.into());
        }
    }
    Ok(())
}

/// A plugin with code as source: the crate's folder, looked at by
/// `package` for the conventions of `docs/plugin-conventions.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The plugin's id: `plugin/plugin.ini`'s, else the crate's name
    /// without `paperpaw-plugin-`.
    pub id: String,
    pub dir: PathBuf,
    /// Why the plugin may embed large files: `embedded-assets` under
    /// `[package.metadata.paperpaw]` in its `Cargo.toml` (a plugin built
    /// into the firmware has no installer to bring files to the card).
    pub embedded_assets: Option<String>,
}

/// The reason `embedded-assets = "…"` gives under
/// `[package.metadata.paperpaw]` in a `Cargo.toml`'s `text`.
pub(crate) fn embedded_assets(text: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[package.metadata.paperpaw]";
        } else if inside
            && let Some(value) = line.strip_prefix("embedded-assets")
            && let Some(value) = value.trim_start().strip_prefix('=')
        {
            let reason = value.trim().trim_matches('"').trim();
            return (!reason.is_empty()).then(|| reason.to_string());
        }
    }
    None
}

impl Source {
    /// The crate in `dir` (it has a `Cargo.toml`).
    pub fn read(dir: &Path) -> Result<Source, String> {
        let from_manifest = std::fs::read(dir.join("plugin/plugin.ini"))
            .ok()
            .and_then(|bytes| {
                let files = BTreeMap::from([("plugin.ini".to_string(), Rc::from(bytes))]);
                data_plugin_id(&files)
            });
        let id = from_manifest.or_else(|| {
            let text = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
            let name = text
                .lines()
                .find_map(|l| l.trim().strip_prefix("name")?.trim().strip_prefix('='))?
                .trim()
                .trim_matches('"')
                .to_string();
            let id = name.strip_prefix("paperpaw-plugin-").unwrap_or(&name);
            is_plugin_name(id).then(|| id.to_string())
        });
        let embedded_assets = std::fs::read_to_string(dir.join("Cargo.toml"))
            .ok()
            .and_then(|text| embedded_assets(&text));
        id.map(|id| Source {
            id,
            dir: dir.to_path_buf(),
            embedded_assets,
        })
        .ok_or_else(|| {
            format!(
                "{}: no plugin id (`plugin/plugin.ini` or a crate named `paperpaw-plugin-<id>`)",
                dir.display()
            )
        })
    }
}

/// What a run looks at.
#[derive(Debug, Clone, Default)]
pub struct Target {
    /// Plugins with code as source (a crate's folder), for `package`.
    pub sources: Vec<Source>,
    /// The plugins whose screens, widgets, settings and texts are looked
    /// at, by id. Empty: every plugin of the app (PaperPaw's own
    /// development).
    pub plugins: Vec<String>,
    /// Data plugins put into the emulated device's `plugins` folder.
    pub data: Vec<DataPlugin>,
    /// Code plugins built for a device, looked at as files.
    pub ppn: Vec<PathBuf>,
    /// Code plugins as libraries (`.so`, interface 7), looked at as files.
    pub libraries: Vec<PathBuf>,
    /// Adds a plugin with code to the app.
    pub add: Option<AddPlugins>,
}

impl Target {
    /// Takes a path from the command line: a data plugin folder (it has
    /// a `plugin.ini`), a `.ppn` file or a `.so` library.
    pub fn take_path(&mut self, path: &Path) -> Result<(), String> {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ppn"))
        {
            if !path.is_file() {
                return Err(format!("{}: no such file", path.display()));
            }
            self.ppn.push(path.to_path_buf());
            return Ok(());
        }
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("so"))
        {
            if !path.is_file() {
                return Err(format!("{}: no such file", path.display()));
            }
            self.libraries.push(path.to_path_buf());
            return Ok(());
        }
        if path.join("plugin.ini").is_file() {
            self.take_data(DataPlugin::read(path)?);
            return Ok(());
        }
        if path.join("Cargo.toml").is_file() {
            // A crate: `package` reads its source for the conventions; a
            // plugin with code is run from its own repository, with
            // `paperpaw_dev::main` in an example (docs/plugins.md,
            // "Checking your plugin").
            self.sources.push(Source::read(path)?);
            return Ok(());
        }
        Err(format!(
            "{}: neither a data plugin folder (with plugin.ini), a .ppn file nor a .so library",
            path.display()
        ))
    }

    /// Looks at a data plugin.
    pub fn take_data(&mut self, plugin: DataPlugin) {
        if !self.plugins.contains(&plugin.id) {
            self.plugins.push(plugin.id.clone());
        }
        self.data.push(plugin);
    }

    /// Whether `id` (a screen, widget, setting, translation key or
    /// action) belongs to a plugin looked at: its namespace is one of
    /// them, or none was named.
    pub fn ours(&self, id: &str) -> bool {
        self.plugins.is_empty() || self.owner(id).is_some()
    }

    /// The plugin looked at whose namespace `id` is in.
    pub fn owner(&self, id: &str) -> Option<&str> {
        let namespace = id.split(['.', '@', ' ', '/']).next().unwrap_or(id);
        self.plugins
            .iter()
            .find(|p| p.as_str() == namespace)
            .map(String::as_str)
    }

    /// Whether the app is looked at in the emulator at all (not only
    /// `.ppn` files).
    pub fn runs(&self) -> bool {
        !self.plugins.is_empty()
            || !self.data.is_empty()
            || (self.ppn.is_empty() && self.libraries.is_empty() && self.sources.is_empty())
    }
}

/// The id a data plugin's `plugin.ini` gives, if any and a plugin's name
/// (it names the plugin's folder on the device).
pub fn data_plugin_id(files: &BTreeMap<String, Rc<[u8]>>) -> Option<String> {
    let text = std::str::from_utf8(files.get("plugin.ini")?).ok()?;
    paperpaw_config::parse(text)
        .filter_map(Result::ok)
        .find(|entry| entry.section == "plugin" && entry.key == "id")
        .map(|entry| entry.value.as_str().into_owned())
        .filter(|id| is_plugin_name(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_belong_by_namespace() {
        let target = Target {
            plugins: vec!["hello".into()],
            ..Target::default()
        };
        assert!(target.ours("hello.screen"));
        assert!(target.ours("hello.widget@2 0/0 2x1"));
        assert!(!target.ours("helloworld.screen"));
        assert!(!target.ours("core.home"));
        assert!(Target::default().ours("core.home"));
    }

    #[test]
    fn a_sample_data_plugin_is_taken() {
        let folder =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/plugins/large-print");
        let mut target = Target::default();
        target.take_path(&folder).unwrap();
        assert_eq!(target.data.len(), 1);
        assert_eq!(target.plugins, [target.data[0].id.clone()]);
        assert!(target.data[0].file("plugin.ini").is_some());
    }

    #[test]
    fn hidden_folders_are_not_read_hidden_files_are() {
        let root = std::env::temp_dir().join(format!("paperpaw-dev-hidden-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, text) in [
            (
                "plugin.ini",
                "[plugin]
id = dusk
",
            ),
            (
                ".git/config",
                "[core]
",
            ),
            (".DS_Store", "x"),
            (
                "i18n/en.ini",
                "a = b
",
            ),
        ] {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        let plugin = DataPlugin::read(&root).unwrap();
        let paths: Vec<&str> = plugin.files.keys().map(String::as_str).collect();
        assert_eq!(paths, [".DS_Store", "i18n/en.ini", "plugin.ini"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_data_plugin_handed_in_is_named_by_its_manifest() {
        let files = [(
            "plugin.ini".to_string(),
            b"[plugin]\nid = dusk\nname = Dusk\n".to_vec(),
        )];
        let plugin = DataPlugin::new("given", files);
        assert_eq!(plugin.id, "dusk");
        assert_eq!(plugin.place(), "dusk");
        // Changed, it is told apart.
        let changed = DataPlugin::new(
            "given",
            [(
                "plugin.ini".to_string(),
                b"[plugin]\nid = dusk\nname = Dusky\n".to_vec(),
            )],
        );
        assert_ne!(plugin.digest(), changed.digest());
        assert_eq!(plugin.digest(), plugin.clone().digest());
        let none: [(String, Vec<u8>); 0] = [];
        assert_eq!(DataPlugin::new("given", none).id, "given");
        // An id that would leave the plugins folder is not taken.
        let files = [(
            "plugin.ini".to_string(),
            b"[plugin]\nid = ../../x\n".to_vec(),
        )];
        assert_eq!(DataPlugin::new("given", files).id, "given");
    }
}
