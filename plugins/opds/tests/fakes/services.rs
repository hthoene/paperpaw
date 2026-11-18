//! Scripted `core.net` and `core.library` services and the data folder in
//! memory, for the tests that drive an engine through the scripted host: the
//! requests as they go out, the imports as they are asked, the answers' files.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    dead_code,
    reason = "test fixtures"
)]
use super::Mem;
use paperpaw_core::native_host::registrations::{
    EventHttpDone, EventHttpProgress, Record, ServiceLibResult, ServiceNetAnswer, ServiceNetBytes,
    ServiceNetStarted, record_encode_one, records_decode,
};
use paperpaw_native::host_glue::ERR_STATE;
use paperpaw_plugin_opds::native_files::Files;
use paperpaw_scripted_host::{ScriptedHost, event_batch};
use paperpaw_sdk::native::Error;
use std::cell::RefCell;

thread_local! {
    pub static SEEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    pub static DISK: Mem = Mem::default();
    /// What the network was asked, what the library was asked, in order.
    pub static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The address an answer came from after redirects.
    pub static FINAL: RefCell<String> = const { RefCell::new(String::new()) };
    /// Names the library holds already.
    pub static TAKEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The library refuses an import with this status.
    pub static IMPORT_FAILS: RefCell<Option<i32>> = const { RefCell::new(None) };
}

pub fn say(line: String) {
    SEEN.with(|seen| seen.borrow_mut().push(line));
}

pub fn seen() -> Vec<String> {
    SEEN.with(|seen| seen.take())
}

pub fn log() -> Vec<String> {
    LOG.with(|log| log.take())
}

pub fn put(name: &str, bytes: &[u8]) {
    DISK.with(|d| d.put(name, bytes));
}

pub fn file(name: &str) -> Option<String> {
    DISK.with(|d| d.text(name))
}

/// The data folder in memory, as the engine sees it.
pub struct Disk;

impl Files for Disk {
    fn size(&self, name: &str) -> Result<u64, Error> {
        DISK.with(|d| d.size(name))
    }
    fn read_at(&self, name: &str, offset: u64, out: &mut [u8]) -> Result<usize, Error> {
        DISK.with(|d| d.read_at(name, offset, out))
    }
    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        DISK.with(|d| d.write(name, data))
    }
    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        DISK.with(|d| d.append(name, data))
    }
    fn remove(&self, name: &str) -> Result<(), Error> {
        DISK.with(|d| d.remove(name))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        DISK.with(|d| d.rename(from, to))
    }
}

pub fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// A scripted `core.net`: requests get the ids 1, 2, ...; an answer's final
/// address is [`FINAL`].
pub fn net(host: &ScriptedHost) {
    let next = RefCell::new(0_u32);
    host.service("core.net", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (1, Some(Record::ServiceNetRequest(request))) => {
                let mut line = format!("{} {} #{}", request.method, request.url, request.token);
                for row in &rows[1..] {
                    match row {
                        Record::ServiceNetHeader(h) => {
                            let value = if h.name == "Authorization" {
                                "<hidden>"
                            } else {
                                h.value
                            };
                            line += &format!(" [{}: {value}]", h.name);
                            if h.name == "Authorization" {
                                line += &format!("<{}>", h.value);
                            }
                        }
                        Record::ServiceNetBody(b) => {
                            line += &format!(" body {}", String::from_utf8_lossy(b.data));
                        }
                        Record::ServiceNetFile(f) => line += &format!(" file {}", f.path),
                        _ => return Err(-1),
                    }
                }
                line += &format!(" max_file {}", request.max_file);
                if request.flags & 1 != 0 {
                    line += " progress";
                }
                LOG.with(|log| log.borrow_mut().push(line));
                *next.borrow_mut() += 1;
                Ok(stream(&[Record::ServiceNetStarted(ServiceNetStarted {
                    id: *next.borrow(),
                })]))
            }
            (2, Some(Record::ServiceNetToken(token))) => {
                LOG.with(|log| log.borrow_mut().push(format!("cancel #{}", token.token)));
                Ok(Vec::new())
            }
            (3, Some(Record::ServiceNetId(_))) => {
                let url = FINAL.with(|f| f.borrow().clone());
                Ok(stream(&[Record::ServiceNetAnswer(ServiceNetAnswer {
                    status: 200,
                    flags: 0,
                    body_len: 0,
                    etag: "",
                    content_type: "",
                    final_url: &url,
                })]))
            }
            (4, Some(Record::ServiceNetRead(_))) => {
                Ok(stream(&[Record::ServiceNetBytes(ServiceNetBytes {
                    data: b"",
                })]))
            }
            _ => Err(-1),
        }
    });
}

/// A scripted `core.library` with `import`: a taken name is `ERR_STATE` unless
/// a free one is asked for; the file moves out of the data folder.
pub fn library(host: &ScriptedHost) {
    host.service("core.library", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (12, Some(Record::ServiceLibImport(i))) => {
                LOG.with(|log| {
                    log.borrow_mut().push(format!(
                        "import {} {}/{} flags {}",
                        i.from, i.folder, i.name, i.flags
                    ));
                });
                if let Some(status) = IMPORT_FAILS.with(|f| *f.borrow()) {
                    return Err(status);
                }
                let mut name = i.name.to_string();
                let taken = TAKEN.with(|t| t.borrow().contains(&name));
                if taken && i.flags & 3 == 0 {
                    return Err(ERR_STATE);
                }
                if taken {
                    name = name.replace(".epub", " (2).epub");
                }
                DISK.with(|d| d.files.borrow_mut().remove(i.from));
                let path = format!("Books/{}/{name}", i.folder);
                TAKEN.with(|t| t.borrow_mut().push(name));
                Ok(stream(&[Record::ServiceLibResult(ServiceLibResult {
                    path: &path,
                    status: 0,
                    flags: 0,
                })]))
            }
            _ => Err(-1),
        }
    });
}

pub fn done(id: u32, token: u32, status: u32, reason: u32) -> Vec<u8> {
    event_batch(&Record::EventHttpDone(EventHttpDone {
        now_ms: 0,
        id,
        token,
        status,
        reason,
        flags: 0,
        body_len: 0,
    }))
    .unwrap()
}

pub fn progress(id: u32, token: u32, now_ms: u64, received: u64, total: Option<u64>) -> Vec<u8> {
    event_batch(&Record::EventHttpProgress(EventHttpProgress {
        now_ms,
        id,
        token,
        received,
        total: total.unwrap_or(0),
        flags: u32::from(total.is_some()),
    }))
    .unwrap()
}

pub fn nothing() -> Vec<u8> {
    done(0, 0, 0, 0)
}
