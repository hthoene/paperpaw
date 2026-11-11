//! The preparing loop through the kernel's pump (`doc_pump`): the production
//! document host wired into the kernel's host services, a format plugin and
//! an opener plugin in the kernel's instance registry. The format answers op 3
//! `prepare` in its own call (the registry's route, metered to it), and the
//! opener hears of the change from the service pump.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use crate::native_host::{
    doc_service::{PREPARING, READY},
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
};
use paperpaw_native::host_glue::ERR_INVALID;

/// What the plugins of the kernel saw: the format's calls (op, document) and
/// the opener's `event.document_changed` (document, state, progress).
#[derive(Default)]
struct Log {
    calls: RefCell<Vec<(u32, u32)>>,
    heard: RefCell<Vec<(u32, u32, u32)>>,
    /// What the format answers to its next call.
    reply: RefCell<Vec<u8>>,
}

/// A plugin of the kernel: a format (its `core.format` token answers what
/// the log says) or an opener (subscribed to `event.document_changed`).
struct Probe(Rc<Log>);

impl NativeInstance for Probe {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        tag == wire::EVENT_DOCUMENT_CHANGED_TAG
    }
    fn service_event(&self, bytes: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        match wire::records_decode_single(bytes)? {
            Record::EventDocumentChanged(e) => {
                let heard = (e.document, e.state, e.progress_ppm);
                self.0.heard.borrow_mut().push(heard);
                Ok(0)
            }
            _ => Err(ERR_INVALID),
        }
    }
}

impl Dispatch for Probe {
    fn event(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn dropped(&self, _: u32, _: i32) {}
    fn stage(&self, token: u32, bytes: &[u8], out: &mut [u8], _: &Env<'_>) -> Result<i32, i32> {
        assert_eq!(token, 5, "the token of the format's declaration");
        let Record::StageCall(call) = wire::records_decode_single(bytes).unwrap() else {
            panic!("not a stage.call")
        };
        let Some(Ok(local::Record::FmtCall(call))) =
            local::input_decode(call.payload).unwrap().next()
        else {
            panic!("not a fmt.call")
        };
        self.0.calls.borrow_mut().push((call.op, call.document));
        let reply = self.0.reply.borrow();
        out[..reply.len()].copy_from_slice(&reply);
        Ok(i32::try_from(reply.len()).unwrap())
    }
}

fn fmt_state(state: u32, progress: u32) -> Vec<u8> {
    let mut bytes = vec![0_u8; 3840];
    let record = local::FmtState::new(0, state, progress, 0, 0, 5, "epub.epub");
    let n = local::reply_encode(&[local::Record::FmtState(record)], &mut bytes).unwrap();
    bytes.truncate(n);
    bytes
}

/// A kernel with a format plugin and an opener plugin in its registry and the
/// world's document host wired into its host services; the format's
/// declaration names the format plugin's own key.
fn kernel(w: &World<'_>, log: &Rc<Log>) -> (crate::Kernel, u64) {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let mut insert = |id: &'static str| {
        let lease = NativeLease::try_new(Probe(log.clone())).unwrap();
        kernel
            .shared
            .native_instances
            .test_insert(Id::new_static(id), Some(lease), true)
            .unwrap()
    };
    let format_key = insert("epub");
    let opener = insert("opener");
    let hosts = kernel.shared.native_instances.hosts().unwrap();
    hosts.wire_documents(w.state.clone()).unwrap();
    w.install("epub", format_key, &[format("epub", "epub", 0, 0)]);
    (kernel, opener)
}

/// The opener opens `path` through the kernel's own `core.document`; the
/// format answers the open (through the world's frame) with "preparing".
fn open(w: &World<'_>, kernel: &crate::Kernel, opener: u64, path: &str) -> u32 {
    *w.reply.borrow_mut() = Ok(fmt_state(PREPARING, 0));
    let op = services_wire::service_schema("core.document", 1)
        .unwrap()
        .op(1)
        .unwrap();
    let mut input = vec![0; 4096];
    let open = Record::ServiceDocOpen(wire::ServiceDocOpen {
        root: 1,
        path,
        format: "",
        flags: 0,
        image_side: 0,
        cover_side: 0,
        css_root: 0,
        css_path: "",
    });
    let rows = usize::from(op.input_rows);
    let used = wire::records_encode_counted(&[open], rows, op.max_input, &mut input).unwrap();
    let caller = Caller {
        owner: opener,
        plugin: "opener",
        features: AdmittedFeatures::test(1, FEATURES),
        hosts: kernel.shared.native_instances.hosts().unwrap(),
    };
    let mut out = vec![0; op.max_reply];
    let length = doc_service::backend(w, &caller, op, &input[..used], &mut out).unwrap();
    match wire::record_decode_one(&out[..length]).unwrap() {
        Record::ServiceDocState(state) => {
            assert_eq!(state.state, PREPARING);
            state.document
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_kernels_pump_steps_a_preparing_book_through_its_format_until_ready_and_tells_the_opener() {
    world(|w| {
        let log = Rc::new(Log::default());
        let (mut kernel, opener) = kernel(w, &log);
        assert!(!kernel.native_doc_pump(), "nothing prepares yet");
        let document = open(w, &kernel, opener, "books/a.epub");
        assert!(log.calls.borrow().is_empty(), "the open ran in the frame");
        *log.reply.borrow_mut() = fmt_state(PREPARING, 500_000);
        assert!(kernel.native_doc_pump(), "a step was made: more to come");
        *log.reply.borrow_mut() = fmt_state(READY, 1_000_000);
        assert!(kernel.native_doc_pump());
        assert!(!kernel.native_doc_pump(), "a ready book is not stepped");
        // Two op 3 calls of the format plugin's own, metered to it, each
        // outside every callback and none left open.
        assert_eq!(
            log.calls.borrow().as_slice(),
            [(3, document), (3, document)]
        );
        assert_eq!(kernel.shared.meter.calls_of("epub"), 2);
        assert_eq!(kernel.shared.native_instances.depth(), 0);
        assert!(!w.state.preparing());
        // The service pump tells the opener once, with the latest values.
        assert!(log.heard.borrow().is_empty());
        kernel.native_service_pump();
        assert_eq!(
            log.heard.borrow().as_slice(),
            [(document, READY, 1_000_000)]
        );
    });
}

#[test]
fn inside_a_callback_the_kernels_pump_calls_no_format_and_steps_once_it_returned() {
    world(|w| {
        let log = Rc::new(Log::default());
        let (mut kernel, opener) = kernel(w, &log);
        let document = open(w, &kernel, opener, "books/a.epub");
        *log.reply.borrow_mut() = fmt_state(PREPARING, 500_000);
        kernel.shared.native_instances.test_enter_callback();
        assert!(!kernel.native_doc_pump());
        kernel.shared.native_instances.test_leave_callback();
        assert!(log.calls.borrow().is_empty(), "the format was not entered");
        assert_eq!(kernel.shared.native_instances.doc_retry(), None, "no wait");
        assert!(kernel.native_doc_pump());
        assert_eq!(log.calls.borrow().as_slice(), [(3, document)]);
        kernel.native_service_pump();
        assert_eq!(
            log.heard.borrow().as_slice(),
            [(document, PREPARING, 500_000)]
        );
    });
}

#[test]
fn the_cover_job_goes_on_by_timed_wakes_on_an_idle_device_and_holds_no_timer_off() {
    world(|w| {
        let log = Rc::new(Log::default());
        let (mut kernel, opener) = kernel(w, &log);
        let document = open(w, &kernel, opener, "books/a.epub");
        assert!(w.state.table.start_cover_job(opener, document));
        kernel.now = Instant(10_000);
        let asleep_at = kernel.sleep_due();
        *log.reply.borrow_mut() = fmt_state(PREPARING, 100_000);
        // A step of the job: no "more" (the pump does not ask for a pass at
        // once), but a timed wake 100 ms on.
        assert!(!kernel.native_doc_pump());
        assert_eq!(log.calls.borrow().len(), 1);
        assert_eq!(kernel.shared.native_instances.doc_retry(), Some(10_100));
        assert!(!kernel.shared.native_instances.service_more());
        // The shell is woken at the retry at the latest (the opener's event
        // for the step may ask earlier).
        assert!(kernel.service_due().is_some_and(|at| at.millis() <= 10_100));
        // Before then no step; at the wake the next one.
        kernel.now = Instant(10_099);
        assert!(!kernel.native_doc_pump());
        assert_eq!(log.calls.borrow().len(), 1);
        kernel.now = Instant(10_100);
        assert!(!kernel.native_doc_pump());
        assert_eq!(log.calls.borrow().len(), 2);
        assert_eq!(kernel.shared.native_instances.doc_retry(), Some(10_200));
        // The job is no activity and holds no sleep off: the sleep is due
        // when it was, and nothing keeps it from happening.
        assert_eq!(kernel.sleep_due(), asleep_at);
        assert!(!kernel.busy_at_sleep(Instant(10_200)));
        *log.reply.borrow_mut() = fmt_state(READY, 1_000_000);
        kernel.now = Instant(10_200);
        // Ready: the step that finished it asks for the next pass (the end
        // is looked at), and no wait is left.
        assert!(kernel.native_doc_pump());
        assert_eq!(kernel.shared.native_instances.doc_retry(), None);
    });
}

#[test]
fn a_book_that_is_not_the_cover_job_is_stepped_at_once_again() {
    world(|w| {
        let log = Rc::new(Log::default());
        let (mut kernel, opener) = kernel(w, &log);
        open(w, &kernel, opener, "books/a.epub");
        kernel.now = Instant(10_000);
        *log.reply.borrow_mut() = fmt_state(PREPARING, 100_000);
        assert!(kernel.native_doc_pump(), "more to come");
        assert_eq!(kernel.shared.native_instances.doc_retry(), None);
        assert!(kernel.native_doc_pump(), "the next step at once");
        assert_eq!(log.calls.borrow().len(), 2);
    });
}

#[test]
fn the_cover_job_waits_ten_times_longer_where_the_device_saves_power() {
    use paperpaw_sdk::power::PowerLevel;
    for (level, gap) in [
        (PowerLevel::Normal, 100),
        (PowerLevel::Saving, 1_000),
        (PowerLevel::Offline, 1_000),
    ] {
        world(|w| {
            let log = Rc::new(Log::default());
            let (mut kernel, opener) = kernel(w, &log);
            let document = open(w, &kernel, opener, "books/a.epub");
            assert!(w.state.table.start_cover_job(opener, document));
            kernel.shared.power.level = level;
            kernel.now = Instant(10_000);
            *log.reply.borrow_mut() = fmt_state(PREPARING, 100_000);
            assert!(!kernel.native_doc_pump());
            assert_eq!(
                kernel.shared.native_instances.doc_retry(),
                Some(10_000 + gap),
                "{level:?}"
            );
        });
    }
}
