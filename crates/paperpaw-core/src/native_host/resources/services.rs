//! Service handles retain schema metadata, never plugin buffers or callbacks.
#[cfg(test)]
use super::HostResources;
use super::{Kind, OwnerResources, Resource};
use crate::abi7::{
    context::Frame,
    handles,
    services_wire::{ServiceOp, ServiceSchema},
};
use crate::native_host::admitted_features::AdmittedFeatures;
use crate::native_host::host_services::Caller;
use alloc::vec::Vec;
use core::cell::Cell;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_FULL, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};

/// A service family's stateless backend: the frame, the caller (owner,
/// plugin id, admitted features, the wired host views), the op, its input and
/// its output.
pub(crate) type Backend =
    fn(&dyn Frame, &Caller<'_>, &ServiceOp, &[u8], &mut [u8]) -> Result<usize, i32>;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ServiceKey {
    pub(crate) schema: &'static ServiceSchema,
    pub(crate) minor: u16,
    /// The opening instance's admitted features, for gated ops at call time.
    pub(crate) features: AdmittedFeatures,
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
        use crate::abi7::services_wire::{
            core_device_v1, core_document_v1, core_files_v1, core_library_v1, core_lookup_v1,
            core_net_v1, core_registry_v1, core_secrets_v1, core_sockets_v1, core_ui_v1,
            core_work_v1, service_schema,
        };
        use crate::native_host::{
            device_service, doc_service, files_service, lib_service, lookup_service, net_service,
            registry_service, secrets_service, sock_service, ui_service, work_service,
        };
        let implementations: [(&str, u16, Backend); 11] = [
            (
                core_device_v1::KEY,
                core_device_v1::MAJOR,
                device_service::backend,
            ),
            (
                core_document_v1::KEY,
                core_document_v1::MAJOR,
                doc_service::backend,
            ),
            (
                core_files_v1::KEY,
                core_files_v1::MAJOR,
                files_service::backend,
            ),
            (
                core_library_v1::KEY,
                core_library_v1::MAJOR,
                lib_service::backend,
            ),
            (
                core_lookup_v1::KEY,
                core_lookup_v1::MAJOR,
                lookup_service::backend,
            ),
            (core_ui_v1::KEY, core_ui_v1::MAJOR, ui_service::backend),
            (core_net_v1::KEY, core_net_v1::MAJOR, net_service::backend),
            (
                core_sockets_v1::KEY,
                core_sockets_v1::MAJOR,
                sock_service::backend,
            ),
            (
                core_work_v1::KEY,
                core_work_v1::MAJOR,
                work_service::backend,
            ),
            (
                core_secrets_v1::KEY,
                core_secrets_v1::MAJOR,
                secrets_service::backend,
            ),
            (
                core_registry_v1::KEY,
                core_registry_v1::MAJOR,
                registry_service::backend,
            ),
        ];
        let mut registry = Self {
            entries: Vec::new(),
        };
        for (key, major, backend) in implementations {
            let schema = service_schema(key, major).ok_or(ERR_NOT_FOUND)?;
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
        caller: &Caller<'_>,
        op: &ServiceOp,
        input: &[u8],
        out: &mut [u8],
    ) -> Result<usize, i32> {
        (self.entry.backend)(frame, caller, op, input, out)
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
    fn the_device_serves_the_files_family() {
        let registry = ServiceRegistry::device().unwrap();
        assert!(registry.schema("core.files").is_some());
    }

    #[test]
    fn device_registration_refuses_test_only_schema_in_code() {
        let mut registry = ServiceRegistry::device().unwrap();
        let registered = registry.entries.len();
        let test_only = crate::abi7::services_wire::SERVICES
            .iter()
            .find(|schema| schema.test_only)
            .unwrap();
        assert_eq!(
            registry.register(test_only, |_, _, _, _, _| Ok(0), false),
            Err(ERR_UNSUPPORTED)
        );
        assert_eq!(registry.entries.len(), registered, "nothing was added");
    }

    fn key(family: &str) -> ServiceKey {
        ServiceKey {
            schema: crate::abi7::services_wire::service_schema(family, 1).unwrap(),
            minor: 0,
            features: AdmittedFeatures::NONE,
        }
    }

    #[test]
    fn one_open_service_holds_room_for_four_handles() {
        // #248, P2: the handle table grows four slots at a time.
        let pool = HostResources::try_new().unwrap();
        let owner = pool.try_owner().unwrap();
        owner.insert_service(key("core.ui"), usize::MAX).unwrap();
        assert_eq!(owner.table.handles.borrow().capacity(), 4);
    }

    #[test]
    fn a_refused_growth_says_no_memory_and_keeps_the_open_handles() {
        use crate::native_host::shell::tests::FAIL_NTH;
        let pool = HostResources::try_new().unwrap();
        let owner = pool.try_owner().unwrap();
        let ui = owner.insert_service(key("core.ui"), usize::MAX).unwrap();
        // The first step's four slots are taken without a growth.
        for family in ["core.files", "core.library", "core.net"] {
            owner.insert_service(key(family), usize::MAX).unwrap();
        }
        FAIL_NTH.with(|n| n.set(1));
        let refused = owner.insert_service(key("core.device"), usize::MAX);
        FAIL_NTH.with(|n| n.set(0));
        assert_eq!(refused, Err(ERR_NO_MEMORY));
        let open = owner.service(ui).map(|key| key.schema.key);
        assert_eq!(open, Ok("core.ui"), "the open handle is kept");
        assert!(owner.insert_service(key("core.device"), usize::MAX).is_ok());
    }
}
