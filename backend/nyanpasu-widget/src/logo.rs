//! Image logos: decoded once, then rasterized at the physical size of the
//! badge, so the renderer draws them pixel for pixel instead of scaling them.

use std::io::Cursor;

use fast_image_resize::{
    FilterType, MulDiv, PixelType, ResizeAlg, ResizeOptions, Resizer, images::Image,
};
use slint::{Rgba8Pixel, SharedPixelBuffer};
use snafu::{ResultExt as _, Snafu};

/// How strongly [`rasterize`] sharpens a minified logo: the weight of the
/// detail an unsharp mask adds back. Lanczos3 already keeps edges; this
/// counters the softness left when 192 px artwork lands on 28 to 88 px.
const SHARPEN_AMOUNT: f32 = 0.35;

#[derive(Debug, Snafu)]
pub(crate) enum LogoError {
    #[snafu(display("could not decode the logo image"))]
    Decode { source: png::DecodingError },
    #[snafu(display("the logo image has no pixels"))]
    Empty,
}

/// Decodes a PNG into premultiplied RGBA, the form both resizing and Slint's
/// renderer need.
pub(crate) fn decode(bytes: &[u8]) -> Result<SharedPixelBuffer<Rgba8Pixel>, LogoError> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().context(DecodeSnafu)?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or(LogoError::Empty)?];
    let info = reader.next_frame(&mut buffer).context(DecodeSnafu)?;
    let pixels = &buffer[..info.buffer_size()];
    let (width, height) = (info.width, info.height);
    if width == 0 || height == 0 {
        return Err(LogoError::Empty);
    }

    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(pixels),
        png::ColorType::Rgb => {
            for pixel in pixels.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in pixels.as_chunks::<2>().0 {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        // Palettes are expanded by the transformations above.
        png::ColorType::Grayscale | png::ColorType::Indexed => {
            for &value in pixels {
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
        }
    }

    let mut image = Image::from_vec_u8(width, height, rgba, PixelType::U8x4)
        .expect("the buffer holds width × height RGBA pixels");
    MulDiv::default()
        .multiply_alpha_inplace(&mut image)
        .expect("U8x4 supports alpha multiplication");
    Ok(SharedPixelBuffer::clone_from_slice(
        image.buffer(),
        width,
        height,
    ))
}

/// Scales `source` (premultiplied) to fit a `size` × `size` square, centred
/// on transparency, with Lanczos3, and sharpens it when it was minified.
pub(crate) fn rasterize(
    source: &SharedPixelBuffer<Rgba8Pixel>,
    size: u32,
) -> SharedPixelBuffer<Rgba8Pixel> {
    let size = size.max(1);
    let (src_width, src_height) = (source.width(), source.height());
    let scale = f64::from(size) / f64::from(src_width.max(src_height));
    let width = ((f64::from(src_width) * scale).round() as u32).clamp(1, size);
    let height = ((f64::from(src_height) * scale).round() as u32).clamp(1, size);

    let src = fast_image_resize::images::ImageRef::new(
        src_width,
        src_height,
        source.as_bytes(),
        PixelType::U8x4,
    )
    .expect("the buffer holds width × height RGBA pixels");
    let mut scaled = Image::new(width, height, PixelType::U8x4);
    // The source is already premultiplied, so the resizer must not multiply
    // it again.
    let options = ResizeOptions::new()
        .resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))
        .use_alpha(false);
    Resizer::new()
        .resize(&src, &mut scaled, &options)
        .expect("both images are U8x4");
    let mut scaled = scaled.into_vec();
    if scale < 1.0 {
        sharpen(&mut scaled, width as usize, height as usize, SHARPEN_AMOUNT);
    }

    let mut square = SharedPixelBuffer::<Rgba8Pixel>::new(size, size);
    let (left, top) = ((size - width) / 2, (size - height) / 2);
    let row_bytes = width as usize * 4;
    let target = square.make_mut_bytes();
    for (y, row) in scaled.chunks_exact(row_bytes).enumerate() {
        let start = ((top as usize + y) * size as usize + left as usize) * 4;
        target[start..start + row_bytes].copy_from_slice(row);
    }
    square
}

/// An unsharp mask with a 3 × 3 Gaussian blur on premultiplied RGBA: adds
/// `amount` times the difference to the blurred image back, then clamps the
/// colour channels to alpha so the pixels stay validly premultiplied.
fn sharpen(pixels: &mut [u8], width: usize, height: usize, amount: f32) {
    const KERNEL: [[u32; 3]; 3] = [[1, 2, 1], [2, 4, 2], [1, 2, 1]];
    let source = pixels.to_vec();
    let at = |x: usize, y: usize, channel: usize| u32::from(source[(y * width + x) * 4 + channel]);
    for y in 0..height {
        for x in 0..width {
            let mut out = [0f32; 4];
            for (channel, value) in out.iter_mut().enumerate() {
                let mut blurred = 0;
                for (dy, row) in KERNEL.iter().enumerate() {
                    for (dx, weight) in row.iter().enumerate() {
                        // Edge pixels repeat outward.
                        let sx = (x + dx).saturating_sub(1).min(width - 1);
                        let sy = (y + dy).saturating_sub(1).min(height - 1);
                        blurred += weight * at(sx, sy, channel);
                    }
                }
                let original = at(x, y, channel) as f32;
                let blurred = blurred as f32 / 16.0;
                *value = (original + amount * (original - blurred)).clamp(0.0, 255.0);
            }
            let alpha = out[3].round();
            let pixel = &mut pixels[(y * width + x) * 4..][..4];
            for channel in 0..3 {
                pixel[channel] = out[channel].round().min(alpha) as u8;
            }
            pixel[3] = alpha as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(width: u32, height: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(data)
            .unwrap();
        bytes
    }

    #[test]
    fn decoding_premultiplies_alpha_and_expands_other_colour_types() {
        let rgba = decode(&encode(1, 1, png::ColorType::Rgba, &[200, 100, 50, 128])).unwrap();
        let pixel = rgba.as_slice()[0];
        assert_eq!(pixel.a, 128);
        // 200 × 128 / 255, rounded either way.
        assert!((100..=101).contains(&pixel.r), "{pixel:?}");

        let gray = decode(&encode(1, 1, png::ColorType::Grayscale, &[77])).unwrap();
        let pixel = gray.as_slice()[0];
        assert_eq!((pixel.r, pixel.g, pixel.b, pixel.a), (77, 77, 77, 255));
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(decode(b"\x89PNG\r\n\x1a\nnot an image").is_err());
    }

    #[test]
    fn a_wide_logo_is_centred_in_a_transparent_square() {
        // 4 × 2 opaque red, fitted into 8 × 8: 8 × 4 in rows 2 to 5.
        let source = SharedPixelBuffer::clone_from_slice(&[255, 0, 0, 255].repeat(8), 4, 2);
        let square = rasterize(&source, 8);
        assert_eq!((square.width(), square.height()), (8, 8));
        let alpha = |x: usize, y: usize| square.as_slice()[y * 8 + x].a;
        assert_eq!(alpha(4, 0), 0);
        assert_eq!(alpha(4, 1), 0);
        assert_eq!(alpha(4, 3), 255);
        assert_eq!(alpha(4, 6), 0);
    }

    #[test]
    fn minified_pixels_stay_premultiplied() {
        // A checkerboard of opaque white and transparent pixels rings hardest
        // under Lanczos3 and the unsharp mask.
        let mut data = Vec::new();
        for y in 0..64 {
            for x in 0..64 {
                data.extend_from_slice(if (x / 4 + y / 4) % 2 == 0 {
                    &[255, 255, 255, 255]
                } else {
                    &[0, 0, 0, 0]
                });
            }
        }
        let source = SharedPixelBuffer::clone_from_slice(&data, 64, 64);
        for pixel in rasterize(&source, 21).as_slice() {
            assert!(pixel.r <= pixel.a && pixel.g <= pixel.a && pixel.b <= pixel.a);
        }
    }

    #[test]
    fn sharpening_raises_the_contrast_of_an_edge() {
        // One row: dark, dark, light, light.
        let mut pixels = [
            [60, 60, 60, 255],
            [60, 60, 60, 255],
            [200, 200, 200, 255],
            [200, 200, 200, 255],
        ]
        .concat();
        sharpen(&mut pixels, 4, 1, SHARPEN_AMOUNT);
        assert!(pixels[4] < 60, "the dark side of the edge darkens");
        assert!(pixels[8] > 200, "the light side of the edge lightens");
        assert_eq!(pixels[7], 255, "opaque stays opaque");
    }
}
