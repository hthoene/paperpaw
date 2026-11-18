#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use crate::test_volume::Mem;

/// What Nextcloud answers to a depth-1 `PROPFIND`: the folder itself, a
/// subfolder, photos (one with a space and an umlaut in its name), a file that
/// is none and a hidden one.
pub(crate) const NEXTCLOUD: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:s="http://sabredav.org/ns" xmlns:oc="http://owncloud.org/ns">
 <d:response><d:href>/remote.php/dav/files/me/Photos/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/files/me/Photos/Trip/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/files/me/Photos/beach.JPG</d:href>
  <d:propstat><d:prop><d:resourcetype/><d:getcontentlength>2048</d:getcontentlength></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/files/me/Photos/Sch%C3%B6ne%20Aussicht.png</d:href>
  <d:propstat><d:prop><d:resourcetype/><d:getcontentlength>4096</d:getcontentlength></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/files/me/Photos/notes.txt</d:href>
  <d:propstat><d:prop><d:resourcetype/><d:getcontentlength>5</d:getcontentlength></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/files/me/Photos/.hidden.jpg</d:href>
  <d:propstat><d:prop><d:resourcetype/><d:getcontentlength>5</d:getcontentlength></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/files/me/Photos/unsized.jpeg</d:href>
  <d:propstat><d:prop><d:resourcetype/></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;

const BASE: &str = "https://cloud.example.org/remote.php/dav/files/me/Photos/";

#[test]
fn the_parts_of_an_address_are_told_apart() {
    assert_eq!(origin(BASE), "https://cloud.example.org");
    assert_eq!(origin("http://10.0.0.2:8080/dav/"), "http://10.0.0.2:8080");
    assert_eq!(origin("nonsense"), "");
    assert_eq!(path(BASE), "/remote.php/dav/files/me/Photos/");
    assert_eq!(path("http://host"), "/");
    assert_eq!(path("http://host/a?b=c#d"), "/a");
    assert_eq!(
        resolve(BASE, "/remote.php/dav/files/me/Photos/a.jpg").unwrap(),
        "https://cloud.example.org/remote.php/dav/files/me/Photos/a.jpg"
    );
    assert_eq!(
        resolve(BASE, "b.jpg").unwrap(),
        "https://cloud.example.org/remote.php/dav/files/me/Photos/b.jpg"
    );
    assert_eq!(
        resolve(BASE, "http://other/c.jpg").unwrap(),
        "http://other/c.jpg"
    );
}

#[test]
fn the_photos_of_a_listing_are_read_with_their_sizes_and_sorted() {
    let volume = Mem::default();
    volume.write(LISTING, NEXTCLOUD.as_bytes()).unwrap();
    let (photos, more) = read(&volume, LISTING, BASE).unwrap();
    assert!(!more);
    let got: alloc::vec::Vec<(&str, u64)> = photos
        .iter()
        .map(|p| (p.href.rsplit('/').next().unwrap(), p.size))
        .collect();
    assert_eq!(
        got,
        [
            ("beach.JPG", 2048),
            ("Sch%C3%B6ne%20Aussicht.png", 4096),
            ("unsized.jpeg", 0)
        ]
    );
    assert!(
        photos[0]
            .href
            .starts_with("https://cloud.example.org/remote.php/")
    );
}

#[test]
fn a_listing_read_in_pieces_of_any_size_gives_the_same_photos() {
    // The reader takes 2 KiB at a time: a listing of many photos crosses pieces.
    let mut xml = alloc::string::String::from(r#"<d:multistatus xmlns:d="DAV:">"#);
    for n in 0..120 {
        xml.push_str(&alloc::format!(
            "<d:response><d:href>/dav/p/img{n:03}.jpg</d:href><d:propstat><d:prop><d:resourcetype/><d:getcontentlength>{}</d:getcontentlength></d:prop></d:propstat></d:response>",
            n + 1
        ));
    }
    xml.push_str("</d:multistatus>");
    let volume = Mem::default();
    volume.write(LISTING, xml.as_bytes()).unwrap();
    let (photos, more) = read(&volume, LISTING, "http://h/dav/p/").unwrap();
    assert!(!more);
    assert_eq!(photos.len(), 120);
    assert_eq!(photos[119].size, 120);
}

#[test]
fn more_photos_than_are_kept_say_so() {
    let mut xml = alloc::string::String::from(r#"<d:multistatus xmlns:d="DAV:">"#);
    for n in 0..(MOST + 5) {
        xml.push_str(&alloc::format!(
            "<d:response><d:href>/dav/img{n:04}.png</d:href><d:propstat><d:prop><d:resourcetype/></d:prop></d:propstat></d:response>"
        ));
    }
    xml.push_str("</d:multistatus>");
    let volume = Mem::default();
    volume.write(LISTING, xml.as_bytes()).unwrap();
    let (photos, more) = read(&volume, LISTING, "http://h/dav/").unwrap();
    assert_eq!(photos.len(), MOST);
    assert!(more);
}

#[test]
fn a_missing_listing_is_an_error_not_an_empty_server() {
    assert!(read(&Mem::default(), LISTING, BASE).is_err());
}
