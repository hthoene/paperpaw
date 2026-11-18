#!/usr/bin/env python3
"""A fake Home Assistant REST server for the smart home plugin's tests.

    python3 -I fake_ha.py [--token TOKEN] [--filler N] [--port N]

Prints `PORT <n>` once it listens, then one line per request (`GET /api/states 200`).
It serves what the plugin uses and nothing else, with a long-lived token as the
bearer:

* GET /api/states: every entity (the ones of a small home, then N filler
  sensors, so the list is long), as Home Assistant writes them.
* GET /api/states/<entity_id>: one state, 404 where there is none.
* POST /api/services/<domain>/<service> with {"entity_id": ...}: toggle, turn_on,
  turn_off and press change the state kept here and answer with the states that
  changed; a scene turns its lights on.

A wrong or missing token is a 401 with Home Assistant's message.
"""
import argparse
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HOME = [
    ("sensor.kitchen_temperature", "21.537", {"unit_of_measurement": "°C", "device_class": "temperature", "friendly_name": "Kitchen temperature"}),
    ("sensor.kitchen_humidity", "48.2", {"unit_of_measurement": "%", "device_class": "humidity", "friendly_name": "Kitchen humidity"}),
    ("sensor.house_power", "342.8", {"unit_of_measurement": "W", "device_class": "power", "friendly_name": "House power"}),
    ("binary_sensor.front_door", "off", {"device_class": "door", "friendly_name": "Front door"}),
    ("binary_sensor.hall_motion", "on", {"device_class": "motion", "friendly_name": "Hall motion"}),
    ("light.living_room_lamp", "on", {"friendly_name": "Living room lamp", "brightness": 180, "supported_color_modes": ["brightness"]}),
    ("light.desk", "off", {"friendly_name": "Desk light"}),
    ("switch.coffee_machine", "off", {"friendly_name": "Coffee machine"}),
    ("scene.evening", "unknown", {"friendly_name": "Evening", "entity_id": ["light.living_room_lamp", "light.desk"]}),
    ("lock.front", "locked", {"friendly_name": "Front lock"}),
    ("climate.hall", "heat", {"friendly_name": "Hall heating", "temperature": 21}),
]


def make_state(entity_id, state, attributes):
    return {
        "entity_id": entity_id,
        "state": state,
        "attributes": attributes,
        "last_changed": "2026-11-18T01:02:03.000000+00:00",
        "last_updated": "2026-11-18T01:02:03.000000+00:00",
        "context": {"id": "01HZ", "parent_id": None, "user_id": None},
    }


class Home:
    def __init__(self, filler):
        self.states = {}
        for entity_id, state, attributes in HOME:
            self.states[entity_id] = make_state(entity_id, state, dict(attributes))
        for n in range(filler):
            entity_id = f"sensor.filler_{n:04d}"
            self.states[entity_id] = make_state(
                entity_id, str(10 + n % 50) + "." + str(n % 10),
                {"unit_of_measurement": "°C", "device_class": "temperature",
                 "friendly_name": f"Filler sensor {n:04d}", "state_class": "measurement",
                 "icon": "mdi:thermometer", "attribution": "Fake data, not real: \"quoted\" ü"},
            )

    def call(self, domain, service, entity_id):
        changed = []
        targets = [entity_id]
        if domain == "scene" and service == "turn_on":
            scene = self.states.get(entity_id)
            targets = scene["attributes"].get("entity_id", []) if scene else []
            service = "turn_on"
        for target in targets:
            state = self.states.get(target)
            if state is None or target.split(".")[0] not in (domain, "light", "switch"):
                continue
            old = state["state"]
            if service == "toggle":
                new = "off" if old == "on" else "on"
            elif service == "turn_on":
                new = "on"
            elif service == "turn_off":
                new = "off"
            else:
                continue
            if new != old:
                state["state"] = new
                changed.append(state)
        return changed


def serve(token, filler, port):
    home = Home(filler)

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):
            pass

        def reply(self, status, body):
            data = json.dumps(body).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(data)
            print(f"{self.command} {self.path} {status}", flush=True)

        def authorised(self):
            if self.headers.get("Authorization") == f"Bearer {token}":
                return True
            self.reply(401, {"message": "401: Unauthorized"})
            return False

        def do_GET(self):
            if not self.authorised():
                return
            if self.path == "/api/states":
                self.reply(200, list(home.states.values()))
            elif self.path.startswith("/api/states/"):
                state = home.states.get(self.path[len("/api/states/"):])
                self.reply(200, state) if state else self.reply(404, {"message": "Entity not found."})
            elif self.path == "/api/":
                self.reply(200, {"message": "API running."})
            else:
                self.reply(404, {"message": "404: Not Found"})

        def do_POST(self):
            length = int(self.headers.get("Content-Length", "0"))
            body = self.rfile.read(length)
            if not self.authorised():
                return
            parts = self.path.split("/")
            if len(parts) == 5 and parts[:3] == ["", "api", "services"]:
                try:
                    entity_id = json.loads(body)["entity_id"]
                except (ValueError, KeyError, TypeError):
                    return self.reply(400, {"message": "Message malformed"})
                self.reply(200, home.call(parts[3], parts[4], entity_id))
            else:
                self.reply(404, {"message": "404: Not Found"})

    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"PORT {server.server_address[1]}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--token", default="test-token")
    parser.add_argument("--filler", type=int, default=0)
    parser.add_argument("--port", type=int, default=0)
    args = parser.parse_args()
    sys.exit(serve(args.token, args.filler, args.port))
