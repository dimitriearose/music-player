//! Album art, drawn with text characters.
//!
//! Terminal.app can't show real images, but it can colour each character
//! cell's foreground and background. The character `▀` ("upper half block")
//! fills the top half of a cell with the foreground colour, leaving the
//! bottom half in the background colour. So one cell = two pixels stacked
//! vertically, and a 20×20-pixel image fits in 20 columns × 10 rows, which
//! looks square because cells are about twice as tall as they are wide.

use image::imageops::FilterType;
use ratatui::style::Color;

/// Width and height of the art in pixels (= columns; rows are half this).
pub const ART_SIZE: u32 = 20;

/// A small, square, decoded image: `size × size` RGB pixels, row by row.
#[derive(Debug, Clone, PartialEq)]
pub struct Art {
    pub size: u32,
    /// `[u8; 3]` is an array of three bytes: red, green, blue (0–255 each).
    pub pixels: Vec<[u8; 3]>,
}

impl Art {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        self.pixels[(y * self.size + x) as usize]
    }
}

/// Decode JPEG/PNG bytes and shrink them to `size × size`. `None` if the
/// bytes aren't an image we understand.
pub fn decode(bytes: &[u8], size: u32) -> Option<Art> {
    let img = image::load_from_memory(bytes).ok()?;
    // `Triangle` averages neighbouring pixels while shrinking, which keeps
    // the colours of a 600×600 cover looking right at 20×20.
    let small = img.resize_exact(size, size, FilterType::Triangle).to_rgb8();
    let pixels = small.pixels().map(|p| p.0).collect();
    Some(Art { size, pixels })
}

/// The cover's main colour, made bright enough to read on a dark
/// background: used to tint the progress bar. `None` for a grey cover.
///
/// How: every pixel votes for its colour, but vivid pixels count far more
/// than dull ones (weight = saturation²), so a small splash of red on a
/// grey cover still wins over the grey. Then we scale the result up until
/// its brightest channel is at least 200 (of 255).
pub fn accent(art: &Art) -> Option<[u8; 3]> {
    let (mut sum, mut total) = ([0.0f64; 3], 0.0f64);
    for p in &art.pixels {
        let max = *p.iter().max().unwrap() as f64;
        let min = *p.iter().min().unwrap() as f64;
        if max < 30.0 {
            continue; // near-black: no real colour to speak of
        }
        let saturation = (max - min) / max;
        let weight = saturation * saturation;
        for c in 0..3 {
            sum[c] += p[c] as f64 * weight;
        }
        total += weight;
    }
    // Not enough colour anywhere (black-and-white or grey cover).
    if total < art.pixels.len() as f64 * 0.02 {
        return None;
    }
    let avg = sum.map(|v| v / total);
    let brightest = avg.iter().cloned().fold(1.0, f64::max);
    let scale = (200.0 / brightest).max(1.0);
    Some(avg.map(|v| (v * scale).min(255.0) as u8))
}

/// Does this terminal understand full 24-bit colour? Terminals that do
/// usually say so in the `COLORTERM` environment variable.
pub fn truecolor_supported() -> bool {
    std::env::var("COLORTERM")
        .map(|v| v.contains("truecolor") || v.contains("24bit"))
        .unwrap_or(false)
}

/// The colour to draw an RGB pixel with: exact if the terminal supports it,
/// otherwise the closest of the standard 256 terminal colours.
pub fn color(rgb: [u8; 3], truecolor: bool) -> Color {
    if truecolor {
        Color::Rgb(rgb[0], rgb[1], rgb[2])
    } else {
        Color::Indexed(rgb_to_256(rgb))
    }
}

/// Nearest colour in the xterm 256-colour palette.
///
/// That palette has a 6×6×6 colour cube (indexes 16–231) whose levels are
/// 0, 95, 135, 175, 215, 255, plus 24 greys (232–255). We find the nearest
/// in each and keep whichever is closer.
pub fn rgb_to_256(rgb: [u8; 3]) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let nearest_level = |v: u8| {
        (0..6)
            .min_by_key(|&i| (LEVELS[i] as i32 - v as i32).abs())
            .unwrap_or(0)
    };
    let (r, g, b) = (
        nearest_level(rgb[0]),
        nearest_level(rgb[1]),
        nearest_level(rgb[2]),
    );
    let cube_index = 16 + 36 * r + 6 * g + b;
    let cube_rgb = [LEVELS[r], LEVELS[g], LEVELS[b]];

    // Greys are 8, 18, 28, ..., 238.
    let avg = (rgb[0] as u32 + rgb[1] as u32 + rgb[2] as u32) / 3;
    let grey_step = ((avg.saturating_sub(8) + 5) / 10).min(23) as u8;
    let grey = 8 + 10 * grey_step;
    let grey_index = 232 + grey_step as usize;

    if distance(rgb, [grey, grey, grey]) < distance(rgb, cube_rgb) {
        grey_index as u8
    } else {
        cube_index as u8
    }
}

/// Squared distance between two colours (no square root needed just to
/// compare which is closer).
fn distance(a: [u8; 3], b: [u8; 3]) -> i32 {
    (0..3).map(|i| (a[i] as i32 - b[i] as i32).pow(2)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn art_of(pixels: Vec<[u8; 3]>) -> Art {
        let size = (pixels.len() as f64).sqrt() as u32;
        Art { size, pixels }
    }

    #[test]
    fn accent_finds_the_vivid_colour_and_brightens_it() {
        // Mostly grey, with a quarter dark red.
        let mut pixels = vec![[120, 120, 120]; 12];
        pixels.extend(vec![[100, 10, 10]; 4]);
        let [r, g, b] = accent(&art_of(pixels)).unwrap();
        assert!(r >= 200, "brightened: {r}");
        assert!(g < 60 && b < 60, "still red: {g} {b}");
    }

    #[test]
    fn grey_or_black_covers_have_no_accent() {
        assert_eq!(accent(&art_of(vec![[90, 90, 90]; 16])), None);
        assert_eq!(accent(&art_of(vec![[5, 0, 10]; 16])), None);
    }

    #[test]
    fn pure_colours_map_to_the_cube_corners() {
        assert_eq!(rgb_to_256([0, 0, 0]), 16);
        assert_eq!(rgb_to_256([255, 255, 255]), 231);
        assert_eq!(rgb_to_256([255, 0, 0]), 196);
        assert_eq!(rgb_to_256([0, 0, 255]), 21);
    }

    #[test]
    fn mid_greys_use_the_grey_ramp() {
        let i = rgb_to_256([128, 128, 128]);
        assert!((232..=255).contains(&i), "got {i}");
    }

    #[test]
    fn decodes_and_shrinks_a_png() {
        // Build a 4×4 red PNG in memory, then decode it at 2×2.
        let img = image::RgbImage::from_pixel(4, 4, image::Rgb([200, 10, 10]));
        let mut bytes = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
        let art = decode(&bytes, 2).unwrap();
        assert_eq!(art.pixels.len(), 4);
        assert_eq!(art.pixel(1, 1), [200, 10, 10]);
    }

    #[test]
    fn rejects_non_images() {
        assert_eq!(decode(b"not an image", 2), None);
    }
}
