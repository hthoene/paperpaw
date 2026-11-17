//! The X4 remote's wire protocol on the emulator (`--headless --repl`): one
//! command line per line of standard input, answered with `PP:` lines on
//! standard output, so `tools/x4-remote.py` drives the emulator with the
//! very scripts it drives the X4 with (`--emulator`).
//!
//! The commands are those of [`paperpaw_x4::remote`] (parsed by it), as far
//! as an emulator can answer them: buttons, touch, actions, slots, screens,
//! events, typing, settings, `describe`, `screen`, `heap`, `shot`, `restart`.
//! The rest is answered `PP:ERR not on the emulator`; the tool answers the
//! card's commands itself (the card is a folder here). Every frame the panel
//! shows is told first, as a log line like the X4's
//! (`INFO - frame Partial 0,0 480x800: emulated`), so scripts that wait for
//! or time frames run unchanged (`wait <ms>` lets emulated time pass, `png <hex path>`
//! saves a screenshot: the tool's, not the X4's). Time here is emulated: what a script
//! measures in milliseconds is the host's, not the panel's.

use std::io::{BufRead, Write};

use paperpaw_core::Effects;
use paperpaw_x4::remote::{Command, Touch, hex_bytes, part_state};

use crate::budget;
use crate::device::Device;
use crate::headless;

/// Serves the commands from standard input until it ends.
pub fn serve(device: &mut Device) {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let before = device.frames_shown;
        let result = command(device, &line);
        // The frames this command showed, as the X4's log tells them.
        let new = usize::try_from(device.frames_shown - before).unwrap_or(usize::MAX);
        let skip = device.shown.len().saturating_sub(new);
        for (area, mode) in &device.shown[skip..] {
            println!(
                "INFO - frame {mode:?} {},{} {}x{}: emulated",
                area.x, area.y, area.width, area.height
            );
        }
        match result {
            Ok(()) => {}
            Err(why) => println!("PP:ERR {why}"),
        }
        let _ = std::io::stdout().flush();
    }
}

fn pairs(params: Vec<(String, String)>) -> paperpaw_core::sdk::Params {
    let mut with = paperpaw_core::sdk::Params::new();
    for (key, value) in params {
        with.set(key, value);
    }
    with
}

fn id(text: &str) -> Result<paperpaw_core::sdk::Id, String> {
    paperpaw_core::sdk::Id::parse(text).map_err(|_| format!("no id: {text}"))
}

fn ok() -> Result<(), String> {
    println!("PP:OK");
    Ok(())
}

/// Answers one line.
fn command(device: &mut Device, line: &str) -> Result<(), String> {
    // Not the X4's: a screenshot as a PNG file (the path in hexadecimal).
    if let Some(path) = line.strip_prefix("png ") {
        let path = String::from_utf8(unhex(path.trim())?).map_err(|e| e.to_string())?;
        headless::screenshot(device, std::path::Path::new(&path))?;
        return ok();
    }
    // Not the X4's either: emulated time passes (the X4's own passes by itself).
    if let Some(ms) = line.strip_prefix("wait ") {
        headless::pass_time(
            device,
            ms.trim()
                .parse()
                .map_err(|_| format!("no milliseconds: {ms}"))?,
        );
        return ok();
    }
    let command = paperpaw_x4::remote::parse(line).ok_or_else(|| format!("no command: {line}"))?;
    if ask(device, &command)? {
        return Ok(());
    }
    match command {
        Command::Press { button, ms } => {
            headless::press_button(device, button, ms)?;
            // As scripts: the network and the clock go on a little.
            headless::settle(device);
            ok()
        }
        Command::Run(action) => {
            device.run_action(id(&action)?);
            ok()
        }
        Command::Open(path) => {
            device.call(|kernel, now| kernel.open_book(now, &path));
            ok()
        }
        Command::Slot { id: slot, params } => {
            let (slot, with) = (id(&slot)?, pairs(params));
            device.call(|kernel, now| kernel.open_slot(now, slot, with));
            ok()
        }
        Command::Show { id: screen, params } => {
            device.open_screen(id(&screen)?, pairs(params));
            ok()
        }
        Command::Emit { event, params } => {
            let (event, with) = (id(&event)?, pairs(params));
            device.call(|kernel, now| kernel.send_event(now, event, with));
            ok()
        }
        Command::Type { text, done } => {
            if device.type_text(&text, done) {
                ok()
            } else {
                Err("no text field".into())
            }
        }
        Command::Set { id: setting, value } => {
            let mut failed = None;
            device.call(
                |kernel, now| match kernel.set_setting(now, &setting, &value) {
                    Ok(effects) => effects,
                    Err(why) => {
                        failed = Some(why);
                        Effects::default()
                    }
                },
            );
            failed.map_or_else(ok, |why| Err(why.into()))
        }
        Command::Touch { phase, x, y } => {
            headless::touch_phase(
                device,
                matches!(phase, Touch::Down),
                matches!(phase, Touch::Up),
                x,
                y,
            );
            headless::settle(device);
            ok()
        }
        Command::Restart => {
            device.restart();
            ok()
        }
        Command::Sync => ok(),
        Command::Log(_) => ok(),
        _ => Err("not on the emulator".into()),
    }
}

/// What only asks: a setting, the screen, the parts shown, the heap, the panel. Whether `command` was one.
fn ask(device: &mut Device, command: &Command) -> Result<bool, String> {
    match command {
        Command::Setting(setting) => {
            let value = device.kernel.settings().get(setting);
            let value = value.ok_or("no such setting")?;
            println!("PP:SETTING {}", value.to_config_text());
        }
        Command::Settings(prefix) => {
            let settings = device.kernel.settings();
            for spec in settings.specs() {
                let name = spec.id.as_str();
                if name.starts_with(prefix.as_str())
                    && let Some(value) = settings.get(name)
                {
                    println!("PP:VALUE {name} {}", value.to_config_text());
                }
            }
            println!("PP:END");
        }
        Command::Screen => {
            let screen = device.kernel.active_screen().map(ToString::to_string);
            println!("PP:SCREEN {}", screen.unwrap_or_default());
        }
        Command::Describe => {
            for part in device.kernel.describe() {
                let state = part_state(part.focused, part.on, part.disabled);
                let a = part.area;
                println!(
                    "PP:PART {:?} {} {} {} {} {state} {}",
                    part.role,
                    a.x,
                    a.y,
                    a.width,
                    a.height,
                    part.label.replace('\n', " ")
                );
            }
            println!("PP:END");
        }
        Command::Heap => {
            // No stack here; no fragmentation: the largest block is what the
            // budget leaves.
            let stats = budget::stats();
            let free = stats
                .limit
                .map_or(0, |limit| limit.saturating_sub(stats.live));
            println!("PP:HEAP {} {free} 0 0 {free} {}", stats.live, stats.peak);
        }
        Command::Shot => shot(device)?,
        _ => return Ok(false),
    }
    Ok(true)
}

/// The panel as the X4's `shot` sends it: 8-bit gray rows, run-length coded.
fn shot(device: &Device) -> Result<(), String> {
    let (width, height) = (
        usize::from(device.panel.width()),
        usize::from(device.panel.height()),
    );
    let rgb = device.panel.rgb_bytes();
    println!("PP:FRAME {width} {height} 8");
    for y in 0..height {
        let mut runs = String::new();
        let mut last: Option<(u8, u8)> = None;
        let flush = |run: Option<(u8, u8)>, runs: &mut String| {
            if let Some((count, value)) = run {
                runs.push_str(&format!("{count:02x}{value:02x}"));
            }
        };
        for x in 0..width {
            let at = (y * width + x) * 3;
            let pixel = rgb.get(at..at + 3).unwrap_or(&[255, 255, 255]);
            let gray =
                ((u32::from(pixel[0]) * 30 + u32::from(pixel[1]) * 59 + u32::from(pixel[2]) * 11)
                    / 100) as u8;
            match &mut last {
                Some((count, value)) if *value == gray && *count < 255 => *count += 1,
                _ => {
                    flush(last.take(), &mut runs);
                    last = Some((1, gray));
                }
            }
        }
        flush(last.take(), &mut runs);
        println!("PP:ROW {y} {runs}");
    }
    println!("PP:END");
    Ok(())
}

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    hex_bytes(text.as_bytes()).ok_or_else(|| String::from("not hexadecimal"))
}
