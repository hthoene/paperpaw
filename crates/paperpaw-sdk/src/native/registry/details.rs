//! Exact typed replies for the three scalar registry operations.
use super::*;

/// Complete metadata for one setting; choices remain separately paged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistrySettingDetail<'a> {
    setting: RegistrySetting<'a>,
    extra: RegistrySettingExtra<'a>,
    help: Option<&'a str>,
}
impl<'a> RegistrySettingDetail<'a> {
    /// Basic setting metadata and readable current value.
    #[must_use]
    pub const fn setting(&self) -> RegistrySetting<'a> {
        self.setting
    }
    /// Range, editor, condition and choice-count metadata.
    #[must_use]
    pub const fn extra(&self) -> RegistrySettingExtra<'a> {
        self.extra
    }
    /// Complete translated help text, when supplied.
    #[must_use]
    pub const fn help(&self) -> Option<&'a str> {
        self.help
    }
}

/// Inventory and description for one plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryPluginDetail<'a> {
    plugin: RegistryPlugin<'a>,
    description: Option<&'a str>,
}
impl<'a> RegistryPluginDetail<'a> {
    /// Plugin identity, state and installed version.
    #[must_use]
    pub const fn plugin(&self) -> RegistryPlugin<'a> {
        self.plugin
    }
    /// Complete translated description, when supplied.
    #[must_use]
    pub const fn description(&self) -> Option<&'a str> {
        self.description
    }
}

/// Name and complete description for one preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryPresetDetail<'a> {
    preset: RegistryItem<'a>,
    description: Option<&'a str>,
}
impl<'a> RegistryPresetDetail<'a> {
    /// Preset identity, owning plugin and translated name.
    #[must_use]
    pub const fn preset(&self) -> RegistryItem<'a> {
        self.preset
    }
    /// Complete translated description, when supplied.
    #[must_use]
    pub const fn description(&self) -> Option<&'a str> {
        self.description
    }
}

impl Registry {
    fn detail<'a>(
        &self,
        host: &Host<'_>,
        op: u32,
        key: &str,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DetailRecords<'a>, Error> {
        if key.is_empty() {
            return Err(Error::Invalid);
        }
        let bytes = self.call(
            host,
            op,
            Record::ServiceRegKey(wire::ServiceRegKey { key }),
            input,
            out,
        )?;
        Ok(DetailRecords::new(bytes))
    }

    /// Read one setting's metadata and complete help text.
    ///
    /// # Errors
    /// Returns NotFound for an unknown key, encoding or host errors, or Invalid
    /// for the wrong record order, a malformed value or extra records.
    pub fn setting<'a>(
        &self,
        host: &Host<'_>,
        key: &str,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistrySettingDetail<'a>, Error> {
        let mut records = self.detail(host, schema::OP_SETTING, key, input, out)?;
        let setting = RegistrySetting::decode(records.required()?)?;
        let extra = RegistrySettingExtra::decode(records.required()?)?;
        let help = optional_text(&mut records)?;
        if setting.key() != key {
            return Err(Error::Invalid);
        }
        Ok(RegistrySettingDetail {
            setting,
            extra,
            help,
        })
    }

    /// Read one plugin's inventory and complete description.
    ///
    /// # Errors
    /// Returns NotFound for an unknown key, encoding or host errors, or Invalid
    /// for inconsistent, misordered or extra records.
    pub fn plugin<'a>(
        &self,
        host: &Host<'_>,
        key: &str,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPluginDetail<'a>, Error> {
        let mut records = self.detail(host, schema::OP_PLUGIN, key, input, out)?;
        let plugin = RegistryPlugin::decode(records.required()?)?;
        let description = optional_text(&mut records)?;
        if plugin.id() != key {
            return Err(Error::Invalid);
        }
        Ok(RegistryPluginDetail {
            plugin,
            description,
        })
    }

    /// Read one preset's name and complete description.
    ///
    /// # Errors
    /// Returns NotFound for an unknown key, encoding or host errors, or Invalid
    /// for inconsistent, misordered or extra records.
    pub fn preset<'a>(
        &self,
        host: &Host<'_>,
        key: &str,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPresetDetail<'a>, Error> {
        let mut records = self.detail(host, schema::OP_PRESET, key, input, out)?;
        let preset = RegistryItem::decode(records.required()?)?;
        let description = optional_text(&mut records)?;
        if preset.id() != key {
            return Err(Error::Invalid);
        }
        Ok(RegistryPresetDetail {
            preset,
            description,
        })
    }
}

fn optional_text<'a>(records: &mut DetailRecords<'a>) -> Result<Option<&'a str>, Error> {
    let text = match records.next()? {
        None => None,
        Some(Record::ServiceRegText(text)) => Some(text.text),
        Some(_) => return Err(Error::Invalid),
    };
    records.finish()?;
    Ok(text)
}

struct DetailRecords<'a> {
    bytes: &'a [u8],
}
impl<'a> DetailRecords<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
    fn next(&mut self) -> Result<Option<Record<'a>>, Error> {
        if self.bytes.is_empty() {
            return Ok(None);
        }
        let (record, length) =
            wire::record_decode_prefix(self.bytes).map_err(|_| Error::Invalid)?;
        self.bytes = self.bytes.get(length..).ok_or(Error::Invalid)?;
        Ok(Some(record))
    }
    fn required(&mut self) -> Result<Record<'a>, Error> {
        self.next()?.ok_or(Error::Invalid)
    }
    fn finish(&self) -> Result<(), Error> {
        if self.bytes.is_empty() {
            Ok(())
        } else {
            Err(Error::Invalid)
        }
    }
}
