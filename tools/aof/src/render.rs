use std::io::{self, Write};

use base64::Engine;
use image::ImageFormat;
use resvg::tiny_skia;
use resvg::usvg;
use snafu::{ResultExt, Snafu};

/// Maximum payload bytes per Kitty graphics protocol chunk. The protocol
/// specifies that escape sequences carrying image data may not exceed
/// 4096 bytes of base64 payload per chunk; longer images are split into
/// multiple chunks with `m=1` on all but the last.
const KITTY_CHUNK_BYTES: usize = 4096;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum RenderError {
    #[snafu(display("failed to parse SVG: {source}"))]
    SvgParse { source: usvg::Error },

    #[snafu(display("pixmap allocation failed for {width}x{height}"))]
    PixmapAlloc { width: u32, height: u32 },

    #[snafu(display("failed to encode rasterized image as PNG: {source}"))]
    PngEncode { source: image::ImageError },

    #[snafu(display("failed to write Kitty graphics protocol to stdout: {source}"))]
    Display { source: io::Error },
}

/// Rasterize an SVG byte slice to an in-memory `DynamicImage` at the
/// SVG's natural pixel size. Split from `render_svg` so tests can
/// verify the rasterization pipeline without needing a real TTY.
pub fn rasterize(svg: &[u8]) -> Result<image::DynamicImage, RenderError> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).context(SvgParseSnafu)?;
    let size = tree.size().to_int_size();
    let (width, height) = (size.width(), size.height());

    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| PixmapAllocSnafu { width, height }.build())?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

    let rgba = image::RgbaImage::from_raw(width, height, pixmap.data().to_vec())
        .expect("pixmap buffer always matches width*height*4");
    Ok(image::DynamicImage::ImageRgba8(rgba))
}

/// Rasterize and display the SVG inline in the current terminal using
/// the Kitty graphics protocol (supported natively by Ghostty, Kitty,
/// and WezTerm).
pub fn render_svg(svg: &[u8]) -> Result<(), RenderError> {
    let img = rasterize(svg)?;
    let png = encode_png(&img)?;
    write_kitty_image(&mut io::stdout(), &png).context(DisplaySnafu)
}

fn encode_png(img: &image::DynamicImage) -> Result<Vec<u8>, RenderError> {
    let mut buf = Vec::new();
    img.write_to(&mut io::Cursor::new(&mut buf), ImageFormat::Png)
        .context(PngEncodeSnafu)?;
    Ok(buf)
}

/// Emit a complete Kitty graphics protocol "transmit and display" command
/// for `png_bytes`. The first chunk carries the format / action keys; all
/// subsequent chunks carry only `m=` (1 for more, 0 for final).
fn write_kitty_image(out: &mut impl Write, png_bytes: &[u8]) -> io::Result<()> {
    let payload = base64::engine::general_purpose::STANDARD.encode(png_bytes);
    let chunks: Vec<&str> = payload
        .as_bytes()
        .chunks(KITTY_CHUNK_BYTES)
        .map(|c| std::str::from_utf8(c).expect("base64 output is ascii"))
        .collect();

    for (i, chunk) in chunks.iter().enumerate() {
        let is_first = i == 0;
        let is_last = i == chunks.len() - 1;
        out.write_all(b"\x1b_G")?;
        if is_first {
            // f=100: payload is PNG. a=T: transmit and display.
            out.write_all(b"f=100,a=T,")?;
        }
        write!(out, "m={}", if is_last { 0 } else { 1 })?;
        out.write_all(b";")?;
        out.write_all(chunk.as_bytes())?;
        out.write_all(b"\x1b\\")?;
    }
    // Trailing newline so the cursor lands below the image rather than
    // overwriting its bottom row with the next prompt.
    out.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_kitty_image_emits_a_single_chunk_for_small_payloads() {
        let mut out = Vec::new();
        write_kitty_image(&mut out, b"tiny").unwrap();
        let s = std::str::from_utf8(&out).unwrap();
        // Exactly one ESC_G ... ESC\ envelope.
        assert_eq!(s.matches("\x1b_G").count(), 1);
        assert_eq!(s.matches("\x1b\\").count(), 1);
        // First (and only) chunk carries the f= and a= keys and m=0.
        assert!(s.contains("f=100,a=T,m=0;"));
    }

    #[test]
    fn write_kitty_image_chunks_large_payloads() {
        // A payload that base64-encodes to slightly more than one chunk.
        let raw = vec![0u8; (KITTY_CHUNK_BYTES * 3 / 4) + 1];
        let mut out = Vec::new();
        write_kitty_image(&mut out, &raw).unwrap();
        let s = std::str::from_utf8(&out).unwrap();
        // Two chunks: header chunk has m=1, final chunk has m=0.
        assert_eq!(s.matches("\x1b_G").count(), 2);
        assert!(s.contains("f=100,a=T,m=1;"));
        assert!(s.contains("\x1b_Gm=0;"));
    }
}
