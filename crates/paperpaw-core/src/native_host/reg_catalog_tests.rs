#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
use super::super::{
    declaration_store::{ImageOwner, Kind, PendingDeclarations},
    registrations::{self as wire, SettingGroup},
};
use super::*;
use alloc::{rc::Rc, vec};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY};
use paperpaw_sdk::Id;

struct Image {
    bytes: RefCell<Vec<u8>>,
    generation: Cell<u32>,
    last_error: Cell<i32>,
}
impl ImageOwner for Image {
    fn plugin_id(&self) -> &str {
        "fixture"
    }
    fn generation(&self) -> u32 {
        self.generation.get()
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        let bytes = self.bytes.borrow();
        visit(Some(&bytes));
    }
    fn invalid_declaration(&self, error: i32) {
        self.last_error.set(error);
    }
}
fn image() -> Rc<Image> {
    let record = Record::SettingGroup(SettingGroup {
        name: "group",
        title: "Group",
        order: 0,
        page: "",
        parent: "",
    });
    let mut bytes = vec![0; wire::REGISTRATION_MAX_BYTES];
    let len = wire::records_encode(&[record], &mut bytes).unwrap();
    bytes.truncate(len);
    Rc::new(Image {
        bytes: RefCell::new(bytes),
        generation: Cell::new(1),
        last_error: Cell::new(0),
    })
}
fn install(store: &RefCell<DeclarationStore>, image: &Rc<Image>, owned: bool) -> DeclKey {
    let mut pending = PendingDeclarations::new(Id::new_static("fixture"), image.clone());
    let mut allocations = Allocations::default();
    if owned {
        pending
            .stage_batch(&image.bytes.borrow().clone(), &mut allocations)
            .unwrap();
    } else {
        pending
            .stage_batch(&image.bytes.borrow(), &mut allocations)
            .unwrap();
    }
    let mut store = store.borrow_mut();
    store.prepare(pending, &mut allocations).unwrap().commit();
    let mut key = None;
    store.keys(Kind::Group, &mut |found| key = Some(found));
    key.unwrap()
}
fn query(
    catalog: &RegCatalog,
    store: &RefCell<DeclarationStore>,
    generation: u32,
) -> Result<(), i32> {
    catalog.with_record(store, generation, 0, &mut |record| {
        assert!(matches!(record, Record::SettingGroup(_)));
        Ok(())
    })
}
#[test]
fn image_and_owned_rows_share_the_store_visit_and_survive_input_drop() {
    for owned in [false, true] {
        let store = RefCell::new(DeclarationStore::default());
        let source = image();
        let row = install(&store, &source, owned);
        let catalog = RegCatalog::new();
        let generation = catalog
            .replace(&store, &[row], &mut Allocations::default())
            .unwrap();
        assert_eq!(
            Rc::strong_count(&source),
            2,
            "catalog must not retain the owner"
        );
        drop(source);
        assert_eq!(query(&catalog, &store, generation), Ok(()));
    }
}
#[test]
fn removed_plugin_and_reused_owner_slot_make_catalog_rows_stale() {
    for owned in [false, true] {
        let store = RefCell::new(DeclarationStore::default());
        let source = image();
        let row = install(&store, &source, owned);
        let catalog = RegCatalog::new();
        let generation = catalog
            .replace(&store, &[row], &mut Allocations::default())
            .unwrap();
        store
            .borrow_mut()
            .remove_owner(&Id::new_static("fixture"), 1);
        assert_eq!(query(&catalog, &store, generation), Err(ERR_STATE));
        let new = image();
        let new_row = install(&store, &new, owned);
        assert_eq!(query(&catalog, &store, generation), Err(ERR_STATE));
        let next = catalog
            .replace(&store, &[new_row], &mut Allocations::default())
            .unwrap();
        assert_eq!(query(&catalog, &store, generation), Err(ERR_STATE));
        assert_eq!(query(&catalog, &store, next), Ok(()));
    }
}
#[test]
fn source_and_store_generation_changes_are_stale() {
    let store = RefCell::new(DeclarationStore::default());
    let source = image();
    let row = install(&store, &source, false);
    let catalog = RegCatalog::new();
    let generation = catalog
        .replace(&store, &[row], &mut Allocations::default())
        .unwrap();
    source.generation.set(2);
    assert_eq!(query(&catalog, &store, generation), Err(ERR_STATE));
    let replacement = image();
    replacement.generation.set(2);
    install(&store, &replacement, false);
    source.generation.set(1);
    assert_eq!(query(&catalog, &store, generation), Err(ERR_STATE));
}
#[test]
fn malformed_image_ranges_log_invalid_and_map_to_state_without_publication() {
    let store = RefCell::new(DeclarationStore::default());
    let source = image();
    let row = install(&store, &source, false);
    let catalog = RegCatalog::new();
    let generation = catalog
        .replace(&store, &[row], &mut Allocations::default())
        .unwrap();
    source.bytes.borrow_mut().truncate(1);
    assert_eq!(query(&catalog, &store, generation), Err(ERR_STATE));
    assert_eq!(source.last_error.get(), ERR_INVALID);
    assert_eq!(
        catalog.replace(&store, &[row], &mut Allocations::default()),
        Err(ERR_STATE)
    );
    assert_eq!(catalog.generation(), generation);
}
#[test]
fn busy_store_catalog_and_failed_reservations_preserve_publication() {
    let store = RefCell::new(DeclarationStore::default());
    let source = image();
    let row = install(&store, &source, true);
    let catalog = RegCatalog::new();
    let generation = catalog
        .replace(&store, &[row], &mut Allocations::default())
        .unwrap();
    for fail_after in 0..3 {
        assert_eq!(
            catalog.replace(
                &store,
                &[row],
                &mut Allocations {
                    fail_after: Some(fail_after),
                    ..Default::default()
                }
            ),
            Err(ERR_NO_MEMORY)
        );
        assert_eq!(catalog.generation(), generation);
        assert_eq!(query(&catalog, &store, generation), Ok(()));
    }
    {
        let _exclusive = store.borrow_mut();
        assert_eq!(query(&catalog, &store, generation), Err(ERR_BUSY));
        assert_eq!(
            catalog.replace(&store, &[row], &mut Allocations::default()),
            Err(ERR_BUSY)
        );
    }
    let _exclusive = catalog.data.borrow_mut();
    assert_eq!(query(&catalog, &store, generation), Err(ERR_BUSY));
}
#[test]
fn row_size_and_count_caps_are_checked() {
    assert_eq!(core::mem::size_of::<DeclKey>(), 16);
    let store = RefCell::new(DeclarationStore::default());
    let source = image();
    let row = install(&store, &source, true);
    let catalog = RegCatalog::new();
    assert_eq!(
        catalog.replace(&store, &vec![row; 65536], &mut Allocations::default()),
        Err(ERR_NO_SPACE)
    );
}

#[test]
fn generation_wrap_skips_zero() {
    let store = RefCell::new(DeclarationStore::default());
    let catalog = RegCatalog::new();
    catalog.generation.set(u32::MAX);
    assert_eq!(
        catalog.replace(&store, &[], &mut Allocations::default()),
        Ok(1)
    );
}
