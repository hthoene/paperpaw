// Node smoke of the built web module (python shells/web/build.py, then):
//   node shells/web/tests/smoke.mjs <abs path to paperpaw-web.wasm> <abs path to www/paperpaw-web.js>
// No DOM, only what the page's wrapper needs: starts an X4 and a Boox, runs the
// store through a recorded answer and opens a sample book on the Boox.
import { readFileSync } from "node:fs";
const [wasm, wrapper] = process.argv.slice(2);
globalThis.ImageData = class { constructor(data, width, height) { this.data = data; this.width = width; this.height = height; } };
const { PaperPawDevice } = await import(wrapper);
const module = await WebAssembly.compile(readFileSync(wasm));
const LIST = {format: 2, plugins: [{id: "airy-theme", name: "Airy Theme", summary: "Light and calm.", group: "look", author: "Someone", licence: "MIT", kind: "data", latest: {version: "1.0.0", sdk: "0.4", formats: 1, size: 1234}}]};
let failed = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "ok  " : "FAIL"} ${name} ${extra}`); if (!ok) failed++; };
for (const profile of ["xteink-x4", "boox-go-7"]) {
  const d = await PaperPawDevice.from(module);
  d.create({ profile, language: "en", dark: false });
  d.start();
  let t = 5000;
  d.advance(t);
  const home = d.command("describe", t);
  check(`${profile}: starts on the home screen`, String(home.screen).startsWith("home."), home.screen);
  d.setFixtures([{ method: "GET", url: "https://paperpaw.org/api/v1/plugins.json*", headers: { "content-type": "application/json" }, body: LIST, delay: 300 }]);
  t += 1000; d.command("run store.open", t);
  t += 200; d.advance(t);
  const told = JSON.stringify(d.requests());
  t += 400; d.advance(t);
  const store = d.command("describe", t);
  // An X4 must join its radio first (a test set-up of its own): the store is checked on the Boox.
  if (profile === "boox-go-7") check(`${profile}: the store lists the plugin from the fixture`, JSON.stringify(store).includes("Look"), store.screen);
  if (profile === "boox-go-7") check(`${profile}: the request was told`, told.includes("plugins.json"), told.slice(0, 80));
  d.command("run core.home", t += 500); d.advance(t += 1000);
  const back = d.command("describe", t);
  check(`${profile}: back home`, String(back.screen).startsWith("home."), back.screen);
  if (profile === "boox-go-7") {
    // A book: the home's start-reading card opens the library, a tap on a book opens it.
    const [w, h] = d.size();
    const tap = (x, y) => { d.pointer("down", x, y, t += 60000); d.pointer("up", x, y, t += 100); d.advance(t += 3000); };
    tap(w / 2, h * 0.34);
    const lib = d.command("describe", t);
    const alice = (lib.parts ?? []).find((p) => (p.label ?? "").toLowerCase().includes("alice"));
    check(`${profile}: the library lists the sample book`, !!alice, lib.screen);
    if (alice) {
      tap(alice.x + alice.width / 2, alice.y + alice.height / 2);
      t += 8000; d.advance(t);
      const book = d.command("describe", t);
      check(`${profile}: the book opens in the reader`, String(book.screen).startsWith("reader."), book.screen);
      const page = d.shown();
      check(`${profile}: the page has text pixels`, page.data.filter((v) => v < 128).length > 1000);
    }
  }
  const frame = d.shown();
  check(`${profile}: a frame is drawn`, frame.width > 0 && frame.data.some((v) => v !== 255));
}
console.log(failed ? `SMOKE FAILED (${failed})` : "SMOKE OK");
process.exit(failed ? 1 : 0);
