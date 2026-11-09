#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The cable's native server in a kernel with `core.net` wired with the cable:
//! the computer's lines reach the server through the kernel, a request without
//! a body waits for the server's start, bodies land in `.received/`, answers
//! go back as lines, and the protocol's rules hold (503, 403, files).
extern crate std;
use super::*;
use crate::Instant;
use crate::native_host::registrations::{self as wire, Record};
use crate::native_host::{
    admitted_features::AdmittedFeatures,
    event_tags::EventTags,
    host_services::{Caller, HostServices},
    shell_wiring::{
        HostWiring, HttpJob, NetInputs, NetLimits, ServedReply, ServerReason, ShellHttp,
        ShellNetwork,
    },
    stack_rows::{StackRow, StackRows},
};
use paperpaw_platform::Storage;
use paperpaw_sdk::{Id, MemoryStorage};
use std::{string::String, vec::Vec};

/// A shell that never serves anything itself: the cable is the kernel's.
struct Shell;
impl ShellHttp for Shell {
    fn start(&self, _: &HttpJob<'_>) -> Result<(), i32> {
        Ok(())
    }
    fn abort(&self, _: u32) {}
    fn listen(&self, _: u32, _: u32, _: &mut [u8]) -> Result<usize, ServerReason> {
        panic!("the cable's server is not the shell's")
    }
    fn unlisten(&self, _: u32) {}
    fn reply(&self, _: u32, _: &ServedReply<'_>) -> Result<(), i32> {
        panic!("a cable answer is not the shell's")
    }
    fn close_served(&self, _: u32, _: u32) {}
}
impl ShellNetwork for Shell {
    fn now_ms(&self) -> u64 {
        0
    }
    fn network_up(&self) -> bool {
        false
    }
    fn network_demand(&self, _: bool) {}
}

struct World {
    kernel: Kernel,
    owner: u64,
    card: Rc<MemoryStorage>,
    rows: Rc<StackRows>,
}

fn world() -> World {
    let mut kernel = Kernel::builder(crate::tests::DEVICE).build().unwrap();
    let owner = kernel
        .shared
        .native_instances
        .test_insert(Id::new_static("cab"), None, true)
        .unwrap();
    let card = Rc::new(MemoryStorage::new());
    let rows = Rc::new(StackRows::default());
    {
        let resources = kernel.shared.native_instances.test_resources().unwrap();
        resources
            .wire(HostWiring::new().net(NetInputs::new(
                card.clone(),
                Rc::new(Shell),
                Rc::new(Shell),
                NetLimits::new(49_152, 2).servers(1).cable(true),
            )))
            .unwrap();
        let owners = resources.hosts().owners().unwrap();
        owners.admit(owner, "cab").unwrap();
        owners.subscribe(owner, EventTags::collect(|_| true));
        owners.attach_screens(&rows);
    }
    // The plugin's screen shows (its callbacks serve from it).
    rows.update([StackRow {
        instance: 1,
        owner,
        flags: 0,
    }]);
    kernel.sync_net_policy();
    World {
        kernel,
        owner,
        card,
        rows,
    }
}

impl World {
    fn device(&self) -> Rc<NetDevice> {
        self.kernel.native_cable().unwrap()
    }

    fn with_caller<T>(&self, f: impl FnOnce(&NetDevice, &Caller<'_>) -> T) -> T {
        let hosts = HostServices::new();
        let who = Caller {
            owner: self.owner,
            plugin: "cab",
            features: AdmittedFeatures::NONE,
            hosts: &hosts,
        };
        let _callback = self.rows.enter_callback(self.owner, 1);
        f(&self.device(), &who)
    }

    /// The plugin serves on the cable (`Server::cable`), from its screen.
    fn serve(&self, port: u32) -> Result<(), i32> {
        self.with_caller(|device, who| {
            net_service::serve_for_tests(
                device,
                who,
                &wire::ServiceNetServe {
                    token: 1,
                    port,
                    flags: 0,
                    max_body: 0,
                },
            )
        })
    }

    /// The plugin answers `request` with `status` and `body`, or a file.
    fn respond(&self, request: u32, status: u32, body: &[u8], file: bool) -> Result<(), i32> {
        let mut records = std::vec![Record::ServiceNetResponse(wire::ServiceNetResponse {
            request,
            status,
            content_type: "text/plain",
        })];
        if file {
            records.push(Record::ServiceNetFile(wire::ServiceNetFile {
                role: 0,
                root: 0,
                path: "page.html",
            }));
        } else if !body.is_empty() {
            records.push(Record::ServiceNetBody(wire::ServiceNetBody { data: body }));
        }
        let mut input = std::vec![0u8; 4_096];
        let used = wire::records_encode_counted(&records, 20, 4_096, &mut input).unwrap();
        self.with_caller(|device, who| net_service::respond_for_tests(device, who, &input[..used]))
    }

    /// The events the plugin's callback would get: `(state or request,
    /// target, body length)`, server events with an empty target.
    fn events(&self) -> Vec<(u32, String, u64)> {
        let device = self.device();
        let mut all = Vec::new();
        let mut out = std::vec![0u8; 2_048];
        while let Some(used) = net_service::next_event(&*device, self.owner, &mut out).unwrap() {
            match crate::native_host::codec::decode_any_single(&out[..used]).unwrap() {
                Record::EventServer(e) => all.push((e.state, String::new(), 0)),
                Record::EventServedRequest(e) => {
                    all.push((e.request, String::from(e.target), e.body_len));
                }
                other => panic!("{other:?}"),
            }
        }
        net_service::event_delivered(&*device, self.owner).unwrap();
        all
    }

    fn line(&mut self, at: u64, line: &str) {
        self.kernel.cable_line(Instant(at), line);
    }

    fn sent(&mut self) -> Vec<String> {
        self.kernel.take_cable_lines()
    }
}

#[test]
fn a_request_waits_for_the_server_and_its_answer_goes_back_as_lines() {
    let mut w = world();
    // No server yet: `core.cable` is asked to open (nothing offers it here)
    // and the request without a body waits.
    w.line(1_000, "PPT/1 1 GET /api/hello 0");
    assert!(w.sent().is_empty(), "held, not answered");
    assert_eq!(w.kernel.native_cable_due(), Some(1_000 + HOLD_MS));
    // The screen serves on the cable: no network needed, no address.
    w.serve(0).unwrap();
    w.kernel.native_cable_pump();
    assert_eq!(
        w.events(),
        [(0, String::new(), 0), (1, String::from("/api/hello"), 0)]
    );
    w.respond(1, 200, b"{}", false).unwrap();
    assert_eq!(
        w.sent(),
        ["PPT/1 1 200 text/plain", "PPT/1 1 data 7b7d", "PPT/1 1 end"]
    );
    // Reported gone at the next pass: a second answer finds nothing.
    w.kernel.native_cable_pump();
    assert!(w.respond(1, 200, b"", false).is_err());
}

#[test]
fn an_upload_lands_in_received_and_is_told_whole() {
    let mut w = world();
    w.serve(0).unwrap();
    w.kernel.native_cable_pump();
    let _ = w.events();
    w.line(1_000, "PPT/1 7 PUT /api/file?name=a.txt 5");
    assert!(w.events().is_empty(), "not whole yet");
    w.line(1_001, "PPT/1 7 data 6162");
    w.line(1_002, "PPT/1 7 data 636465");
    let told = w.events();
    assert_eq!(told.len(), 1);
    let (request, target, len) = told[0].clone();
    assert_eq!((target.as_str(), len), ("/api/file?name=a.txt", 5));
    let path = alloc::format!(".paperpaw/plugins/cab/.received/1/{request}");
    let mut buf = [0u8; 8];
    let read = w.card.read_at(&path, 0, &mut buf).unwrap();
    assert_eq!(&buf[..read], b"abcde");
    w.respond(request, 201, b"", false).unwrap();
    assert_eq!(w.sent(), ["PPT/1 7 201 text/plain", "PPT/1 7 end"]);
}

#[test]
fn without_a_server_a_body_is_refused_and_a_held_request_times_out() {
    let mut w = world();
    w.line(1_000, "PPT/1 2 PUT /api/file 3");
    assert_eq!(w.sent()[0], "PPT/1 2 503 text/plain;charset=utf-8");
    // Its data lines are passed over: no answer per line (Senior S1).
    w.line(1_000, "PPT/1 2 data 6162");
    w.line(1_000, "PPT/1 2 data 63");
    assert!(w.sent().is_empty());
    w.line(1_000, "PPT/1 3 GET /api/hello 0");
    assert!(w.sent().is_empty());
    w.kernel.now = Instant(1_000 + HOLD_MS);
    w.kernel.native_cable_pump();
    assert_eq!(w.sent()[0], "PPT/1 3 503 text/plain;charset=utf-8");
    assert_eq!(w.kernel.native_cable_due(), None);
}

#[test]
fn a_file_is_refused_on_the_cable() {
    let mut w = world();
    w.card
        .write(".paperpaw/plugins/cab/page.html", b"<p>")
        .unwrap();
    w.serve(0).unwrap();
    w.kernel.native_cable_pump();
    let _ = w.events();
    w.line(1_000, "PPT/1 4 GET / 0");
    let (request, _, _) = w.events()[0].clone();
    assert_eq!(
        w.respond(request, 200, b"", true),
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    );
    assert_eq!(w.sent()[0], "PPT/1 4 500 text/plain;charset=utf-8");
}

#[test]
fn after_back_a_request_before_the_pause_is_ended_on_the_device() {
    let mut w = world();
    w.serve(0).unwrap();
    w.kernel.native_cable_pump();
    let _ = w.events();
    w.line(1_000, "PPT/1 1 GET /api/hello 0");
    let _ = w.events();
    // The screen closed: the server stops, the open request is answered.
    net_service::stop_for_tests(&*w.device(), w.owner, 1).unwrap();
    assert_eq!(w.sent()[0], "PPT/1 1 503 text/plain;charset=utf-8");
    w.line(2_000, "PPT/1 2 GET /api/hello 0");
    assert_eq!(w.sent()[0], "PPT/1 2 403 text/plain;charset=utf-8");
    // After the pause it opens the screen again and waits.
    w.line(
        2_000 + crate::io::cable::PAUSE_MS,
        "PPT/1 3 GET /api/hello 0",
    );
    assert!(w.sent().is_empty());
}

#[test]
fn the_cable_does_not_take_the_devices_one_network_server() {
    let w = world();
    w.serve(0).unwrap();
    // `SERVERS` is 1 here: a network server still has its place.
    w.with_caller(|device, who| {
        net_service::serve_for_tests(
            device,
            who,
            &wire::ServiceNetServe {
                token: 2,
                port: 8080,
                flags: 0,
                max_body: 0,
            },
        )
    })
    .unwrap();
}
