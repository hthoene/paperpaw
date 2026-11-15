//! A word drawn where the host's own way of drawing a word is not what the
//! reading settings ask: letters spaced, pairs not kerned, or letters joined
//! into a ligature. The word is drawn in pieces, each one run of the host, and
//! the pen is put where the layout measured the letters (`native_typeset`), so
//! that the line is as wide as it was laid out.
use super::Ink;
use crate::native_typeset::{Faces, Measure, Role, small_capital};
use alloc::string::String;
use paperpaw_sdk::{
    layout::{Face, Typesetter},
    native::Error,
};

/// The piece being collected: where it starts, its size and its text.
struct Piece {
    text: String,
    size: f32,
    from: f32,
}

/// `word` in `face` drawn from `x` on `baseline`, apart from how the host
/// would draw it at once; its width.
///
/// # Errors
/// What `ink` refuses, `NoMemory` for the piece.
pub(super) fn draw_apart<M: Measure>(
    ink: &mut impl Ink,
    faces: &Faces<M>,
    face: Face,
    (x, baseline): (f32, i32),
    word: &str,
) -> Result<f32, Error> {
    let role = Role::of(face);
    let (small, spacing) = (face.style.small_caps(), faces.spacing(face));
    let mut piece = Piece {
        text: String::new(),
        size: 0.0,
        from: x,
    };
    piece
        .text
        .try_reserve(word.len().saturating_mul(2))
        .map_err(|_| Error::NoMemory)?;
    let mut pen = x;
    let mut before: Option<char> = None;
    let mut at = 0;
    while let Some(c) = word.get(at..).and_then(|rest| rest.chars().next()) {
        if paperpaw_sdk::text::is_invisible(c) {
            at += c.len_utf8();
            continue;
        }
        // What the layout adds between this letter and the one before.
        let kern = before.map_or(0.0, |left| faces.kern(face, left, c));
        let joined = match faces.joins(face) && c == 'f' && !small {
            true => word
                .get(at..)
                .and_then(|rest| faces.ligature_at(face, rest)),
            false => None,
        };
        if let Some((letter, len, width)) = joined {
            pen = flush(ink, role, &mut piece, baseline, pen)? + kern;
            let mut buffer = [0u8; 4];
            let text = &*letter.encode_utf8(&mut buffer);
            ink.run(role, faces.size(face), pen, baseline, text)?;
            pen += width;
            before = word.get(at..at + len).and_then(|s| s.chars().next_back());
            at += len;
            continue;
        }
        let (shown, size) = small_capital(c, small, faces.size(face));
        // The host kerns what one run holds: a letter goes on in the piece where
        // its pair is kerned as the layout set it, else it starts another.
        let same = !piece.text.is_empty()
            && spacing.to_bits() == 0
            && piece.size.to_bits() == size.to_bits()
            && before.is_some_and(|left| (faces.font_kern(face, left, c) - kern).abs() < 1e-3);
        if !same {
            pen = flush(ink, role, &mut piece, baseline, pen)? + kern;
            piece.from = pen;
            piece.size = size;
        }
        piece
            .text
            .try_reserve(shown.len_utf8())
            .map_err(|_| Error::NoMemory)?;
        piece.text.push(shown);
        if spacing.to_bits() != 0 {
            // Each letter alone: the space after it, as the layout has it.
            let advanced = flush(ink, role, &mut piece, baseline, pen)?;
            pen = advanced + spacing;
        }
        before = Some(c);
        at += c.len_utf8();
    }
    pen = flush(ink, role, &mut piece, baseline, pen)?;
    Ok(pen - x)
}

/// Draws the piece, if there is one, and gives the pen after it (`pen` if none).
fn flush(
    ink: &mut impl Ink,
    role: Role,
    piece: &mut Piece,
    baseline: i32,
    pen: f32,
) -> Result<f32, Error> {
    if piece.text.is_empty() {
        return Ok(pen);
    }
    let advance = ink.run(role, piece.size, piece.from, baseline, &piece.text)?;
    piece.text.clear();
    Ok(piece.from + advance)
}
