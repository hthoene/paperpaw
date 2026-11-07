//! Service handles retain schema metadata, never plugin buffers or callbacks.
#[cfg(test)]
use super::HostResources;
use super::{Kind, OwnerResources, Resource};
use crate::abi7::{
    context::Frame,
    handles,
    services_wire::{ServiceOp, ServiceSchema},
};
use alloc::vec::Vec;
use core::cell::Cell;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_FULL, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};

pub(crate) type Backend = fn(&dyn Frame, &ServiceOp, &[u8], &mut [u8]) -> Result<usize, i32>;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ServiceKey {
    pub(crate) schema: &'static ServiceSchema,
    pub(crate) minor: u16,
}

#[derive(Debug)]
struct Registered {
    schema: &'static ServiceSchema,
    backend: Backend,
    active: Cell<bool>,
}

#[derive(Debug)]
pub(super) struct ServiceRegistry {
    entries: Vec<Registered>,
}

impl ServiceRegistry {
    pub(super) fn device() -> Result<Self, i32> {
        // New service families add only their implemented backends here. Schema
        // presence alone is not implementation, and test schemas stay refused.
        let implementations: &[(&'static ServiceSchema, Backend)] = &[];
        let mut registry = Self {
            entries: Vec::new(),
        };
        for &(schema, backend) in implementations {
            registry.register(schema, backend, false)?;
        }
        Ok(registry)
    }

    fn register(
        &mut self,
        schema: &'static ServiceSchema,
        backend: Backend,
        test_host: bool,
    ) -> Result<(), i32> {
        if schema.test_only && !(cfg!(test) && test_host) {
            return Err(ERR_UNSUPPORTED);
        }
        self.entries
            .try_reserve_exact(1)
            .map_err(|_| ERR_NO_MEMORY)?;
        self.entries.push(Registered {
            schema,
            backend,
            active: Cell::new(false),
        });
        Ok(())
    }

    #[cfg(test)]
    fn test(backend: Backend) -> Result<Self, i32> {
        let mut registry = Self {
            entries: Vec::new(),
        };
        let schema = crate::abi7::services_wire::service_schema(
            crate::abi7::services_wire::core_test_echo_v1::KEY,
            crate::abi7::services_wire::core_test_echo_v1::MAJOR,
        )
        .ok_or(ERR_NOT_FOUND)?;
        registry.register(schema, backend, true)?;
        Ok(registry)
    }

    fn schema(&self, key: &str) -> Option<&'static ServiceSchema> {
        self.entries
            .iter()
            .find(|entry| entry.schema.key == key)
            .map(|entry| entry.schema)
    }

    fn enter(&self, key: ServiceKey) -> Result<ServiceCall<'_>, i32> {
        let entry = self
            .entries
            .iter()
            .find(|entry| {
                entry.schema.key == key.schema.key && entry.schema.major == key.schema.major
            })
            .ok_or(ERR_STATE)?;
        if entry.active.replace(true) {
            return Err(ERR_BUSY);
        }
        Ok(ServiceCall { entry })
    }
}

pub(crate) struct ServiceCall<'a> {
    entry: &'a Registered,
}
impl ServiceCall<'_> {
    pub(crate) fn run(
        &self,
        frame: &dyn Frame,
        op: &ServiceOp,
        input: &[u8],
        out: &mut [u8],
    ) -> Result<usize, i32> {
        (self.entry.backend)(frame, op, input, out)
    }
}
impl Drop for ServiceCall<'_> {
    fn drop(&mut self) {
        self.entry.active.set(false);
    }
}

impl OwnerResources {
    pub(crate) fn service_schema(&self, key: &str) -> Result<&'static ServiceSchema, i32> {
        self.table.services.schema(key).ok_or(ERR_NOT_FOUND)
    }

    pub(crate) fn insert_service(&self, key: ServiceKey, limit: usize) -> Result<u32, i32> {
        let result = {
            let mut table = self.table.handles.try_borrow_mut().map_err(|_| ERR_BUSY)?;
            // Only this owner's keys count. Other owners may open the same key.
            let mut owned_duplicate = false;
            table.each_owner_mut(self.owner, |resource| {
                if let Resource::Service(existing) = resource {
                    owned_duplicate |= existing.schema.key == key.schema.key;
                }
            });
            if owned_duplicate {
                return Err(ERR_FULL);
            }
            table.insert_limited(self.owner, Kind::Service, Resource::Service(key), limit)
        };
        result.map_err(|(error, _)| error.status())
    }

    pub(crate) fn service(&self, handle: u32) -> Result<ServiceKey, i32> {
        let table = self.table.handles.try_borrow().map_err(|_| ERR_BUSY)?;
        match table
            .get(self.owner, Kind::Service, handle)
            .map_err(handles::Error::status)?
        {
            Resource::Service(key) => Ok(*key),
            _ => Err(ERR_STATE),
        }
    }

    pub(crate) fn enter_service(&self, key: ServiceKey) -> Result<ServiceCall<'_>, i32> {
        self.table.services.enter(key)
    }
}

#[cfg(test)]
impl HostResources {
    pub(crate) fn with_test_service(backend: Backend) -> Result<Self, super::FlatError> {
        Self::with_services(
            super::blobs::Limits::X4,
            ServiceRegistry::test(backend).map_err(|_| super::FlatError::Allocation)?,
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
    use super::*;
    #[test]
    fn device_registration_refuses_test_only_schema_in_code() {
        let mut registry = ServiceRegistry::device().unwrap();
        assert_eq!(
            registry.register(
                &crate::abi7::services_wire::SERVICES[0],
                |_, _, _, _| Ok(0),
                false
            ),
            Err(ERR_UNSUPPORTED)
        );
        assert!(registry.entries.is_empty());
    }
}
