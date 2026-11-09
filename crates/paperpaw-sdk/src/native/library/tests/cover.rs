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
fn image(id: u32, flags: u32) -> Record<'static> {
    Record::ServiceDocImage(wire::ServiceDocImage {
        image: id,
        index: u32::MAX,
        width: 300,
        height: 450,
        flags,
    })
}

#[test]
fn cover_asks_by_reference_and_side_and_reads_the_image() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 1024], [0; 4096]);
    reply(&state, &[image(5, 0b11)]);
    let cover = lib
        .cover(
            &host,
            BookRef::path("/books/a.epub").unwrap(),
            600,
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_COVER);
    assert_eq!(cover.image().get(), 5);
    let info = cover.info();
    assert_eq!(
        (
            info.width(),
            info.height(),
            info.colour(),
            info.not_decoded()
        ),
        (300, 450, true, true)
    );
    let bytes = state.input.borrow();
    let rows = wire::records_decode(&bytes).unwrap();
    let sent: Vec<_> = rows.iter().collect();
    assert!(matches!(&sent[0], Record::ServiceLibRef(r) if r.path == "/books/a.epub"));
    assert!(matches!(&sent[1], Record::ServiceLibCover(c) if c.side == 600));
}

#[test]
fn cover_refusals_and_bad_answers() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 1024], [0; 4096]);
    let book = BookRef::path("/books/a.epub").unwrap();
    for (status, want) in [
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_BUSY, Error::Busy),
        (abi::ERR_STATE, Error::State),
    ] {
        state.error.set(status);
        assert_eq!(lib.cover(&host, book, 0, &mut input, &mut out), Err(want));
    }
    state.error.set(0);
    // An image id of 0, an empty reply and two rows are Invalid.
    reply(&state, &[image(0, 0)]);
    assert_eq!(
        lib.cover(&host, book, 0, &mut input, &mut out),
        Err(Error::Invalid)
    );
    reply(&state, &[]);
    assert_eq!(
        lib.cover(&host, book, 0, &mut input, &mut out),
        Err(Error::Invalid)
    );
    reply(&state, &[image(1, 0), image(2, 0)]);
    assert_eq!(
        lib.cover(&host, book, 0, &mut input, &mut out),
        Err(Error::Invalid)
    );
}

#[test]
fn cover_needs_its_buffers_and_ignores_flag_bits_it_does_not_know() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let book = BookRef::path("/books/a.epub").unwrap();
    let mut out = [0; 4096];
    // An input buffer that cannot hold the request is refused before the host.
    assert!(
        lib.cover(&host, book, 0, &mut [0; 8], &mut out).is_err(),
        "a short input"
    );
    assert_eq!(state.calls.get(), 0);
    // A reply the output cannot carry is the host's refusal, passed through.
    reply(&state, &[image(5, 0)]);
    state.error.set(abi::ERR_NO_SPACE);
    assert_eq!(
        lib.cover(&host, book, 0, &mut [0; 1024], &mut [0; 4]),
        Err(Error::NoSpace)
    );
    state.error.set(0);
    // Bits other than colour (0) and not decoded (1) are dropped.
    reply(&state, &[image(5, 0b1100)]);
    let cover = lib.cover(&host, book, 0, &mut [0; 1024], &mut out).unwrap();
    assert_eq!(
        (cover.info().colour(), cover.info().not_decoded()),
        (false, false)
    );
}
