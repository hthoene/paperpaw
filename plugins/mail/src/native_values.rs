//! An account's values as the host has them: the settings through
//! `settings.read` and the password through `core.secrets`, read for one
//! connection and wiped when it is made (the reply buffer is wiped at once, the
//! copy is a [`Secret`]).
use crate::native_model::account::{Values, secret_name};
use crate::native_model::mem::{Error, Secret, copy};
use alloc::string::String;
use paperpaw_sdk::native::{
    Host,
    secrets::{Secrets, wipe},
    settings::{self, SettingValue},
};
use paperpaw_ui_kit::mem::buffer;

/// The longest text setting (a host name is at most 253 bytes).
const TEXT: usize = 320;
/// The input and the reply of a secrets call.
const INPUT: usize = 1280;
const REPLY: usize = 4096;

/// The settings and the secrets of this plugin, for one reading.
pub(crate) struct HostValues<'h> {
    pub(crate) host: &'h Host<'h>,
    pub(crate) secrets: &'h Secrets,
}

impl Values for HostValues<'_> {
    fn text(&self, id: &str) -> Result<String, Error> {
        let mut bytes = [0u8; TEXT];
        match settings::read_setting(self.host, id, &mut bytes)? {
            SettingValue::Text(text) => copy(text),
            // Unset, or not a text: nothing.
            _ => Ok(String::new()),
        }
    }

    fn int(&self, id: &str) -> Result<i64, Error> {
        let mut bytes = [0u8; 64];
        match settings::read_setting(self.host, id, &mut bytes)? {
            SettingValue::Int(value) => Ok(value),
            _ => Ok(0),
        }
    }

    fn secret(&self, id: &str) -> Result<Option<Secret>, Error> {
        let mut input = buffer(INPUT)?;
        let mut reply = buffer(REPLY)?;
        let found = match self
            .secrets
            .get(self.host, secret_name(id), &mut input, &mut reply)
        {
            Ok(value) => {
                let mut secret = Secret::with_capacity(value.value().len())?;
                secret.push_str(value.value())?;
                Ok(Some(secret))
            }
            Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        };
        wipe(&mut input);
        wipe(&mut reply);
        found
    }
}

/// The settings of this plugin alone, for what needs no secret (which
/// accounts are set up, a provider's servers): a secret is never asked for.
pub(crate) struct SettingsValues<'h>(pub(crate) &'h Host<'h>);

impl Values for SettingsValues<'_> {
    fn text(&self, id: &str) -> Result<String, Error> {
        let mut bytes = [0u8; TEXT];
        match settings::read_setting(self.0, id, &mut bytes)? {
            SettingValue::Text(text) => copy(text),
            _ => Ok(String::new()),
        }
    }

    fn int(&self, id: &str) -> Result<i64, Error> {
        let mut bytes = [0u8; 64];
        match settings::read_setting(self.0, id, &mut bytes)? {
            SettingValue::Int(value) => Ok(value),
            _ => Ok(0),
        }
    }

    fn secret(&self, _: &str) -> Result<Option<Secret>, Error> {
        Ok(None)
    }
}
