//! #747 slot 9: a foreign draw callback fills rectangles in a real emulator
//! framebuffer through the owner's scoped Canvas forwarding. Colour is the
//! generated `0x00RRGGBB`; the loader forwards scalars and converts nothing.
use super::super::canvas as bridge;
use super::environment::{BOOL, run};
use super::*;
use paperpaw_core::native_host::registrations::Registration;
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{Color, PixelFormat, SystemStatus};

const WIDTH: u16 = 160;
const HEIGHT: u16 = 80;
const RECT: (i32, i32, u32, u32) = (8, 8, 64, 32);

/// A draw callback that paints black, then proves a colour with the reserved
/// high byte is refused before any pixel changes.
const DRAW_BODY: &str = r#"
    let status = unsafe { (h.canvas_fill_rect)(s.ctx, 8, 8, 64, 32, 0x00000000) };
    if status != OK { return status; }
    let refused = unsafe { (h.canvas_fill_rect)(s.ctx, 0, 0, 160, 80, 0x01000000) };
    if refused != ERR_INVALID { return -1000; }
"#;

fn table_fill<S>(context: Pin<&Context<S>>, color: u32) -> i32 {
    // SAFETY: exact pinned context; the call retains nothing.
    unsafe {
        (context.table.canvas_fill_rect)(
            ptr::from_ref(context.get_ref()).cast_mut().cast(),
            RECT.0,
            RECT.1,
            RECT.2,
            RECT.3,
            color,
        )
    }
}

pub(super) fn gray(frame: &Framebuffer, x: usize, y: usize) -> u8 {
    let view = frame.view();
    assert_eq!(view.format, PixelFormat::Gray8);
    view.data[y * view.stride + x]
}

pub(super) fn save_png(frame: &Framebuffer, path: &Path) {
    let view = frame.view();
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        u32::from(view.width),
        u32::from(view.height),
    );
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    let rows: Vec<u8> = (0..usize::from(view.height))
        .flat_map(|y| {
            view.data[y * view.stride..y * view.stride + usize::from(view.width)].to_vec()
        })
        .collect();
    writer.write_image_data(&rows).unwrap();
}

#[test]
fn foreign_draw_fills_the_emulator_framebuffer_through_scoped_slot_9() {
    let fixture = std::rc::Rc::new(foreign::fixture_query(&[], false, DRAW_BODY));
    let shot = std::env::var_os("PAPERPAW_E2E_SCREENSHOT_SYNTHETIC").map(std::path::PathBuf::from);
    run(&BOOL, "true", SystemStatus::default(), move |env| {
        let (plugin, evidence) = fixture.open();
        let batch = plugin
            .make_registered(plugin.begin::<Registration>().unwrap())
            .unwrap()
            .commit()
            .unwrap();
        let context = plugin.owner.as_ref().context();
        let depth = DispatchDepth::new();
        let mut frame = Framebuffer::new(WIDTH, HEIGHT, PixelFormat::Gray8).unwrap();
        frame
            .canvas()
            .fill_rect(0, 0, u32::from(WIDTH), u32::from(HEIGHT), Color::WHITE);
        // Outside any draw dispatch the slot has no canvas.
        assert_eq!(table_fill(context, 0), host_glue::ERR_STATE);
        assert_eq!(gray(&frame, 8, 8), 255, "nothing painted outside dispatch");
        // One draw dispatch with the Env and this frame.
        assert_eq!(
            bridge::dispatch_draw(&batch, 1, &depth, env, &mut frame.canvas()).unwrap(),
            host_glue::OK
        );
        assert_eq!(evidence.callback_markers.get(), 1);
        // Inside the rectangle black, outside white, to the pixel.
        for (x, y, want) in [
            (8, 8, 0),
            (71, 39, 0),
            (40, 20, 0),
            (7, 8, 255),
            (8, 7, 255),
            (72, 39, 255),
            (71, 40, 255),
            (0, 0, 255),
            (159, 79, 255),
        ] {
            assert_eq!(gray(&frame, x, y), want, "pixel ({x},{y})");
        }
        // The slot is closed again after the dispatch, whatever happened in it.
        assert_eq!(table_fill(context, 0), host_glue::ERR_STATE);
        // A second frame is drawn independently: the scope kept nothing from the first.
        let mut other = Framebuffer::new(WIDTH, HEIGHT, PixelFormat::Gray8).unwrap();
        other
            .canvas()
            .fill_rect(0, 0, u32::from(WIDTH), u32::from(HEIGHT), Color::WHITE);
        assert_eq!(
            bridge::dispatch_draw(&batch, 1, &depth, env, &mut other.canvas()).unwrap(),
            host_glue::OK
        );
        assert_eq!(gray(&other, 8, 8), 0);
        // An empty visible region sends no frame and calls nothing.
        let markers = evidence.callback_markers.get();
        let empty = paperpaw_platform::Rect::new(0, 0, WIDTH, 0);
        assert_eq!(
            bridge::dispatch_draw(&batch, 1, &depth, env, &mut other.canvas().clipped(empty))
                .unwrap(),
            host_glue::OK
        );
        assert_eq!(evidence.callback_markers.get(), markers);
        drop(batch);
        plugin.owner.as_ref().revoke();
        plugin.owner.as_ref().finish().unwrap();
        if let Some(path) = &shot {
            save_png(&frame, path);
            let info =
                png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()))
                    .read_info()
                    .unwrap();
            assert_eq!(
                (info.info().width, info.info().height),
                (u32::from(WIDTH), u32::from(HEIGHT))
            );
        }
        drop(plugin);
    });
}
