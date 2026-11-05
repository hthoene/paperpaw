//! A damaged stream must not gain ownership of another file.
use super::*;

#[test]
fn a_corrupt_stream_cannot_become_a_live_crosslink() {
    let mut image = prepared(|s| {
        s.write("damaged.txt", b"old!").unwrap();
        s.write("kept.txt", b"safe").unwrap();
    });
    let bad = set_offset(&image, "damaged.txt");
    let kept = set_offset(&image, "kept.txt");
    let victim_first = image[kept + 52..kept + 56].to_vec();
    image[bad + 52..bad + 56].copy_from_slice(&victim_first);
    // Do NOT reseal: this is a corrupt stream with an invalid checksum.
    let bytes = card_of(&image);
    let card = mount(&bytes);
    let listed = names(&card, "");
    eprintln!(
        "listed after corrupt stream: {listed:?}, left={}",
        card.left_alone()
    );
    assert_eq!(whole(&card, "kept.txt"), Ok(b"safe".to_vec()));
    if listed.contains(&"damaged.txt".to_string()) {
        card.remove("damaged.txt").unwrap();
        card.sync().unwrap();
        drop(card);
        let card = mount(&bytes);
        card.write("overwrite.txt", &[b'Y'; 4096]).unwrap();
        card.sync().unwrap();
        assert_eq!(
            whole(&card, "kept.txt"),
            Ok(b"safe".to_vec()),
            "sweep promoted a corrupt crosslink and removal freed the victim"
        );
    }
}
