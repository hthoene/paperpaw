//! A freestanding loaded plugin draws through core's actual shared host table.
use super::super::dev::Loaded;
use super::{environment::DEVICE, foreign::fixture_custom};
use paperpaw_core::native_host::{
    DispatchDepth,
    registrations::{self, Registration},
};
use paperpaw_core::sdk::{
    Env, Font, FontSpec, Id, Manifest, Plugin, Priority, RegisterError, Registrar, Version, builtin,
};
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{Color, Instant, PixelFormat, Rect};
use std::{borrow::Cow, cell::Cell, rc::Rc};

const WIDTH: u16 = 96;
const HEIGHT: u16 = 40;
const CLIP: Rect = Rect::new(9, 4, 24, 24);

const DRAW: &str = r#"
    let key = b"role:ui";
    let text = b"PaperPaw";
    let mut font = 0;
    let result = unsafe { (h.font_acquire)(s.ctx, PpBytes { ptr: key.as_ptr(), len: key.len() }, &mut font) };
    if result != OK { return result; }
    let (mut ascent, mut descent, mut height) = (-1.0, -1.0, -1.0);
    let result = unsafe { (h.text_metrics)(s.ctx, font, 16.0, &mut ascent, &mut descent, &mut height) };
    if result != OK { return result; }
    if ascent <= 0.0 || descent < 0.0 || height <= 0.0 { return ERR_INVALID; }
    let mut expected_width = -1.0;
    let text = PpBytes { ptr: text.as_ptr(), len: text.len() };
    let result = unsafe { (h.text_measure)(s.ctx, font, 16.0, text, &mut expected_width) };
    if result != OK { return result; }
    let result = unsafe { (h.canvas_clip)(s.ctx, 9, 4, 24, 24) };
    if result != OK { return result; }
    let mut drawn_width = -1.0;
    let result = unsafe { (h.text_draw)(s.ctx, font, 16.0, 2.0, 22, 0, text, &mut drawn_width) };
    if result != OK { return result; }
    if drawn_width != expected_width { return ERR_INVALID; }
    let result = unsafe { (h.handle_release)(s.ctx, font) };
    if result != OK { return result; }
"#;

type Run = Rc<dyn Fn(&Env<'_>)>;
struct Renderer(Run);

impl Plugin for Renderer {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("render-fixture"),
            "Render fixture",
            Version::new(1, 0, 0),
        )
    }

    fn register(&self, registrar: &mut Registrar) -> Result<(), RegisterError> {
        let data: &'static [u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../plugins/fonts-default/fonts/SourceSans3.ttf"
        ));
        let font = Font::from_data(Cow::Borrowed(data), &[]).unwrap();
        registrar.add_font(FontSpec::new(Id::new_static("render-fixture.face"), font))?;
        registrar.bind_font(
            builtin::fonts::UI,
            Id::new_static("render-fixture.face"),
            Priority::DEFAULT,
        )?;
        let run = Rc::clone(&self.0);
        registrar.subscribe(
            builtin::events::STARTED,
            Box::new(move |env, _, _| run(env)),
        )
    }
}

#[test]
fn loaded_plugin_draws_text_with_metrics_and_clip_through_shared_table() {
    let mut wire = vec![0; registrations::REGISTRATION_MAX_BYTES];
    let length =
        registrations::registrations_encode(&[Registration::Draw { token: 17 }], &mut wire)
            .unwrap();
    wire.truncate(length);
    let fixture = Rc::new(fixture_custom(wire, &[], false, DRAW, ""));
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let run = Rc::new(move |env: &Env<'_>| {
        let library = std::fs::read(&fixture.path).unwrap();
        let loaded = Loaded::from_bytes("render-fixture", &library).unwrap();
        let mut actual = Framebuffer::new(WIDTH, HEIGHT, PixelFormat::Gray8).unwrap();
        let mut expected = Framebuffer::new(WIDTH, HEIGHT, PixelFormat::Gray8).unwrap();
        for frame in [&mut actual, &mut expected] {
            frame
                .canvas()
                .fill_rect(0, 0, u32::from(WIDTH), u32::from(HEIGHT), Color::WHITE);
        }
        assert_eq!(
            loaded
                .draw_fixture(0, &DispatchDepth::new(), env, &mut actual.canvas())
                .unwrap(),
            0
        );
        let font = env.fonts().role(builtin::fonts::UI.as_str()).unwrap();
        font.draw(
            &mut expected.canvas().clipped(CLIP),
            16.0,
            2.0,
            22,
            "PaperPaw",
            Color::BLACK,
        );
        assert_eq!(actual.view().data, expected.view().data);
        assert!(
            actual.view().data.iter().any(|pixel| *pixel < 128),
            "real text was rasterized"
        );
        if let Some(path) = std::env::var_os("PAPERPAW_ABI7_RENDER_FIXTURE") {
            super::canvas::save_png(&actual, std::path::Path::new(&path));
        }
        observed.set(observed.get() + 1);
    });
    let mut kernel = paperpaw_core::Kernel::builder(DEVICE)
        .plugin(Renderer(run))
        .build()
        .unwrap();
    let _effects = kernel.start(Instant(17));
    assert_eq!(calls.get(), 1);
}
