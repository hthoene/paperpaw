//! Reads from the registry that the screens need, as owned copies.
use crate::model::{Bound, GLOBAL, POLICY, Place};
use alloc::{string::String, vec::Vec};
use paperpaw_native_kit::{rows, text::copy};
use paperpaw_sdk::builtin::slots;
use paperpaw_sdk::native::{
    Error, Host,
    registry::{MenuQuery, Registry, RegistryBindingOrigin},
};

const INPUT: usize = 512;
const OUTPUT: usize = 4096;

fn buffer(len: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    bytes.resize(len, 0);
    Ok(bytes)
}

/// An open registry with its transport buffers, released by [`Self::finish`].
pub struct Reader<'h, 'cb> {
    host: &'h Host<'cb>,
    registry: Registry,
    input: Vec<u8>,
    output: Vec<u8>,
}

/// The layers that set a gesture policy and the user's changes (layer, input).
pub type PolicyAndChanges = (Vec<String>, Vec<(String, String)>);

/// An id and its host-translated title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub id: String,
    pub title: String,
}

impl<'h, 'cb> Reader<'h, 'cb> {
    pub fn open(host: &'h Host<'cb>) -> Result<Self, Error> {
        Ok(Self {
            host,
            registry: Registry::open(host)?,
            input: buffer(INPUT)?,
            output: buffer(OUTPUT)?,
        })
    }

    /// Releases the registry, whatever `result` is.
    pub fn finish<T>(self, result: Result<T, Error>) -> Result<T, Error> {
        let released = self.registry.release(self.host);
        let value = result?;
        released?;
        Ok(value)
    }

    /// The inputs this device has.
    pub fn inputs(&mut self) -> Result<Vec<Named>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.inputs(self.host, cursor, &mut self.input, &mut self.output), |row| {
            found.try_reserve(1).map_err(|_| Error::NoMemory)?;
            found.push(Named { id: copy(row.id())?, title: copy(row.title())? });
            Ok(())
        })?;
        Ok(found)
    }

    /// Every action: id and translated title.
    pub fn actions(&mut self) -> Result<Vec<Named>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.actions(self.host, "", cursor, &mut self.input, &mut self.output), |row| {
            found.try_reserve(1).map_err(|_| Error::NoMemory)?;
            found.push(Named { id: copy(row.id())?, title: copy(row.title())? });
            Ok(())
        })?;
        Ok(found)
    }

    /// Every screen: id and translated title.
    pub fn screens(&mut self) -> Result<Vec<Named>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.screens(self.host, "", cursor, &mut self.input, &mut self.output), |row| {
            found.try_reserve(1).map_err(|_| Error::NoMemory)?;
            found.push(Named { id: copy(row.id())?, title: copy(row.title())? });
            Ok(())
        })?;
        Ok(found)
    }

    /// The bindings of `layers`, as the host resolves them.
    pub fn bindings(&mut self, layers: &[String]) -> Result<Vec<Bound>, Error> {
        let mut found = Vec::new();
        for layer in layers {
            rows!(cursor => self.registry.bindings(self.host, layer, cursor, &mut self.input, &mut self.output), |row| {
                found.try_reserve(1).map_err(|_| Error::NoMemory)?;
                found.push(Bound {
                    layer: copy(row.layer())?,
                    input: copy(row.input())?,
                    action: copy(row.action())?,
                });
                Ok(())
            })?;
        }
        Ok(found)
    }

    /// The layers that set how they take gestures, and the layers (and inputs)
    /// the user changed.
    pub fn policy_layers_and_changes(&mut self) -> Result<PolicyAndChanges, Error> {
        let (mut policy, mut changes) = (Vec::new(), Vec::new());
        rows!(cursor => self.registry.bindings(self.host, "", cursor, &mut self.input, &mut self.output), |row| {
            if row.input() == POLICY && !policy.iter().any(|l: &String| l == row.layer()) {
                policy.try_reserve(1).map_err(|_| Error::NoMemory)?;
                policy.push(copy(row.layer())?);
            }
            if row.origin() == RegistryBindingOrigin::User {
                changes.try_reserve(1).map_err(|_| Error::NoMemory)?;
                changes.push((copy(row.layer())?, copy(row.input())?));
            }
            Ok(())
        })?;
        Ok((policy, changes))
    }

    /// The ids in the current entries of `menu`.
    pub fn menu_items(&mut self, menu: &str) -> Result<Vec<String>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.menu(self.host, menu, MenuQuery::Current, cursor, &mut self.input, &mut self.output), |row| {
            found.try_reserve(1).map_err(|_| Error::NoMemory)?;
            found.push(copy(row.item())?);
            Ok(())
        })?;
        Ok(found)
    }
}

/// The places changes may apply: everywhere, the home screen, reading, the
/// library; then the tools that say how they take gestures. `title` makes a
/// translated name from a catalog key.
pub fn places(
    title: &mut dyn FnMut(&str) -> Result<String, Error>,
    screens: &[Named],
    with_policy: &[String],
    all: bool,
) -> Result<Vec<Place>, Error> {
    let layers = |own: &str| -> Result<Vec<String>, Error> {
        let mut layers = Vec::new();
        layers.try_reserve(2).map_err(|_| Error::NoMemory)?;
        layers.push(copy(own)?);
        if own != GLOBAL {
            layers.push(copy(GLOBAL)?);
        }
        Ok(layers)
    };
    let mut places = Vec::new();
    for (key, layer) in [
        ("keys.place.everywhere", GLOBAL),
        ("keys.place.home", slots::HOME.as_str()),
        ("keys.place.reading", slots::READER.as_str()),
        ("keys.place.library", slots::LIBRARY.as_str()),
    ] {
        places.try_reserve(1).map_err(|_| Error::NoMemory)?;
        places.push(Place {
            title: title(key)?,
            layers: layers(layer)?,
        });
    }
    let mut screens_here: Vec<Place> = Vec::new();
    for screen in screens {
        // The reader binds on its slot's layer, which is a place already.
        if !all && !with_policy.contains(&screen.id) {
            continue;
        }
        screens_here.try_reserve(1).map_err(|_| Error::NoMemory)?;
        screens_here.push(Place {
            title: copy(&screen.title)?,
            layers: layers(&screen.id)?,
        });
    }
    screens_here.sort_by_cached_key(|p| (p.title.to_lowercase(), String::from(p.layer())));
    places.extend(screens_here);
    Ok(places)
}
