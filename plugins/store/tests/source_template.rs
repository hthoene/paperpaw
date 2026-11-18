//! The template of a community plugin source (#318, `templates/plugin-source`):
//! what its workflow's `paperpaw-sign pack-source` makes of the template's
//! plugin and its signed builds is read here as a device reads a source: the
//! list with its signature (trust on first use), the plugin's page, the gate
//! of a code plugin, and the download of every file with its digest. A file
//! of the source changed after it was signed or listed is refused.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use std::path::{Path, PathBuf};

use paperpaw_native::header5::Header5;
use paperpaw_native::signature::sign;
use paperpaw_native::{Chip, LINK_BASE, Signature};
use paperpaw_plugin_store::native_device::Facts;
use paperpaw_plugin_store::native_download::{self as dl, Download, FILE_REQUEST, Failure, Step};
use paperpaw_plugin_store::native_load::{
    Details, Done, Ended, LIST_TOKEN, Loader, MAX_PAGE_FILE, PAGE_TOKEN, SIGNATURE_TOKEN, Trouble,
    Wire,
};
use paperpaw_plugin_store::native_model::{FORMAT, Revoked};
use paperpaw_plugin_store::native_sources::{
    DOMAIN, Gate, MAX_SOURCE_LIST, Trust, fingerprint, gate, names_code,
};
use paperpaw_publish::source::{self, About};
use paperpaw_sdk::native::{
    Error, INTERFACE,
    net::{Reason, RequestId},
    signature::public_key,
};
use paperpaw_sdk::{MemoryStorage, Storage};

const SOURCE: &str = "https://ada.example/shelf/plugins.json";
const SEED: [u8; 32] = [3; 32];
const ID: &str = "hello-paw";

/// The template's folder.
fn template() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates/plugin-source")
}

/// A folder of its own for each test.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("paperpaw-source-template")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if source.is_dir() {
            copy(&source, &target);
        } else {
            std::fs::copy(&source, &target).unwrap();
        }
    }
}

/// A plugin file for `chip` of this PaperPaw's plugin interface, signed with
/// the key of `seed` (`tools/publish/tests/check_publish.rs` makes them so).
fn ppn(chip: Chip, seed: &[u8; 32]) -> Vec<u8> {
    let image = [0u8; 128];
    let mut header = Header5 {
        target: chip,
        abi_major: INTERFACE,
        needs_minor: 0,
        base: LINK_BASE,
        image_len: image.len() as u32,
        code_at: 96,
        entry: 0,
        meta_at: 32,
        meta_len: 4,
        relocs: 0,
        align: 16,
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        heap_at: None,
        sha256: [0; 32],
        signed: false,
    };
    let mut sha = header.digest();
    sha.update(&image);
    header.sha256 = sha.finish();
    let mut file = header.to_bytes().to_vec();
    file.extend_from_slice(&image);
    file.extend_from_slice(&sign(&file, seed).to_bytes());
    file
}

/// The version of the template's plugin.
fn version() -> String {
    let ini = std::fs::read_to_string(template().join("plugin/plugin.ini")).unwrap();
    let version = ini.lines().find_map(|l| l.strip_prefix("version = "));
    version.unwrap().trim().to_string()
}

/// What the template's workflow gathers (its step "Gather the plugin and its
/// builds") and signs: the plugin's folder named as its id, the X4's build
/// and the X4 Pro's beside it.
fn gathered(name: &str, seed: &[u8; 32]) -> PathBuf {
    let dir = folder(name);
    copy(&template().join("plugin"), &dir.join(ID));
    let build = |device: &str| dir.join(format!("{ID}-{}-{device}.ppn", version()));
    std::fs::write(build("x4"), ppn(Chip::Esp32C3, seed)).unwrap();
    std::fs::write(build("x4-pro"), ppn(Chip::Esp32S3, seed)).unwrap();
    dir
}

/// The source as the workflow publishes it.
fn published(name: &str) -> PathBuf {
    let out = folder(&format!("{name}-site"));
    let about = About {
        author: "Ada",
        date: "2026-11-18",
    };
    let packed = source::pack(&gathered(name, &SEED), &out, &SEED, about).unwrap();
    assert_eq!(packed.key, public_key(&SEED));
    assert_eq!(packed.plugins.len(), 1);
    assert_eq!(packed.plugins[0].id, ID);
    assert_eq!(packed.plugins[0].devices, ["x4", "x4-pro", "x4-classic"]);
    out
}

/// A device that reads the source at [`SOURCE`] from the folder `site`, as a
/// web server serves it: a path is a file, the query is passed over.
struct Reader {
    site: PathBuf,
    device: &'static str,
    storage: MemoryStorage,
    trust: Trust,
    asked: u32,
    /// The request under way: its token and whether the server had the file.
    last: Option<(u32, bool)>,
}

impl Reader {
    fn new(site: &Path, device: &'static str) -> Self {
        Self {
            site: site.to_path_buf(),
            device,
            storage: MemoryStorage::new(),
            trust: Trust::FirstUse,
            asked: 0,
            last: None,
        }
    }

    /// Fetches `url` to `file`, as the host does.
    fn fetch(&mut self, url: &str, file: &str, token: u32, max: u64) -> RequestId {
        let base = SOURCE.strip_suffix("plugins.json").unwrap();
        let path = url.split(['?', '#']).next().unwrap();
        let body = path
            .strip_prefix(base)
            .and_then(|rest| std::fs::read(self.site.join(rest)).ok())
            .filter(|body| body.len() as u64 <= max);
        if let Some(body) = &body {
            self.storage.write(file, body).unwrap();
        }
        self.asked += 1;
        self.last = Some((token, body.is_some()));
        RequestId::new(self.asked).unwrap()
    }

    /// The end of the request under way, as the fetch of lists and pages
    /// reads it.
    fn done(&mut self) -> Done {
        let (token, found) = self.last.take().unwrap();
        let status = if found { 200 } else { 404 };
        Done::new(
            RequestId::new(self.asked).unwrap(),
            token,
            Reason::Ok,
            status,
        )
    }
}

impl Wire for Reader {
    fn storage(&self) -> &dyn Storage {
        &self.storage
    }
    fn facts(&mut self) -> Result<Option<Facts>, Error> {
        Ok(Some(
            Facts::new(self.device, "code.ppn", "de", false, Vec::new()).unwrap(),
        ))
    }
    fn catalog(&mut self) -> Result<(String, bool), Error> {
        Ok((SOURCE.into(), false))
    }
    fn get(&mut self, url: &str, file: &str, token: u32, max: u64) -> Result<RequestId, Error> {
        Ok(self.fetch(url, file, token, max))
    }
    fn cancel(&mut self, _token: u32) {}
    fn final_url(&mut self, _id: RequestId) -> Option<String> {
        None
    }
    fn small_memory(&self) -> bool {
        true
    }
    fn revoke(&mut self, _revoked: &[Revoked]) -> Result<(), Error> {
        Ok(())
    }
    fn trust(&mut self) -> Result<Trust, Error> {
        Ok(self.trust)
    }
    fn pin(&mut self, key: &[u8; 32]) -> Result<(), Error> {
        if self.trust != Trust::FirstUse {
            return Err(Error::Denied);
        }
        self.trust = Trust::Pinned(*key);
        Ok(())
    }
}

impl dl::Wire for Reader {
    fn storage(&self) -> &dyn Storage {
        &self.storage
    }
    fn get(&mut self, url: &str, file: &str, token: u32, max: u64) -> Result<RequestId, Error> {
        Ok(self.fetch(url, file, token, max))
    }
    fn cancel(&mut self, _token: u32) {}
}

/// Reads the source's list: the list, then its signature.
fn list(reader: &mut Reader, loader: &mut Loader) -> (Ended, Trouble) {
    loader.start_list(reader).unwrap();
    assert_eq!(reader.last, Some((LIST_TOKEN, true)));
    let done = reader.done();
    let ended = loader.ended(reader, &done).unwrap();
    if ended != Ended::Ignored {
        return (ended, loader.trouble());
    }
    assert_eq!(reader.last.map(|(token, _)| token), Some(SIGNATURE_TOKEN));
    let done = reader.done();
    (loader.ended(reader, &done).unwrap(), loader.trouble())
}

/// Reads the page of the template's plugin.
fn page(reader: &mut Reader, loader: &mut Loader) {
    loader.start_page(reader, ID).unwrap();
    assert_eq!(reader.last, Some((PAGE_TOKEN, true)));
    let done = reader.done();
    assert_eq!(loader.ended(reader, &done).unwrap(), Ended::Page);
    assert_eq!(loader.details(), (Some(ID), Details::Here));
}

/// Downloads the files of the plugin's page until all are here and checked,
/// or one is refused.
fn download(reader: &mut Reader, loader: &Loader) -> Step {
    let mut download = Download::new();
    download.start(reader, loader.page(ID).unwrap()).unwrap();
    loop {
        let Some((token, found)) = reader.last.take() else {
            return download.step();
        };
        assert_eq!(token, FILE_REQUEST);
        let done = dl::Done::new(
            RequestId::new(reader.asked).unwrap(),
            token,
            Reason::Ok,
            found,
        );
        let step = download.ended(reader, &done).unwrap();
        if !matches!(step, Step::Downloading { .. }) {
            return step;
        }
    }
}

#[test]
fn the_packer_and_the_store_agree_on_what_a_source_is() {
    assert_eq!(source::DOMAIN, DOMAIN);
    assert_eq!(source::FORMAT, FORMAT);
    assert_eq!(source::MAX_LIST as u64, MAX_SOURCE_LIST);
    assert_eq!(source::MAX_LIST as u64, MAX_PAGE_FILE);
    let key = public_key(&SEED);
    assert_eq!(source::fingerprint(&key), fingerprint(&key).unwrap());
}

#[test]
fn every_device_reads_the_templates_source_and_stages_its_plugin() {
    let site = published("read");
    let key = public_key(&SEED);
    assert_eq!(source::check(&site.join("plugins.json")), Ok(key));
    let folder = template().join("plugin");
    for (device, chip) in [
        ("x4", Chip::Esp32C3),
        ("x4-pro", Chip::Esp32S3),
        ("x4-classic", Chip::Esp32S3),
    ] {
        let mut reader = Reader::new(&site, device);
        let mut loader = Loader::default();
        // The first list pins the source's key.
        assert_eq!(
            list(&mut reader, &mut loader),
            (Ended::ListChanged, Trouble::None),
            "{device}"
        );
        assert_eq!(reader.trust, Trust::Pinned(key));
        let index = loader.index().unwrap();
        assert_eq!(index.len(), 1);
        let entry = index.entry(&reader.storage, 0).unwrap().unwrap();
        assert_eq!((entry.id.as_str(), entry.code), (ID, true), "{device}");
        assert_eq!(entry.version, version());
        assert_eq!(entry.author, "Ada");
        assert!(entry.unreviewed && !entry.official);
        // Its page names the source's key as the author's: the gate opens.
        page(&mut reader, &mut loader);
        let page = loader.page(ID).unwrap();
        assert!(page.code, "{device}");
        assert_eq!(page.author_key, Some(key));
        let code = page.files.last().unwrap();
        assert_eq!(code.path, "code.ppn");
        let others = page.files.iter().filter(|file| file.path != "code.ppn");
        assert!(!names_code(others.map(|file| file.path.as_str())));
        assert_eq!(
            gate(reader.trust, page.code, page.author_key.as_ref(), false),
            Gate::Open
        );
        // Every file comes and has the digest its page says.
        assert_eq!(download(&mut reader, &loader), Step::Ready, "{device}");
        let staged = |path: &str| reader.storage.get(&format!("stage/{ID}/{path}")).unwrap();
        for path in ["plugin.ini", "i18n/en.ini"] {
            assert_eq!(staged(path), std::fs::read(folder.join(path)).unwrap());
        }
        // What is only the store's is not installed.
        assert!(
            reader
                .storage
                .get(&format!("stage/{ID}/store/store.ini"))
                .is_none()
        );
        // The code is the device's chip's build, signed with the source's
        // key, which is what the host installs it by.
        let code = staged("code.ppn");
        assert_eq!(code, ppn(chip, &SEED), "{device}");
        let header = Header5::read(
            &code,
            code.len() as u64,
            &paperpaw_native::header5::Host {
                abi_major: INTERFACE,
                ..if chip == Chip::Esp32C3 {
                    paperpaw_native::header5::Host::x4(u16::MAX)
                } else {
                    paperpaw_native::header5::Host::x4_pro(u16::MAX)
                }
            },
        )
        .unwrap();
        let end = header.file_len() as usize;
        let signature = Signature::read(&code[end..]).unwrap();
        assert_eq!(signature.key, key);
        assert!(signature.holds(&code[..end]).is_ok());
    }
}

/// Flips one byte of the file at `path`.
fn change(path: &Path) {
    let mut bytes = std::fs::read(path).unwrap();
    let at = bytes.len() / 2;
    bytes[at] ^= 1;
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn a_list_changed_after_it_was_signed_is_not_read() {
    let site = published("list");
    change(&site.join("plugins.json"));
    assert!(source::check(&site.join("plugins.json")).is_err());
    let mut reader = Reader::new(&site, "x4");
    let mut loader = Loader::default();
    assert_eq!(
        list(&mut reader, &mut loader),
        (Ended::ListUnchanged, Trouble::Unsigned)
    );
    assert!(loader.index().is_none());
    assert_eq!(reader.trust, Trust::FirstUse);
    // Nor one whose signature file is gone.
    let site = published("unsigned");
    std::fs::remove_file(site.join("plugins.json.sig")).unwrap();
    let mut reader = Reader::new(&site, "x4");
    assert_eq!(
        list(&mut reader, &mut loader),
        (Ended::ListUnchanged, Trouble::Unsigned)
    );
}

#[test]
fn a_file_or_a_build_changed_after_it_was_listed_is_refused() {
    let files = |site: &Path| site.join(format!("plugins/{ID}/{}", version()));
    for changed in ["files/plugin.ini", "x4.ppn"] {
        let site = published(&changed.replace(['/', '.'], "-"));
        let file = match changed {
            "x4.ppn" => files(&site).join(format!("{ID}-{}-x4.ppn", version())),
            path => files(&site).join(path),
        };
        change(&file);
        let mut reader = Reader::new(&site, "x4");
        let mut loader = Loader::default();
        assert_eq!(list(&mut reader, &mut loader).0, Ended::ListChanged);
        page(&mut reader, &mut loader);
        assert_eq!(
            download(&mut reader, &loader),
            Step::Failed(Failure::Damaged),
            "{changed}"
        );
    }
}

#[test]
fn a_build_another_key_signed_is_not_packed() {
    // The list's key is not the builds': a device would refuse the code.
    let input = gathered("other-key", &[4; 32]);
    let out = folder("other-key-site");
    let error = source::pack(&input, &out, &SEED, About::default()).unwrap_err();
    assert!(error.contains("signed with another key"), "{error}");
    assert!(!out.join("plugins.json").exists());
}
