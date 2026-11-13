//! Minimal 24-bit BMP encoder for `--screenshot` (no image crate dependency).

use std::io::Write;
use std::path::Path;

/// Encode RGBA pixels (row-major, top-down) as a 24-bit bottom-up BMP.
pub fn encode_bmp(width: usize, height: usize, rgba: &[[u8; 4]]) -> Vec<u8> {
    assert_eq!(rgba.len(), width * height, "pixel count mismatch");
    let row = (width * 3 + 3) & !3;
    let data_len = row * height;
    let file_len = 54 + data_len;
    let mut out = Vec::with_capacity(file_len);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_len as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for y in (0..height).rev() {
        let start = out.len();
        for p in &rgba[y * width..(y + 1) * width] {
            out.extend_from_slice(&[p[2], p[1], p[0]]);
        }
        out.resize(start + row, 0);
    }
    out
}

pub fn save_color_image(path: &Path, image: &egui::ColorImage) -> std::io::Result<()> {
    let px: Vec<[u8; 4]> = image.pixels.iter().map(|c| c.to_array()).collect();
    let bytes = encode_bmp(image.size[0], image.size[1], &px);
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::File::create(path)?.write_all(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmp_header_and_padding() {
        let px = vec![[10, 20, 30, 255]; 3 * 2];
        let b = encode_bmp(3, 2, &px);
        assert_eq!(&b[..2], b"BM");
        // row = 9 bytes padded to 12; 2 rows
        assert_eq!(b.len(), 54 + 24);
        assert_eq!(&b[54..57], &[30, 20, 10]);
    }
}
