#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use crate::test_volume::Mem;
use alloc::vec;

fn remote(href: &str, size: u64) -> Remote {
    Remote {
        href: href.into(),
        size,
    }
}

fn entry(href: &str, size: u64) -> Entry {
    Entry {
        name: file_name(href).unwrap(),
        size,
        href: href.into(),
    }
}

#[test]
fn a_photo_is_kept_under_a_name_of_its_address() {
    let a = file_name("https://x.org/dav/Trip/A.JPG").unwrap();
    assert!(
        a.starts_with('p') && a.ends_with(".jpg") && a.len() == 13,
        "{a}"
    );
    assert_eq!(a, file_name("https://x.org/dav/Trip/A.JPG").unwrap());
    assert_ne!(a, file_name("https://x.org/dav/Trip/B.JPG").unwrap());
    assert!(file_name("https://x.org/a.png").unwrap().ends_with(".png"));
    assert!(file_name("https://x.org/a.jpeg").unwrap().ends_with(".jpg"));
    assert_eq!(file_name("https://x.org/a.gif"), None);
}

#[test]
fn the_list_is_written_and_read_back_and_a_bad_line_is_left_out() {
    let volume = Mem::default();
    assert!(read(&volume).is_empty());
    let entries = vec![
        entry("https://x.org/a.jpg", 1234),
        entry("https://x.org/b.png", 99),
    ];
    write(&volume, &entries).unwrap();
    assert_eq!(read(&volume), entries);
    volume
        .write(
            MANIFEST,
            b"nonsense\np1.jpg\t12\thttps://x.org/c.jpg\nq.txt\t1\th\np2.jpg\tx\th\n",
        )
        .unwrap();
    let kept = read(&volume);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].href, "https://x.org/c.jpg");
    assert_eq!(total(&entries), 1333);
}

#[test]
fn what_fits_the_cap_is_wanted_what_is_there_stays_and_the_rest_goes() {
    let server = [
        remote("https://x.org/a.jpg", 400),
        remote("https://x.org/b.jpg", 400),
        remote("https://x.org/c.jpg", 400),
    ];
    let there = [
        entry("https://x.org/a.jpg", 400),
        entry("https://x.org/old.jpg", 10),
    ];
    let plan = plan(&server, &there, 1000, 0).unwrap();
    assert_eq!(plan.photos, 3);
    assert_eq!(plan.fitting, 2);
    assert_eq!(plan.keep, vec![entry("https://x.org/a.jpg", 400)]);
    assert_eq!(plan.fetch.len(), 1);
    assert_eq!(plan.fetch[0].0, 1);
    assert_eq!(plan.drop, vec![file_name("https://x.org/old.jpg").unwrap()]);
    // More than fits: the next sync starts after the last one taken.
    assert_eq!(plan.window, 2);
}

#[test]
fn the_windows_go_round_so_every_photo_comes_up() {
    let server: Vec<Remote> = (0..6)
        .map(|n| remote(&alloc::format!("https://x.org/{n}.jpg"), 300))
        .collect();
    let mut taken = alloc::collections::BTreeSet::new();
    let (mut window, mut there) = (0, Vec::new());
    for _ in 0..3 {
        let plan = plan(&server, &there, 700, window).unwrap();
        assert_eq!(plan.fitting, 2);
        assert!(total(&plan.keep) + plan.fetch.len() as u64 * 300 <= 700);
        for (index, _) in &plan.fetch {
            taken.insert(*index);
        }
        window = plan.window;
        there = plan
            .keep
            .iter()
            .cloned()
            .chain(plan.fetch.iter().map(|(i, _)| entry(&server[*i].href, 300)))
            .collect();
    }
    assert_eq!(taken.len(), 6);
}

#[test]
fn a_photo_that_fits_whole_means_no_window_and_one_too_big_is_left_out() {
    let server = [
        remote("https://x.org/a.jpg", 100),
        remote("https://x.org/huge.jpg", 5000),
    ];
    let plan = plan(&server, &[], 1000, 0).unwrap();
    assert_eq!(plan.fitting, 1);
    // The big one never fits, so the window moves on past the small one.
    assert_eq!(plan.window, 1);
    let all = [
        remote("https://x.org/a.jpg", 100),
        remote("https://x.org/b.png", 100),
    ];
    let plan = super::plan(&all, &[], 1000, 1).unwrap();
    assert_eq!((plan.fitting, plan.window), (2, 0));
}

#[test]
fn a_size_that_changed_is_fetched_again_and_one_not_told_is_counted_as_a_megabyte() {
    let server = [
        remote("https://x.org/a.jpg", 500),
        remote("https://x.org/b.jpg", 0),
    ];
    let there = [
        entry("https://x.org/a.jpg", 400),
        entry("https://x.org/b.jpg", 777),
    ];
    let plan = plan(&server, &there, 3 << 20, 0).unwrap();
    // `a` changed; `b` did not say, so what is there is kept.
    assert_eq!(plan.fetch.len(), 1);
    assert_eq!(plan.keep, vec![entry("https://x.org/b.jpg", 777)]);
    assert!(plan.drop.is_empty());
    let tight = super::plan(&[remote("https://x.org/b.jpg", 0)], &[], UNKNOWN - 1, 0).unwrap();
    assert_eq!(tight.fitting, 0);
}

#[test]
fn an_empty_server_plans_nothing() {
    let plan = plan(&[], &[entry("https://x.org/a.jpg", 1)], 100, 5).unwrap();
    assert_eq!((plan.photos, plan.fitting, plan.window), (0, 0, 0));
    assert_eq!(plan.drop.len(), 1);
}
