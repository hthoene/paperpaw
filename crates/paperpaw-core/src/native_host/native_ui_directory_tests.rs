//! Publication and lifecycle regressions through the checked native producer.
use super::*;
use crate::native_host::{
    declaration_store::{Allocations, ImageOwner, PendingDeclarations},
    registrations::{self as wire, StatusItem, StatusLine, WidgetSpec},
};
use alloc::rc::Rc;
use core::cell::Cell;
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_STATE};
use paperpaw_sdk::Id;

struct Source {
    plugin: &'static str,
    owner: u64,
    generation: Rc<Cell<u32>>,
}
impl ImageOwner for Source {
    fn plugin_id(&self) -> &str {
        self.plugin
    }
    fn generation(&self) -> u32 {
        self.generation.get()
    }
    fn owner_id(&self) -> Result<u64, i32> {
        Ok(self.owner)
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(Some(&[]));
    }
    fn invalid_declaration(&self, status: i32) {
        panic!("valid owned fixture declaration became invalid: {status}");
    }
}

fn pending(plugin: &'static str, owner: u64, generation: &Rc<Cell<u32>>) -> PendingDeclarations {
    let source = Source {
        plugin,
        owner,
        generation: Rc::clone(generation),
    };
    // Read the identity through the trait so the field stays live on old compilers.
    assert_eq!(ImageOwner::plugin_id(&source), plugin);
    let mut pending = PendingDeclarations::native(Id::new_static(plugin), source).unwrap();
    let records = [
        Record::WidgetSpec(WidgetSpec {
            name: "shared",
            title: "Widget",
            category: "Widgets",
            previews: "",
            cells: 0,
            token: 17,
            flags: 3,
        }),
        Record::StatusItem(StatusItem {
            name: "shared",
            title: "Status item",
            tap: "",
            priority: 255,
            min_width: 2000,
            token: 23,
            flags: 0,
        }),
        Record::StatusLine(StatusLine {
            name: "shared",
            title: "Status line",
            height: 1000,
            token: 41,
            flags: 2,
        }),
    ];
    let mut bytes = [0; 512];
    let length = wire::records_encode(&records, &mut bytes).unwrap();
    pending
        .stage_batch(&bytes[..length], &mut Allocations::default())
        .unwrap();
    pending
}

fn publish(directory: &NativeDeclarations, plugin: &'static str, owner: u64, live: &Rc<Cell<u32>>) {
    directory
        .stage(
            pending(plugin, owner, live),
            owner,
            &mut Allocations::default(),
        )
        .unwrap()
        .commit();
}

fn assert_fixture(directory: &NativeDeclarations, plugin: &str, owner: u64) {
    let id = alloc::format!("{plugin}.shared");
    assert_eq!(
        directory.widget(&id),
        Ok(Declared {
            owner,
            token: 17,
            private: true
        })
    );
    assert_eq!(
        directory.status_item(&id),
        Ok(Declared {
            owner,
            token: 23,
            private: false
        })
    );
    assert_eq!(
        directory.status_line(&id),
        Ok(Declared {
            owner,
            token: 41,
            private: true
        })
    );
}

fn assert_absent(directory: &NativeDeclarations, plugin: &str) {
    let id = alloc::format!("{plugin}.shared");
    assert_eq!(directory.widget(&id), Err(ERR_NOT_FOUND));
    assert_eq!(directory.status_item(&id), Err(ERR_NOT_FOUND));
    assert_eq!(directory.status_line(&id), Err(ERR_NOT_FOUND));
}

fn assert_catalog_fixture(directory: &NativeDeclarations, generation: u32) {
    for (index, token) in [17, 23, 41].into_iter().enumerate() {
        let mut visited = false;
        directory
            .catalog
            .with_record(&directory.store, generation, index, &mut |record| {
                let actual = match record {
                    Record::WidgetSpec(row) => row.token,
                    Record::StatusItem(row) => row.token,
                    Record::StatusLine(row) => row.token,
                    _ => return Err(ERR_STATE),
                };
                assert_eq!(actual, token);
                visited = true;
                Ok(())
            })
            .unwrap();
        assert!(visited);
    }
}

#[test]
fn publication_preserves_full_owner_and_separates_declaration_kinds() {
    let directory = NativeDeclarations::default();
    let live = Rc::new(Cell::new(1));
    publish(&directory, "fixture", u64::MAX, &live);
    assert_fixture(&directory, "fixture", u64::MAX);
    assert_catalog_fixture(&directory, directory.catalog.generation());
    assert_eq!(directory.widget("fixture.absent"), Err(ERR_NOT_FOUND));
}

#[test]
fn stopped_native_owner_disappears_from_every_directory_lookup() {
    let directory = NativeDeclarations::default();
    let live = Rc::new(Cell::new(1));
    publish(&directory, "fixture", u64::MAX, &live);
    assert_fixture(&directory, "fixture", u64::MAX);
    live.set(0);
    assert_absent(&directory, "fixture");
}

#[test]
fn removing_host_owner_uses_full_identity_and_leaves_other_owner_visible() {
    let directory = NativeDeclarations::default();
    let live = Rc::new(Cell::new(1));
    let other_live = Rc::new(Cell::new(1));
    publish(&directory, "fixture", u64::MAX, &live);
    publish(&directory, "other", u64::from(u16::MAX), &other_live);
    directory.store.borrow_mut().remove_host_owner(u64::MAX);
    assert_absent(&directory, "fixture");
    assert_fixture(&directory, "other", u64::from(u16::MAX));
}

#[test]
fn dropping_uncommitted_publication_keeps_catalog_and_prior_owner_unchanged() {
    let directory = NativeDeclarations::default();
    let live = Rc::new(Cell::new(1));
    let other_live = Rc::new(Cell::new(1));
    publish(&directory, "fixture", 11, &live);
    let generation = directory.catalog.generation();
    {
        let publication = directory
            .stage(
                pending("other", 29, &other_live),
                29,
                &mut Allocations::default(),
            )
            .unwrap();
        assert_eq!(directory.catalog.generation(), generation);
        assert_eq!(directory.widget("other.shared"), Err(ERR_BUSY));
        drop(publication);
    }
    assert_eq!(directory.catalog.generation(), generation);
    assert_absent(&directory, "other");
    assert_fixture(&directory, "fixture", 11);
    assert_catalog_fixture(&directory, generation);
}

#[test]
fn every_publication_allocation_failure_preserves_previous_rows() {
    let mut refused = 0;
    let mut completed = false;
    for fail_after in 0..32 {
        let directory = NativeDeclarations::default();
        let live = Rc::new(Cell::new(1));
        let other_live = Rc::new(Cell::new(1));
        publish(&directory, "fixture", u64::MAX, &live);
        let generation = directory.catalog.generation();
        let mut allocations = Allocations {
            fail_after: Some(fail_after),
        };
        let result = directory.stage(pending("other", 29, &other_live), 29, &mut allocations);
        match result {
            Err(status) => {
                assert_eq!(status, ERR_NO_MEMORY);
                refused += 1;
                assert_eq!(directory.catalog.generation(), generation);
                assert_absent(&directory, "other");
                assert_catalog_fixture(&directory, generation);
            }
            Ok(publication) => {
                publication.commit();
                assert_ne!(directory.catalog.generation(), generation);
                assert_fixture(&directory, "other", 29);
                completed = true;
            }
        }
        assert_fixture(&directory, "fixture", u64::MAX);
        if completed {
            break;
        }
    }
    // The producer, key collection and each of the three catalog tables reserve.
    assert!(refused >= 5);
    assert!(completed);
}
