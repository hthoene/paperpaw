//! CalDAV over WebDAV: the bodies of the requests, a multistatus answer read
//! as it streams, addresses and ETags.
use alloc::{string::String, vec::Vec};

use crate::{Error, push_str, xml};

/// The content type of the request bodies here.
pub const XML: &str = "application/xml; charset=utf-8";
/// The content type of a calendar text.
pub const CALENDAR: &str = "text/calendar; charset=utf-8";

/// `PROPFIND` (with `Depth: 1`) for the calendars at an address: what each
/// is, its name, its change tag and what it holds, and where the account's
/// calendars are when the address is not theirs yet.
pub const PROPFIND_CALENDARS: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<d:propfind xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\" \
xmlns:cs=\"http://calendarserver.org/ns/\"><d:prop><d:resourcetype/><d:displayname/>\
<cs:getctag/><c:supported-calendar-component-set/><c:calendar-home-set/>\
<d:current-user-principal/></d:prop></d:propfind>";

/// `REPORT` (with `Depth: 1`) for every to-do of a calendar, with its ETag
/// and its text.
pub const REPORT_TODOS: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<c:calendar-query xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\">\
<d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter>\
<c:comp-filter name=\"VCALENDAR\"><c:comp-filter name=\"VTODO\"/></c:comp-filter>\
</c:filter></c:calendar-query>";

/// `PROPFIND` (with `Depth: 1`) for the address books at an address, as
/// [`PROPFIND_CALENDARS`] asks for calendars.
pub const PROPFIND_BOOKS: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<d:propfind xmlns:d=\"DAV:\" xmlns:card=\"urn:ietf:params:xml:ns:carddav\" \
xmlns:cs=\"http://calendarserver.org/ns/\"><d:prop><d:resourcetype/><d:displayname/>\
<cs:getctag/><card:addressbook-home-set/><d:current-user-principal/></d:prop></d:propfind>";

/// `REPORT` (with `Depth: 1`) for the contacts of an address book that have a
/// birthday or an anniversary, with their ETag and the lines that matter.
pub const REPORT_CONTACTS: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<card:addressbook-query xmlns:d=\"DAV:\" xmlns:card=\"urn:ietf:params:xml:ns:carddav\">\
<d:prop><d:getetag/><card:address-data><card:prop name=\"UID\"/><card:prop name=\"FN\"/>\
<card:prop name=\"N\"/><card:prop name=\"BDAY\"/><card:prop name=\"ANNIVERSARY\"/>\
<card:prop name=\"X-ANNIVERSARY\"/><card:prop name=\"X-APPLE-OMIT-YEAR\"/></card:address-data></d:prop>\
<card:filter test=\"anyof\"><card:prop-filter name=\"BDAY\"/>\
<card:prop-filter name=\"ANNIVERSARY\"/></card:filter></card:addressbook-query>";

/// `REPORT` for the events of a calendar that fall (also in part, and by
/// their repeats) from day `from` to day `to` (counted from 1970-01-01).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn report_events(from: i32, to: i32) -> Result<String, Error> {
    let mut body = String::new();
    push_str(
        &mut body,
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<c:calendar-query xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\">\
<d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter>\
<c:comp-filter name=\"VCALENDAR\"><c:comp-filter name=\"VEVENT\"><c:time-range start=\"",
    )?;
    let mut time = String::new();
    crate::date::DateTime::date(from).write(&mut time)?;
    push_str(&mut body, &time)?;
    push_str(&mut body, "T000000Z\" end=\"")?;
    time.clear();
    crate::date::DateTime::date(to).write(&mut time)?;
    push_str(&mut body, &time)?;
    push_str(
        &mut body,
        "T000000Z\"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>",
    )?;
    Ok(body)
}

/// The most bytes kept of an address in an answer.
pub const HREF_MAX: usize = 512;
/// The most bytes kept of an ETag, a change tag or a name.
pub const FIELD_MAX: usize = 160;

/// One `response` of a multistatus answer, told when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item<'a> {
    /// Its address, as the server wrote it (a path, or a whole address).
    pub href: &'a str,
    /// Its ETag, as written.
    pub etag: &'a str,
    /// Its name for people.
    pub name: &'a str,
    /// The collection's change tag (`getctag`).
    pub ctag: &'a str,
    /// Where the account's calendars are (`calendar-home-set`).
    pub home: &'a str,
    /// The account's own address (`current-user-principal`).
    pub principal: &'a str,
    /// It is a calendar collection.
    pub calendar: bool,
    /// It is an address book.
    pub addressbook: bool,
    /// It holds to-dos (it says so, or does not say what it holds).
    pub todos: bool,
    /// It holds events (as above).
    pub events: bool,
    /// The status of the response itself (404 for something gone), 0 if none.
    pub status: u16,
    /// A field was longer than is kept: do not rely on this one.
    pub cut: bool,
}

/// Who is told what a multistatus answer holds.
pub trait Visit {
    /// What the visitor may refuse with.
    type Error: From<Error>;
    /// A piece of the calendar text of the response being read.
    ///
    /// # Errors
    /// The visitor's own.
    fn data(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
    /// A response has ended.
    ///
    /// # Errors
    /// The visitor's own.
    fn item(&mut self, item: &Item<'_>) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Leaf {
    #[default]
    None,
    Href,
    Etag,
    Name,
    Ctag,
    Home,
    Principal,
    Status,
    Data,
}

/// Reads a multistatus answer from what [`xml::Scanner`] tells it.
#[derive(Debug)]
pub struct Multistatus<V> {
    /// The visitor.
    pub visit: V,
    leaf: Leaf,
    href: Vec<u8>,
    etag: Vec<u8>,
    name: Vec<u8>,
    ctag: Vec<u8>,
    home: Vec<u8>,
    principal: Vec<u8>,
    status: Vec<u8>,
    in_response: bool,
    in_propstat: bool,
    in_prop: bool,
    in_type: bool,
    in_home: bool,
    in_principal: bool,
    in_set: bool,
    calendar: bool,
    book: bool,
    comps: bool,
    todos: bool,
    events: bool,
    cut: bool,
    complete: bool,
}

fn text(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap_or("").trim()
}

impl<V: Visit> Multistatus<V> {
    /// A reader that tells `visit`.
    #[must_use]
    pub const fn new(visit: V) -> Self {
        Self {
            visit,
            leaf: Leaf::None,
            href: Vec::new(),
            etag: Vec::new(),
            name: Vec::new(),
            ctag: Vec::new(),
            home: Vec::new(),
            principal: Vec::new(),
            status: Vec::new(),
            in_response: false,
            in_propstat: false,
            in_prop: false,
            in_type: false,
            in_home: false,
            in_principal: false,
            in_set: false,
            calendar: false,
            book: false,
            comps: false,
            todos: false,
            events: false,
            cut: false,
            complete: false,
        }
    }

    /// Whether the answer was read to its end (`</multistatus>`): only then
    /// is what it did not name not there.
    #[must_use]
    pub const fn complete(&self) -> bool {
        self.complete
    }

    fn reset(&mut self) {
        for field in [
            &mut self.href,
            &mut self.etag,
            &mut self.name,
            &mut self.ctag,
            &mut self.home,
            &mut self.principal,
            &mut self.status,
        ] {
            field.clear();
        }
        (self.calendar, self.comps, self.todos, self.events) = (false, false, false, false);
        self.book = false;
        self.cut = false;
    }

    fn keep(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let (field, max) = match self.leaf {
            Leaf::Href => (&mut self.href, HREF_MAX),
            Leaf::Home => (&mut self.home, HREF_MAX),
            Leaf::Principal => (&mut self.principal, HREF_MAX),
            Leaf::Etag => (&mut self.etag, FIELD_MAX),
            Leaf::Name => (&mut self.name, FIELD_MAX),
            Leaf::Ctag => (&mut self.ctag, FIELD_MAX),
            Leaf::Status => (&mut self.status, 64),
            Leaf::None | Leaf::Data => return Ok(()),
        };
        if field.len() + bytes.len() > max {
            self.cut = true;
            return Ok(());
        }
        field
            .try_reserve(bytes.len())
            .map_err(|_| Error::NoMemory)?;
        field.extend_from_slice(bytes);
        Ok(())
    }

    fn ended(&mut self) -> Result<(), V::Error> {
        let status = text(&self.status)
            .split_ascii_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        let item = Item {
            href: text(&self.href),
            etag: text(&self.etag),
            name: text(&self.name),
            ctag: text(&self.ctag),
            home: text(&self.home),
            principal: text(&self.principal),
            calendar: self.calendar,
            addressbook: self.book,
            todos: self.todos || !self.comps,
            events: self.events || !self.comps,
            status,
            cut: self.cut,
        };
        self.visit.item(&item)
    }
}

impl<V: Visit> xml::Sink for Multistatus<V> {
    type Error = V::Error;

    fn open(&mut self, name: &str, attrs: &str) -> Result<(), V::Error> {
        self.leaf = Leaf::None;
        match name {
            "response" => {
                self.reset();
                self.in_response = true;
            }
            "propstat" => self.in_propstat = true,
            "prop" => self.in_prop = true,
            "resourcetype" => self.in_type = true,
            "calendar" if self.in_type => self.calendar = true,
            "addressbook" if self.in_type => self.book = true,
            "calendar-home-set" | "addressbook-home-set" => self.in_home = true,
            "current-user-principal" => self.in_principal = true,
            "supported-calendar-component-set" => self.in_set = true,
            "comp" if self.in_set => {
                self.comps = true;
                self.todos |= attrs.contains("VTODO");
                self.events |= attrs.contains("VEVENT");
            }
            "href" if self.in_home => self.leaf = Leaf::Home,
            "href" if self.in_principal => self.leaf = Leaf::Principal,
            "href" if self.in_response && !self.in_prop => self.leaf = Leaf::Href,
            "status" if self.in_response && !self.in_propstat => self.leaf = Leaf::Status,
            "getetag" => self.leaf = Leaf::Etag,
            "displayname" => self.leaf = Leaf::Name,
            "getctag" => self.leaf = Leaf::Ctag,
            "calendar-data" | "address-data" => self.leaf = Leaf::Data,
            _ => {}
        }
        Ok(())
    }

    fn text(&mut self, bytes: &[u8]) -> Result<(), V::Error> {
        if self.leaf == Leaf::Data {
            return self.visit.data(bytes);
        }
        Ok(self.keep(bytes)?)
    }

    fn close(&mut self, name: &str) -> Result<(), V::Error> {
        self.leaf = Leaf::None;
        match name {
            "response" if self.in_response => {
                self.in_response = false;
                self.ended()?;
            }
            "propstat" => self.in_propstat = false,
            "prop" => self.in_prop = false,
            "resourcetype" => self.in_type = false,
            "calendar-home-set" | "addressbook-home-set" => self.in_home = false,
            "current-user-principal" => self.in_principal = false,
            "supported-calendar-component-set" => self.in_set = false,
            "multistatus" => self.complete = true,
            _ => {}
        }
        Ok(())
    }
}

/// The scheme and host of an address (`https://host:8443`), without a path.
#[must_use]
pub fn origin(url: &str) -> &str {
    let Some(scheme) = url.find("://") else {
        return "";
    };
    let after = scheme + 3;
    let end = url
        .get(after..)
        .and_then(|rest| rest.find(['/', '?', '#']))
        .map_or(url.len(), |at| after + at);
    url.get(..end).unwrap_or("")
}

/// The path of an address or of an `href` (which is a path already, or a
/// whole address), without query.
#[must_use]
pub fn path(href: &str) -> &str {
    let rest = href.get(origin(href).len()..).unwrap_or(href);
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    rest.get(..end).unwrap_or(rest)
}

/// The whole address an `href` of an answer from `base` means.
///
/// # Errors
/// [`Error::NoMemory`]; [`Error::Invalid`] for a `base` that is no address.
pub fn resolve(base: &str, href: &str) -> Result<String, Error> {
    let mut out = String::new();
    if !origin(href).is_empty() {
        push_str(&mut out, href)?;
        return Ok(out);
    }
    let host = origin(base);
    if host.is_empty() {
        return Err(Error::Invalid);
    }
    push_str(&mut out, host)?;
    if !href.starts_with('/') {
        // Beside what the base names.
        let folder = path(base);
        let end = folder.rfind('/').map_or(0, |at| at + 1);
        push_str(&mut out, folder.get(..end).unwrap_or(""))?;
        if !out.ends_with('/') {
            push_str(&mut out, "/")?;
        }
    }
    push_str(&mut out, href)?;
    Ok(out)
}

/// The address of `name` in the collection at `collection`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn child(collection: &str, name: &str) -> Result<String, Error> {
    let mut out = String::new();
    push_str(&mut out, collection.trim_end_matches('/'))?;
    push_str(&mut out, "/")?;
    push_str(&mut out, name)?;
    Ok(out)
}

/// The bytes a path stands for: `%41` is `A`.
fn decoded(path: &str) -> impl Iterator<Item = u8> + '_ {
    let bytes = path.as_bytes();
    let mut at = 0;
    core::iter::from_fn(move || {
        let byte = *bytes.get(at)?;
        at += 1;
        if byte != b'%' {
            return Some(byte);
        }
        let hex = |b: Option<&u8>| b.and_then(|b| char::from(*b).to_digit(16));
        match (hex(bytes.get(at)), hex(bytes.get(at + 1))) {
            (Some(high), Some(low)) => {
                at += 2;
                u8::try_from(high * 16 + low).ok()
            }
            _ => Some(byte),
        }
    })
}

/// Whether two addresses or `href`s name the same thing on one server: the
/// paths are compared as what they stand for, with or without a `/` at the
/// end.
#[must_use]
pub fn same_path(a: &str, b: &str) -> bool {
    let (a, b) = (path(a).trim_end_matches('/'), path(b).trim_end_matches('/'));
    decoded(a).eq(decoded(b))
}

/// An ETag without what is around it: `W/"abc"` and `"abc"` are `abc`.
#[must_use]
pub fn etag_value(etag: &str) -> &str {
    let etag = etag.trim();
    let etag = etag.strip_prefix("W/").unwrap_or(etag);
    etag.strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(etag)
}

/// Whether two ETags are the same version. An unknown one (empty) is none.
#[must_use]
pub fn etag_eq(a: &str, b: &str) -> bool {
    let (a, b) = (etag_value(a), etag_value(b));
    !a.is_empty() && a == b
}

/// An ETag as `If-Match` sends it: in quotes.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn etag_header(etag: &str) -> Result<String, Error> {
    let mut out = String::new();
    push_str(&mut out, "\"")?;
    push_str(&mut out, etag_value(etag))?;
    push_str(&mut out, "\"")?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, string::ToString};

    #[derive(Default)]
    struct Seen {
        items: Vec<String>,
        data: Vec<u8>,
    }

    impl Visit for Seen {
        type Error = Error;
        fn data(&mut self, bytes: &[u8]) -> Result<(), Error> {
            self.data.extend_from_slice(bytes);
            Ok(())
        }
        fn item(&mut self, item: &Item<'_>) -> Result<(), Error> {
            let data = String::from_utf8_lossy(&core::mem::take(&mut self.data)).to_string();
            self.items.push(format!(
                "{} etag={} name={} ctag={} home={} principal={} cal={} todos={} events={} status={} cut={} data={data}",
                item.href, item.etag, item.name, item.ctag, item.home, item.principal,
                item.calendar, item.todos, item.events, item.status, item.cut
            ));
            Ok(())
        }
    }

    fn read(text: &str, piece: usize) -> (Vec<String>, bool) {
        let mut scanner = xml::Scanner::new();
        let mut reader = Multistatus::new(Seen::default());
        for part in text.as_bytes().chunks(piece.max(1)) {
            scanner.feed(part, &mut reader).unwrap();
        }
        (core::mem::take(&mut reader.visit.items), reader.complete())
    }

    const HOME: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/">
 <d:response><d:href>/dav/calendars/anna/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype>
   <d:current-user-principal><d:href>/dav/principals/users/anna/</d:href></d:current-user-principal></d:prop>
   <d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  <d:propstat><d:prop><d:displayname/><cs:getctag/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>
 </d:response>
 <d:response><d:href>/dav/calendars/anna/tasks/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
   <d:displayname>Haushalt &amp; Garten</d:displayname><cs:getctag>"ct-7"</cs:getctag>
   <cal:supported-calendar-component-set><cal:comp name="VTODO"/></cal:supported-calendar-component-set>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
 </d:response>
 <d:response><d:href>/dav/calendars/anna/personal/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
   <d:displayname>Personal</d:displayname>
   <cal:supported-calendar-component-set><cal:comp name="VEVENT"/></cal:supported-calendar-component-set>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
 </d:response>
</d:multistatus>"#;

    #[test]
    fn calendars_are_told_with_what_they_hold() {
        let (items, complete) = read(HOME, HOME.len());
        assert!(complete);
        assert_eq!(items.len(), 3);
        assert_eq!(
            items[0],
            "/dav/calendars/anna/ etag= name= ctag= home= principal=/dav/principals/users/anna/ cal=false todos=true events=true status=0 cut=false data="
        );
        assert_eq!(
            items[1],
            "/dav/calendars/anna/tasks/ etag= name=Haushalt & Garten ctag=\"ct-7\" home= principal= cal=true todos=true events=false status=0 cut=false data="
        );
        assert!(items[2].contains("cal=true todos=false events=true"));
        for piece in [1, 2, 5, 13, 64] {
            assert_eq!(read(HOME, piece).0, items, "pieces of {piece}");
        }
    }

    #[test]
    fn todos_come_with_etag_and_text_and_an_answer_cut_short_is_not_complete() {
        let report = "<multistatus xmlns=\"DAV:\"><response><href>/c/a.ics</href><propstat><prop>\
<getetag>&quot;e1&quot;</getetag><C:calendar-data xmlns:C=\"urn:ietf:params:xml:ns:caldav\">BEGIN:VTODO&#13;\nSUMMARY:a &lt; b\nEND:VTODO\n</C:calendar-data>\
</prop><status>HTTP/1.1 200 OK</status></propstat></response>\
<response><href>/c/gone.ics</href><status>HTTP/1.1 404 Not Found</status></response></multistatus>";
        let (items, complete) = read(report, 7);
        assert!(complete);
        assert_eq!(
            items[0],
            "/c/a.ics etag=\"e1\" name= ctag= home= principal= cal=false todos=true events=true status=0 cut=false data=BEGIN:VTODO\r\nSUMMARY:a < b\nEND:VTODO\n"
        );
        assert!(items[1].starts_with("/c/gone.ics ") && items[1].contains("status=404"));
        let cut = report.get(..report.len() - 30).unwrap();
        let (items, complete) = read(cut, 7);
        assert_eq!(items.len(), 1);
        assert!(!complete);
    }

    #[test]
    fn a_field_too_long_is_marked() {
        let long = "x".repeat(HREF_MAX + 1);
        let text = format!("<multistatus><response><href>/{long}</href></response></multistatus>");
        let (items, _) = read(&text, 100);
        assert!(items[0].contains("cut=true"));
    }

    #[test]
    fn addresses() {
        assert_eq!(
            origin("https://cloud.example:8443/dav/x?y"),
            "https://cloud.example:8443"
        );
        assert_eq!(origin("/dav/x"), "");
        assert_eq!(path("https://h/dav/a%20b/?x#y"), "/dav/a%20b/");
        assert_eq!(path("/dav/c/"), "/dav/c/");
        let base = "https://h/dav/home/";
        assert_eq!(resolve(base, "/dav/c/").unwrap(), "https://h/dav/c/");
        assert_eq!(resolve(base, "http://other/x").unwrap(), "http://other/x");
        assert_eq!(resolve(base, "c/").unwrap(), "https://h/dav/home/c/");
        assert_eq!(resolve("https://h", "c/").unwrap(), "https://h/c/");
        assert_eq!(resolve("no address", "/x"), Err(Error::Invalid));
        assert_eq!(
            child("https://h/dav/c/", "u.ics").unwrap(),
            "https://h/dav/c/u.ics"
        );
        assert!(same_path("https://h/dav/a%20b/", "/dav/a b"));
        assert!(same_path("/dav/%7Eanna/x.ics", "/dav/~anna/x.ics"));
        assert!(!same_path("/dav/a", "/dav/b"));
        assert!(same_path("/dav/100%/", "/dav/100%"));
    }

    #[test]
    fn etags() {
        assert!(etag_eq("W/\"abc\"", "\"abc\""));
        assert!(etag_eq("abc", "\"abc\""));
        assert!(!etag_eq("\"abc\"", "\"abd\""));
        assert!(!etag_eq("", ""), "an unknown version equals none");
        assert_eq!(etag_header("W/\"abc\"").unwrap(), "\"abc\"");
        assert_eq!(etag_header("abc").unwrap(), "\"abc\"");
    }

    #[test]
    fn an_address_book_and_a_range_of_events() {
        let text = r#"<d:multistatus xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav">
 <d:response><d:href>/dav/abook/</d:href><d:propstat><d:prop>
  <d:resourcetype><d:collection/><card:addressbook/></d:resourcetype>
  <card:addressbook-home-set><d:href>/dav/abook/</d:href></card:addressbook-home-set>
 </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;
        let (items, _) = read(text, 7);
        assert!(items[0].contains("home=/dav/abook/"), "{}", items[0]);
        let mut book = false;
        let mut scanner = xml::Scanner::new();
        struct Flag<'a>(&'a mut bool);
        impl Visit for Flag<'_> {
            type Error = Error;
            fn data(&mut self, _: &[u8]) -> Result<(), Error> {
                Ok(())
            }
            fn item(&mut self, item: &Item<'_>) -> Result<(), Error> {
                *self.0 |= item.addressbook;
                Ok(())
            }
        }
        let mut reader = Multistatus::new(Flag(&mut book));
        scanner.feed(text.as_bytes(), &mut reader).unwrap();
        assert!(book);
        let body = report_events(20_800, 20_860).unwrap();
        assert!(
            body.contains("start=\"20261213T000000Z\" end=\"20270211T000000Z\""),
            "{body}"
        );
    }
}
