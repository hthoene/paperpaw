use super::*;
use crate::native_catalog::{Font, FontFile};
use alloc::{string::ToString, vec};
use paperpaw_sdk::MemoryStorage;

fn sha1_of(data: &[u8]) -> String {
    let mut sha1 = Sha1::new();
    sha1.update(data);
    paperpaw_sdk::digest::hex::encode(&sha1.finish())
}

fn plan(files: &[(&str, &[u8])]) -> Plan {
    let files = files
        .iter()
        .map(|(path, data)| FontFile {
            path: path.to_string(),
            url: "https://f.example/x".to_string(),
            size: data.len() as u64,
            sha1: sha1_of(data),
        })
        .collect();
    let font = Font {
        id: "fredoka".to_string(),
        name: "Fredoka".to_string(),
        version: "1.0".to_string(),
        files,
        ..Font::default()
    };
    Plan::new(&font, false).unwrap().unwrap()
}

#[test]
fn a_job_goes_file_by_file() {
    let plan = plan(&[("plugin.ini", b"a"), ("fonts/r.ppfp", b"bb")]);
    let mut job = Job::new(plan, "Fredoka".to_string(), false);
    assert_eq!(
        job.current().unwrap().save,
        "staging/font-fredoka/plugin.ini"
    );
    assert!(job.advance());
    assert_eq!(
        job.current().unwrap().save,
        "staging/font-fredoka/fonts/r.ppfp"
    );
    assert!(!job.advance());
    assert!(job.current().is_none());
    assert!(!job.installing());
    job.install_asked();
    assert!(job.installing());
}

#[test]
fn a_file_is_kept_only_if_size_and_digest_are_the_catalogs() {
    let data = vec![7u8; 10_000];
    let plan = plan(&[("plugin.ini", &data)]);
    let step = &plan.steps[0];
    let card = MemoryStorage::new();
    // Not there at all.
    assert_eq!(verify(&card, step), Ok(false));
    card.write(&step.save, &data).unwrap();
    assert_eq!(verify(&card, step), Ok(true));
    // One byte different, and a different size.
    let mut other = data.clone();
    other[9_999] = 8;
    card.write(&step.save, &other).unwrap();
    assert_eq!(verify(&card, step), Ok(false));
    card.write(&step.save, &data[..9_999]).unwrap();
    assert_eq!(verify(&card, step), Ok(false));
}

#[test]
fn clearing_removes_the_staged_files_and_their_folders_and_forgives_missing_ones() {
    let plan = plan(&[("plugin.ini", b"a"), ("fonts/r.ppfp", b"bb")]);
    let card = MemoryStorage::new();
    card.write(&plan.steps[1].save, b"bb").unwrap();
    clear(&card, &plan);
    assert!(card.size(&plan.steps[1].save).is_err());
}

#[test]
fn a_job_knows_when_the_host_was_asked_to_install() {
    let mut job = Job::new(plan(&[("plugin.ini", b"a")]), "Fredoka".to_string(), false);
    assert!(!job.installing());
    job.install_asked();
    assert!(job.installing());
}
