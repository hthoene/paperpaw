#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
use super::super::{
    declaration_store::{DeclarationStore, PendingDeclarations},
    registrations::{self as wire, SettingGroup},
};
use super::*;
use alloc::vec;
use paperpaw_sdk::Id;

struct Image {
    bytes: RefCell<Vec<u8>>,
    generation: Cell<u32>,
    plugin: Cell<&'static str>,
}
impl ImageOwner for Image {
    fn plugin_id(&self) -> &str {
        self.plugin.get()
    }
    fn generation(&self) -> u32 {
        self.generation.get()
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        match self.bytes.try_borrow() {
            Ok(bytes) => visit(Some(&bytes)),
            Err(_) => visit(None),
        }
    }
    fn invalid_declaration(&self, _: i32) {}
}
fn fixture() -> (Rc<Image>, Row) {
    let record = Record::SettingGroup(SettingGroup {
        name: "group",
        title: "Group",
        order: 0,
        page: "",
        parent: "",
    });
    fixture_record(record)
}
fn fixture_record(record: Record<'_>) -> (Rc<Image>, Row) {
    let mut bytes = vec![0; wire::REGISTRATION_MAX_BYTES];
    let len = wire::records_encode(&[record], &mut bytes).unwrap();
    bytes.truncate(len);
    let frame = wire::record_framed_len(&record).unwrap();
    let span = Span::new(len - frame, frame, tag(&record).unwrap()).unwrap();
    (
        Rc::new(Image {
            bytes: RefCell::new(bytes),
            generation: Cell::new(1),
            plugin: Cell::new("fixture"),
        }),
        Row::new(0, span).unwrap(),
    )
}
fn query(catalog: &RegCatalog, generation: u32) -> Result<(), i32> {
    catalog.with_record(generation, 0, &mut |record| {
        assert!(matches!(record, Record::SettingGroup(_)));
        Ok(())
    })
}

#[test]
fn dropping_plugin_store_makes_query_stale_before_next_apply_prunes_it() {
    let (image, row) = fixture();
    let source: Rc<dyn ImageOwner> = image.clone();
    let mut allocations = Allocations::default();
    let mut pending = PendingDeclarations::new(Id::new_static("fixture"), source.clone());
    pending
        .stage_batch(&image.bytes.borrow(), &mut allocations)
        .unwrap();
    let mut store = DeclarationStore::default();
    store.prepare(pending, &mut allocations).unwrap().commit();
    let catalog = RegCatalog::new();
    let generation = catalog
        .replace(core::slice::from_ref(&source), &[row], &mut allocations)
        .unwrap();
    drop(image);
    drop(source);
    assert_eq!(query(&catalog, generation), Ok(()));
    drop(store);
    assert_eq!(query(&catalog, generation), Err(ERR_STATE));
    let next = catalog.replace(&[], &[], &mut allocations).unwrap();
    assert_eq!(next, generation + 1);
    assert_eq!(query(&catalog, generation), Err(ERR_STATE));
    assert!(catalog.data.borrow().rows.is_empty());
    assert!(catalog.data.borrow().owners.is_empty());
}

#[test]
fn source_generation_ranges_and_decoding_are_checked_on_every_query() {
    let (image, row) = fixture();
    let source: Rc<dyn ImageOwner> = image.clone();
    let catalog = RegCatalog::new();
    let generation = catalog
        .replace(&[source], &[row], &mut Allocations::default())
        .unwrap();
    image.generation.set(2);
    assert_eq!(query(&catalog, generation), Err(ERR_STATE));
    image.generation.set(1);
    image.bytes.borrow_mut().truncate(1);
    assert_eq!(query(&catalog, generation), Err(ERR_STATE));
    assert_eq!(catalog.generation(), generation);
}

#[test]
fn invalid_owner_index_and_decode_errors_never_publish_partial_updates() {
    let (image, row) = fixture();
    let source: Rc<dyn ImageOwner> = image.clone();
    let catalog = RegCatalog::new();
    let generation = catalog
        .replace(
            core::slice::from_ref(&source),
            &[row],
            &mut Allocations::default(),
        )
        .unwrap();
    let bad_owner = Row { owner: 1, ..row };
    assert_eq!(
        catalog.replace(
            core::slice::from_ref(&source),
            &[bad_owner],
            &mut Allocations::default()
        ),
        Err(ERR_STATE)
    );
    assert_eq!(catalog.generation(), generation);
    assert_eq!(query(&catalog, generation), Ok(()));
    // This is metadata corruption, not a hand-written encoder: erase the frame.
    image.bytes.borrow_mut().fill(0xff);
    assert_eq!(query(&catalog, generation), Err(ERR_STATE));
    assert_eq!(
        catalog.replace(&[source], &[row], &mut Allocations::default()),
        Err(ERR_STATE)
    );
    assert_eq!(catalog.generation(), generation);
}

#[test]
fn every_reservation_failure_and_live_borrow_preserve_catalog_generation() {
    let (image, row) = fixture();
    let source: Rc<dyn ImageOwner> = image;
    let catalog = RegCatalog::new();
    let generation = catalog
        .replace(
            core::slice::from_ref(&source),
            &[row],
            &mut Allocations::default(),
        )
        .unwrap();
    for fail_after in 0..4 {
        let mut allocations = Allocations {
            fail_after: Some(fail_after),
        };
        assert_eq!(
            catalog.replace(core::slice::from_ref(&source), &[row], &mut allocations),
            Err(paperpaw_native::host_glue::ERR_NO_MEMORY)
        );
        assert_eq!(catalog.generation(), generation);
        assert_eq!(query(&catalog, generation), Ok(()));
    }
    catalog
        .with_record(generation, 0, &mut |_| {
            assert_eq!(
                catalog.replace(&[], &[], &mut Allocations::default()),
                Err(ERR_BUSY)
            );
            Ok(())
        })
        .unwrap();
    let _exclusive = catalog.data.borrow_mut();
    assert_eq!(query(&catalog, generation), Err(ERR_BUSY));
}

#[test]
fn packed_rows_and_counts_refuse_narrowing_and_generation_wrap() {
    assert_eq!(core::mem::size_of::<Span>(), 8);
    assert_eq!(
        core::mem::size_of::<Row>() + 2 * core::mem::size_of::<u16>(),
        16
    );
    assert!(matches!(Span::new(0, 65536, 21), Err(ERR_NO_SPACE)));
    assert!(matches!(Span::new(0, 1, 65536), Err(ERR_NO_SPACE)));
    assert!(matches!(
        Row::new(65536, Span::new(0, 1, 21).unwrap()),
        Err(ERR_NO_SPACE)
    ));
    let (image, row) = fixture();
    let source: Rc<dyn ImageOwner> = image;
    let catalog = RegCatalog::new();
    let rows = vec![row; 65536];
    assert_eq!(
        catalog.replace(
            core::slice::from_ref(&source),
            &rows,
            &mut Allocations::default()
        ),
        Err(ERR_NO_SPACE)
    );
    catalog.generation.set(u32::MAX);
    assert_eq!(
        catalog.replace(&[source], &[row], &mut Allocations::default()),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(catalog.generation(), u32::MAX);
}

#[test]
fn owner_identity_mismatch_is_stale_instead_of_rewriting_full_ids() {
    let (image, row) = fixture_record(Record::SlotOffer(wire::SlotOffer {
        slot: "core.home",
        screen: "fixture.reader",
        priority: 0,
    }));
    let source: Rc<dyn ImageOwner> = image.clone();
    let catalog = RegCatalog::new();
    let generation = catalog
        .replace(&[source], &[row], &mut Allocations::default())
        .unwrap();
    image.plugin.set("other");
    assert_eq!(
        catalog.with_record(generation, 0, &mut |_| panic!(
            "mismatched row reached reader"
        )),
        Err(ERR_STATE)
    );
}
