//! Scripted, windowless runs for screenshots, CI and bug reports.
//!
//! A script is a comma-separated list of steps:
//!
//! | Step | Meaning |
//! |------|---------|
//! | `page-forward` | short press of a button |
//! | `long:confirm` | long press of a button |
//! | `tap:240:400` | tap at logical coordinates |
//! | `tap-on:Night mode` | tap the row, button or heading with this text (or beginning so) |
//! | `describe` | print what the screen shows, part by part |
//! | `long-tap:240:400` | long press on the touch screen |
//! | `swipe:400:400:100:400` | swipe from one point to another |
//! | `down:240:400`, `move:300:400`, `up:300:400` | a finger put down, moved and lifted, step by step (e.g. to look at a slider while it moves) |
//! | `wait:500` | let 500 ms pass |
//! | `run:calibre.open` | run an action, as a button bound to it would |
//! | `open:home.add-widget:kind=core.widgets.books` | open a screen with its parameters, as a plugin would |
//! | `redraw` | draw the whole screen again, nothing on it changed |
//! | `phone-joins` | a phone joins the device's own network (the setup with a phone) |
//! | `ble-key:right` | the pretend Bluetooth remote's key pressed (`right`, `left`, `up`, `down`, `page-down`, `page-up`, `space`, `enter`, `escape`, `volume-up`, `volume-down`), once its keys go to the key maps |
//! | `reset-peak` | measure the heap's peak anew from now (what one step takes) |
//! | `emit:core.library-changed` | send an event without parameters, as a plugin would |
//! | `type:hello world` | type into the text field shown, as if on its keys (a script's commas cannot be typed) |
//! | `type-enter:hello` | the same, then confirm it (`type-enter:` confirms only) |
//! | `serve:20` | answer plugins' servers in real time for 20 s |
//! | `shot:out/a.png` | save a screenshot now (the last one comes with `--screenshot`) |

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use paperpaw_platform::{ButtonState, InputEvent, Point, PointerEvent, PointerPhase, PointerTool};

use crate::device::Device;

/// Time between script steps, long enough to never look like a long press.
const STEP_MS: u64 = 100;
/// Comfortably above the default long-press threshold.
const LONG_MS: u64 = 1200;

pub fn run_script(device: &mut Device, script: &str) -> Result<(), String> {
    for step in script.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        run_step(device, step).map_err(|e| format!("step `{step}`: {e}"))?;
        // Network answers take real time; wait for them, so runs repeat.
        device.poll_http(Some(std::time::Duration::from_secs(25)));
        device.advance(STEP_MS);
        device.tick_if_due();
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn run_step(device: &mut Device, step: &str) -> Result<(), String> {
    let parts: Vec<&str> = step.split(':').collect();
    let touch_step = matches!(
        parts.first(),
        Some(&("tap" | "tap-on" | "long-tap" | "swipe" | "down" | "move" | "up"))
    );
    if touch_step && !device.profile.device.input.touch {
        return Err(format!(
            "`{}` has no touch screen",
            device.profile.device.id
        ));
    }
    let number = |s: &str| {
        s.parse::<i32>()
            .map_err(|_| format!("`{s}` is not a number"))
    };
    match parts.as_slice() {
        ["wait", ms] => {
            let ms: u64 = ms.parse().map_err(|_| format!("`{ms}` is not a number"))?;
            hold(device, ms);
        }
        ["shot", path @ ..] if !path.is_empty() => {
            // Paths may hold `:` (`C:/…`).
            let path = path.join(":");
            screenshot(device, Path::new(&path))?;
        }
        ["serve", seconds] => {
            let seconds: u64 = seconds
                .parse()
                .map_err(|_| format!("`{seconds}` is not a number"))?;
            let end = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
            while std::time::Instant::now() < end {
                device.poll_servers();
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        ["long", name] => press(device, button(device, name)?, LONG_MS),
        ["run", action] => {
            let action = paperpaw_core::sdk::Id::parse(action).map_err(|e| e.to_string())?;
            device.run_action(action);
        }
        ["open", screen, given @ ..] => {
            let screen = paperpaw_core::sdk::Id::parse(screen).map_err(|e| e.to_string())?;
            let mut params = paperpaw_core::sdk::Params::new();
            for pair in given {
                let (key, value) = pair
                    .split_once('=')
                    .ok_or_else(|| format!("`{pair}` is no key=value"))?;
                params = params.with(key.to_string(), value);
            }
            device.open_screen(screen, params);
        }
        ["redraw"] => device.draw_again(),
        ["phone-joins"] => device.phone_joins(),
        ["ble-key", key] => {
            if !device.ble_key(key) {
                return Err(format!("no Bluetooth remote types `{key}` now"));
            }
        }
        ["reset-peak"] => crate::budget::reset_peak(),
        ["emit", event] => {
            let event = paperpaw_core::sdk::Id::parse(event).map_err(|e| e.to_string())?;
            let params = paperpaw_core::sdk::Params::new();
            device.call(|kernel, now| kernel.send_event(now, event, params));
        }
        ["type" | "type-enter", text @ ..] => {
            if !device.type_text(&text.join(":"), parts[0] == "type-enter") {
                return Err(String::from("no text field shows"));
            }
        }
        ["tap", x, y] => touch(device, Point::new(number(x)?, number(y)?), None, 60),
        ["tap-on", label @ ..] if !label.is_empty() => {
            let label = label.join(":");
            let parts = device.kernel.describe();
            let part = paperpaw_core::sdk::semantics::find(&parts, &label)
                .ok_or_else(|| format!("nothing on the screen says `{label}`"))?;
            let a = part.area;
            let center = Point::new(
                i32::from(a.x) + i32::from(a.width) / 2,
                i32::from(a.y) + i32::from(a.height) / 2,
            );
            touch(device, center, None, 60);
        }
        ["describe"] => {
            for part in device.kernel.describe() {
                let mut state = String::new();
                if part.focused {
                    state.push_str(" (focused)");
                }
                match part.on {
                    Some(true) => state.push_str(" (on)"),
                    Some(false) => state.push_str(" (off)"),
                    None => {}
                }
                let a = part.area;
                println!(
                    "{:?} \"{}\"{state} at {},{} {}x{}",
                    part.role, part.label, a.x, a.y, a.width, a.height
                );
            }
        }
        ["long-tap", x, y] => touch(device, Point::new(number(x)?, number(y)?), None, LONG_MS),
        ["swipe", x1, y1, x2, y2] => touch(
            device,
            Point::new(number(x1)?, number(y1)?),
            Some(Point::new(number(x2)?, number(y2)?)),
            150,
        ),
        [phase @ ("down" | "move" | "up"), x, y] => {
            let phase = match *phase {
                "down" => PointerPhase::Down,
                "move" => PointerPhase::Move,
                _ => PointerPhase::Up,
            };
            device.input(finger(phase, Point::new(number(x)?, number(y)?)));
        }
        [name] => press(device, button(device, name)?, 60),
        _ => return Err("unknown step".into()),
    }
    Ok(())
}

fn button(device: &Device, name: &str) -> Result<&'static str, String> {
    device
        .profile
        .buttons
        .iter()
        .copied()
        .find(|b| *b == name)
        .ok_or_else(|| {
            format!(
                "`{}` has no button `{name}` (buttons: {})",
                device.profile.device.id,
                device.profile.buttons.join(", ")
            )
        })
}

fn press(device: &mut Device, name: &'static str, hold_ms: u64) {
    let event = |state| InputEvent::Button { name, state };
    device.input(event(ButtonState::Pressed));
    hold(device, hold_ms);
    device.input(event(ButtonState::Released));
}

/// A sample of the one finger scripts use.
fn finger(phase: PointerPhase, point: Point) -> InputEvent {
    InputEvent::Pointer(PointerEvent::new(
        0,
        phase,
        point,
        PointerTool::Finger,
        1000,
    ))
}

fn touch(device: &mut Device, start: Point, end: Option<Point>, hold_ms: u64) {
    let event = finger;
    device.input(event(PointerPhase::Down, start));
    hold(device, hold_ms);
    let end = end.unwrap_or(start);
    if end != start {
        device.input(event(PointerPhase::Move, end));
    }
    device.input(event(PointerPhase::Up, end));
}

/// Lets time pass in small steps so due long presses fire on time.
/// Lets `millis` of simulated time pass, waking the kernel at every time it
/// asked for, exactly like a device would.
fn hold(device: &mut Device, millis: u64) {
    let end = device.now().add_millis(millis);
    loop {
        let now = device.now();
        let next = device.next_wake().filter(|&at| at < end).unwrap_or(end);
        // At least 1 ms per round, so "as soon as possible" requests cannot
        // stall simulated time.
        device.advance(next.since(now).max(1));
        device.tick_if_due();
        if next >= end {
            return;
        }
    }
}

/// Writes what the panel currently shows to a PNG file.
pub fn screenshot(device: &Device, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(
        BufWriter::new(file),
        u32::from(device.panel.width()),
        u32::from(device.panel.height()),
    );
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer
        .write_image_data(&device.panel.rgb_bytes())
        .map_err(|e| e.to_string())
}
