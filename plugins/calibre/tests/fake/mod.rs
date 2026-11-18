//! A device for the session to talk to: the records Calibre left, the secret,
//! the extensions, the space.
#![allow(
    dead_code,
    reason = "shared by the tests; not every test uses every part"
)]
use paperpaw_plugin_calibre::native_keep::Secret;
use paperpaw_plugin_calibre::native_session::Backing;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::text::copy;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Fake {
    /// The books Calibre sent: lpath → metadata.
    pub sent: BTreeMap<String, String>,
    pub info: Option<String>,
    pub trusted: Option<String>,
    pub password: Option<String>,
    pub free: Option<u64>,
    pub total: Option<u64>,
}

impl Fake {
    pub fn with_book(mut self, lpath: &str, metadata: &str) -> Self {
        self.sent.insert(lpath.to_string(), metadata.to_string());
        self
    }
}

const EXTENSIONS: [&str; 3] = ["epub", "fb2.zip", "txt"];

impl Backing for Fake {
    fn sent(&self, lpath: &str) -> Result<bool, Error> {
        Ok(self.sent.contains_key(lpath))
    }
    fn remember(&mut self, lpath: &str, metadata: &str) -> Result<(), Error> {
        self.sent.insert(lpath.to_string(), metadata.to_string());
        Ok(())
    }
    fn forget(&mut self, lpath: &str) -> Result<Option<String>, Error> {
        Ok(self.sent.remove(lpath).and_then(|metadata| {
            paperpaw_plugin_calibre::native_model::text(&metadata, "uuid")
                .ok()
                .flatten()
        }))
    }
    fn books(&self) -> Result<Vec<u128>, Error> {
        Ok((0..self.sent.len() as u128).collect())
    }
    fn metadata(&self, id: u128) -> Result<Option<String>, Error> {
        Ok(self.sent.values().nth(id as usize).cloned())
    }
    fn device_info(&self) -> Result<Option<String>, Error> {
        Ok(self.info.clone())
    }
    fn set_device_info(&mut self, info: &str) -> Result<(), Error> {
        self.info = Some(info.to_string());
        Ok(())
    }
    fn trusted(&self) -> Result<Option<String>, Error> {
        Ok(self.trusted.clone())
    }
    fn trust(&mut self, host: &str) -> Result<(), Error> {
        self.trusted = Some(host.to_string());
        Ok(())
    }
    fn extensions(&self) -> Result<Vec<String>, Error> {
        Ok(EXTENSIONS.iter().map(|e| e.to_string()).collect())
    }
    fn password(&self) -> Result<Option<Secret>, Error> {
        self.password.as_deref().map(Secret::copy).transpose()
    }
    fn device_name(&self) -> Result<String, Error> {
        Ok("My Reader".to_string())
    }
    fn version(&self) -> &str {
        "1.0.9"
    }
    fn space(&self) -> (Option<u64>, Option<u64>) {
        (self.free, self.total)
    }
}

/// A device that allocates only through the fallible helpers, so that a
/// refused allocation reaches the caller as `NoMemory`: its answers are fixed.
pub struct Quiet;

impl Backing for Quiet {
    fn sent(&self, lpath: &str) -> Result<bool, Error> {
        Ok(lpath == "Ann/A.epub")
    }
    fn remember(&mut self, _: &str, _: &str) -> Result<(), Error> {
        Ok(())
    }
    fn forget(&mut self, _: &str) -> Result<Option<String>, Error> {
        copy("u-1").map(Some)
    }
    fn books(&self) -> Result<Vec<u128>, Error> {
        let mut books = Vec::new();
        books.try_reserve(2).map_err(|_| Error::NoMemory)?;
        books.extend([1, 2]);
        Ok(books)
    }
    fn metadata(&self, _: u128) -> Result<Option<String>, Error> {
        copy("{\"title\": \"A\", \"lpath\": \"Ann/A.epub\"}").map(Some)
    }
    fn device_info(&self) -> Result<Option<String>, Error> {
        copy("{\"k\": 1}").map(Some)
    }
    fn set_device_info(&mut self, _: &str) -> Result<(), Error> {
        Ok(())
    }
    fn trusted(&self) -> Result<Option<String>, Error> {
        copy("10.0.0.2").map(Some)
    }
    fn trust(&mut self, _: &str) -> Result<(), Error> {
        Ok(())
    }
    fn extensions(&self) -> Result<Vec<String>, Error> {
        let mut out = Vec::new();
        out.try_reserve(2).map_err(|_| Error::NoMemory)?;
        out.push(copy("epub")?);
        out.push(copy("txt")?);
        Ok(out)
    }
    fn password(&self) -> Result<Option<Secret>, Error> {
        Secret::copy("pw").map(Some)
    }
    fn device_name(&self) -> Result<String, Error> {
        copy("Reader")
    }
    fn version(&self) -> &str {
        "1.0.9"
    }
    fn space(&self) -> (Option<u64>, Option<u64>) {
        (Some(10), Some(20))
    }
}
