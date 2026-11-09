use super::*;
use crate::native::declare::records_wire::Record;

fn host_and_lib<'a>(table: &'a PpHost, state: &mut State) -> (Host<'a>, Library) {
    let host = Host {
        table,
        ctx: core::ptr::from_mut(state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    (host, lib)
}

#[test]
fn provide_sends_the_book_and_only_the_fields_set_with_their_flags() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 4096], [0; 64]);
    let book = BookRef::path("/books/a.epub").unwrap();
    reply(&state, &[]);
    let provided = Provided::new()
        .title("Dune")
        .unwrap()
        .series("Dune Chronicles")
        .unwrap()
        .number("1")
        .unwrap()
        .tags("scifi\nclassic")
        .unwrap();
    lib.provide(&host, book, &provided, &mut input, &mut out)
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_PROVIDE);
    {
        let bytes = state.input.borrow();
        let rows = wire::records_decode(&bytes).unwrap();
        let mut it = rows.iter();
        assert!(matches!(it.next(), Some(Record::ServiceLibRef(r)) if r.path == "/books/a.epub"));
        assert!(matches!(
            it.next(),
            Some(Record::ServiceLibProvided(p))
                if (p.title, p.author, p.series, p.number, p.tags, p.flags)
                    == ("Dune", "", "Dune Chronicles", "1", "scifi\nclassic", 0b11101)
        ));
        assert!(it.next().is_none());
    }
    // An author alone: flag bit 1, the rest left alone.
    let author = Provided::new().author("Frank Herbert").unwrap();
    lib.provide(&host, book, &author, &mut input, &mut out)
        .unwrap();
    let bytes = state.input.borrow();
    let rows = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        rows.iter().nth(1),
        Some(Record::ServiceLibProvided(p)) if p.flags == 0b10 && p.author == "Frank Herbert"
    ));
}

#[test]
fn provided_fields_are_checked_before_the_host_and_an_empty_one_sends_nothing() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 4096], [0; 64]);
    let book = BookRef::path("/books/a.epub").unwrap();
    assert!(Provided::new().is_empty());
    assert_eq!(
        lib.provide(&host, book, &Provided::new(), &mut input, &mut out),
        Err(Error::Invalid)
    );
    assert_eq!(state.calls.get(), 0);
    // The caps of the record, at the limit and one over.
    assert!(Provided::new().title(&"t".repeat(512)).is_ok());
    assert_eq!(Provided::new().title(&"t".repeat(513)), Err(Error::NoSpace));
    assert!(Provided::new().author(&"a".repeat(512)).is_ok());
    assert_eq!(
        Provided::new().author(&"a".repeat(513)),
        Err(Error::NoSpace)
    );
    assert!(Provided::new().series(&"s".repeat(256)).is_ok());
    assert_eq!(
        Provided::new().series(&"s".repeat(257)),
        Err(Error::NoSpace)
    );
    assert!(Provided::new().number(&"1".repeat(32)).is_ok());
    assert_eq!(Provided::new().number(&"1".repeat(33)), Err(Error::NoSpace));
    // Tags: 32 of 1-128 bytes; more, longer, or an empty one is refused.
    let thirty_two = vec!["t"; 32].join("\n");
    assert!(Provided::new().tags(&thirty_two).is_ok());
    let thirty_three = vec!["t"; 33].join("\n");
    assert_eq!(Provided::new().tags(&thirty_three), Err(Error::NoSpace));
    assert!(Provided::new().tags(&"g".repeat(128)).is_ok());
    assert_eq!(Provided::new().tags(&"g".repeat(129)), Err(Error::NoSpace));
    assert_eq!(Provided::new().tags("a\n\nb"), Err(Error::Invalid));
    assert_eq!(Provided::new().tags("a\n"), Err(Error::Invalid));
    assert_eq!(
        Provided::new().tags(&vec!["g".repeat(100); 21].join("\n")),
        Err(Error::NoSpace),
        "2,100 bytes in all"
    );
    // Empty or blank never clears: it is refused, field by field.
    for blank in ["", " ", "\t\n"] {
        assert_eq!(
            Provided::new().title(blank),
            Err(Error::Invalid),
            "{blank:?}"
        );
        assert_eq!(Provided::new().author(blank), Err(Error::Invalid));
        assert_eq!(Provided::new().series(blank), Err(Error::Invalid));
        assert_eq!(Provided::new().number(blank), Err(Error::Invalid));
    }
    assert_eq!(Provided::new().tags(""), Err(Error::Invalid));
    assert_eq!(Provided::new().tags("a\n \nb"), Err(Error::Invalid));
    assert!(Provided::new().title(" Dune ").is_ok());
}

#[test]
fn the_hosts_refusals_and_a_reply_that_is_not_empty_come_back() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 4096], [0; 64]);
    let book = BookRef::path("/books/a.epub").unwrap();
    let provided = Provided::new().title("Dune").unwrap();
    for (status, error) in [
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_DENIED, Error::Denied),
        (abi::ERR_STATE, Error::State),
    ] {
        state.error.set(status);
        assert_eq!(
            lib.provide(&host, book, &provided, &mut input, &mut out),
            Err(error)
        );
    }
    state.error.set(abi::OK);
    reply(&state, &[]);
    assert_eq!(
        lib.provide(&host, book, &provided, &mut input, &mut out),
        Ok(())
    );
    state.reply.replace(vec![0]);
    assert_eq!(
        lib.provide(&host, book, &provided, &mut input, &mut out),
        Err(Error::Invalid)
    );
}
