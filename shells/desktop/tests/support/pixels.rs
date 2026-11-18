//! Shared snapshot PNG decoding and evidence output.
use std::{
    fs::{self, File},
    io::BufWriter,
    path::Path,
};

pub fn grays(path: &Path) -> (u32, u32, Vec<u8>) {
    let (width, height, gray, _) = pixels(path);
    (width, height, gray)
}

/// The gray of each pixel of a PNG, its size, and how many of its pixels
/// are in colour.
pub fn pixels(path: &Path) -> (u32, u32, Vec<u8>, usize) {
    let decoder = png::Decoder::new(std::io::BufReader::new(File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut data = vec![
        0;
        reader
            .output_buffer_size()
            .expect("a frame of a size to hold")
    ];
    let info = reader.next_frame(&mut data).unwrap();
    let data = &data[..info.buffer_size()];
    let colourful = match info.color_type {
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|p| p.iter().max().unwrap() - p.iter().min().unwrap() > 24)
            .count(),
        _ => 0,
    };
    let gray = match info.color_type {
        png::ColorType::Grayscale => data.to_vec(),
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| {
                ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000)
                    as u8
            })
            .collect(),
        other => panic!("{}: unexpected {other:?}", path.display()),
    };
    (info.width, info.height, gray, colourful)
}

pub fn write_gray(path: &Path, width: u32, height: u32, gray: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut encoder = png::Encoder::new(BufWriter::new(File::create(path).unwrap()), width, height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(gray).unwrap();
}
