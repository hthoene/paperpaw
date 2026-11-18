//! The plugin's readers and calls against a fake Home Assistant server
//! (`testdata/fake_ha.py`, a real HTTP server on the loopback): a long list of
//! states streamed in the pieces the plugin reads a saved file in, the
//! entities it keeps, a refused token, and the service calls that switch.
//! Skipped where python3 is missing.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_plugin_smart_home::{
    link::{base, service_body, service_url, state_url, states_url},
    model::{Entity, Kind, Store, domain, kind_of, service_of},
    states::{PIECE, Record, States},
};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};

const TOKEN: &str = "test-token-0123456789";

struct Server {
    child: Child,
    port: u16,
}

impl Server {
    fn start(filler: usize) -> Option<Self> {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/fake_ha.py");
        let mut child = Command::new("python3")
            .args([
                "-I",
                script,
                "--token",
                TOKEN,
                "--filler",
                &filler.to_string(),
            ])
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut()?)
            .read_line(&mut line)
            .ok()?;
        let port = line.strip_prefix("PORT ")?.trim().parse().ok()?;
        Some(Self { child, port })
    }

    fn base(&self) -> String {
        base(&format!("http://127.0.0.1:{}", self.port), false).unwrap()
    }

    /// One request as the host sends it: the status and the body.
    fn call(&self, method: &str, url: &str, token: Option<&str>, body: &str) -> (u32, Vec<u8>) {
        let path = &url[url.find("/api").unwrap()..];
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        let auth = token.map_or(String::new(), |t| format!("Authorization: Bearer {t}\r\n"));
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: x\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&raw[..split]).into_owned();
        let status = head.split(' ').nth(1).unwrap().parse().unwrap();
        (status, raw[split + 4..].to_vec())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

/// The records of a body, read in pieces of `PIECE`.
fn records(body: &[u8]) -> Vec<Record> {
    let mut out = Vec::new();
    let mut states = States::new();
    for piece in body.chunks(PIECE) {
        states
            .feed(piece, &mut |record| {
                out.push(record);
                Ok(())
            })
            .unwrap();
    }
    out
}

#[test]
fn a_long_list_is_read_and_only_the_chosen_entities_are_kept() {
    let Some(server) = Server::start(1500) else {
        eprintln!("python3 is missing: skipped");
        return;
    };
    let base = server.base();
    let (status, body) = server.call("GET", &states_url(&base).unwrap(), Some(TOKEN), "");
    assert_eq!(status, 200);
    assert!(body.len() > 600_000, "{} bytes", body.len());

    // The plugin keeps the chosen ones only: nothing else is held at once.
    let mut store = Store::default();
    for id in [
        "sensor.kitchen_temperature",
        "light.desk",
        "binary_sensor.front_door",
        "scene.evening",
    ] {
        store
            .add(Entity {
                id: id.into(),
                ..Entity::default()
            })
            .unwrap();
    }
    let mut states = States::new();
    let (mut seen, mut count, mut most_held) = (0, 0, 0);
    for piece in body.chunks(PIECE) {
        states
            .feed(piece, &mut |record| {
                count += 1;
                if let Some(entity) = store.find_mut(&record.id) {
                    entity.state = record.state;
                    entity.unit = record.unit;
                    entity.class = record.class;
                    entity.name = record.name;
                    seen += 1;
                }
                most_held = most_held.max(store.entities.len());
                Ok(())
            })
            .unwrap();
    }
    assert_eq!(count, 1500 + 11);
    assert_eq!(seen, 4);
    assert_eq!(most_held, 4);
    let t = store.find("sensor.kitchen_temperature").unwrap();
    assert_eq!(
        (t.state.as_str(), t.unit.as_str(), t.name.as_str()),
        ("21.537", "°C", "Kitchen temperature")
    );
    assert_eq!(
        store.find("binary_sensor.front_door").unwrap().class,
        "door"
    );
    assert_eq!(store.find("light.desk").unwrap().state, "off");
    // What it keeps reads as a person reads it.
    assert!(store.to_text().unwrap().len() < 1000);
}

#[test]
fn a_wrong_token_is_refused_in_the_servers_words() {
    let Some(server) = Server::start(0) else {
        return;
    };
    let url = states_url(&server.base()).unwrap();
    let (status, body) = server.call("GET", &url, Some("wrong"), "");
    assert_eq!(status, 401);
    assert!(records(&body).is_empty());
    assert_eq!(server.call("GET", &url, None, "").0, 401);
    let (status, _) = server.call(
        "GET",
        &state_url(&server.base(), "light.nope").unwrap(),
        Some(TOKEN),
        "",
    );
    assert_eq!(status, 404);
}

#[test]
fn toggles_call_the_service_and_the_answer_confirms_the_state() {
    let Some(server) = Server::start(0) else {
        return;
    };
    let base = server.base();
    let toggle = |id: &str| {
        let service = service_of(id).unwrap();
        let url = service_url(&base, domain(id), service).unwrap();
        let (status, body) = server.call("POST", &url, Some(TOKEN), &service_body(id).unwrap());
        (status, records(&body))
    };
    let (status, changed) = toggle("light.desk");
    assert_eq!(status, 200);
    assert_eq!(changed.len(), 1);
    assert_eq!(
        (changed[0].id.as_str(), changed[0].state.as_str()),
        ("light.desk", "on")
    );
    let (_, changed) = toggle("light.desk");
    assert_eq!(changed[0].state, "off");
    let (_, changed) = toggle("switch.coffee_machine");
    assert_eq!(changed[0].state, "on");
    // A scene turns its lights on; the one that was on is not in the answer.
    assert_eq!(kind_of("scene.evening"), Some(Kind::Scene));
    let (status, changed) = toggle("scene.evening");
    assert_eq!(status, 200);
    assert_eq!(
        changed.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["light.desk"]
    );
    // Asked for after, the single state is the same as the answer said.
    let (status, body) = server.call(
        "GET",
        &state_url(&base, "light.desk").unwrap(),
        Some(TOKEN),
        "",
    );
    assert_eq!(status, 200);
    assert_eq!(records(&body)[0].state, "on");
    // A body that is not a call is the server's refusal.
    let url = service_url(&base, "light", "toggle").unwrap();
    assert_eq!(server.call("POST", &url, Some(TOKEN), "nonsense").0, 400);
}
