use super::*;

#[test]
fn a_prepared_generation_is_published_only_by_commit() {
    let catalog = RegCatalog::new();
    let before = catalog.generation();
    let prepared = catalog.prepare();
    assert_eq!(catalog.generation(), before);
    let next = prepared.commit();
    assert_ne!(next, before);
    assert_eq!(catalog.generation(), next);
}

#[test]
fn prune_moves_the_generation_on() {
    let catalog = RegCatalog::new();
    let before = catalog.generation();
    assert_eq!(catalog.prune(&DeclarationStore::default()), Ok(()));
    assert_ne!(catalog.generation(), before);
}

#[test]
fn generation_wrap_skips_zero() {
    let catalog = RegCatalog::new();
    catalog.generation.set(u32::MAX);
    assert_eq!(catalog.prepare().commit(), 1);
    catalog.generation.set(u32::MAX);
    assert_eq!(catalog.prune(&DeclarationStore::default()), Ok(()));
    assert_eq!(catalog.generation(), 1);
}
