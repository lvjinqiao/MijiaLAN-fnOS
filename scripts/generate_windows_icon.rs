//! Render the authoritative SVG into Windows icon resources with resvg 0.45.1.
//! Run with rustc --edition=2024 -L dependency=target/debug/deps
//! --extern resvg=target/debug/deps/libresvg-<hash>.rlib scripts/generate_windows_icon.rs.
use std::{fs, io::Write};
fn main() {
    let root = "crates/desktop/resources/windows";
    let svg = fs::read(format!("{root}/app-icon.svg")).unwrap();
    let tree = resvg::usvg::Tree::from_data(&svg, &Default::default()).unwrap();
    let sizes = [16u32, 20, 24, 32, 40, 48, 64, 128, 256];
    let mut images = Vec::new();
    for size in sizes {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(size as f32 / 64., size as f32 / 64.),
            &mut pixmap.as_mut(),
        );
        if size == 32 {
            let mut rgba = pixmap.data().to_vec();
            for pixel in rgba.chunks_exact_mut(4) {
                let alpha = u32::from(pixel[3]);
                if alpha > 0 {
                    for color in &mut pixel[..3] {
                        *color = (u32::from(*color) * 255 / alpha).min(255) as u8;
                    }
                }
            }
            fs::write(format!("{root}/app-tray.rgba"), rgba).unwrap();
        }
        let png = pixmap.encode_png().unwrap();
        if size == 64 {
            fs::write(format!("{root}/app-icon-preview.png"), &png).unwrap();
        }
        images.push((size, png));
    }
    let mut ico = fs::File::create(format!("{root}/app.ico")).unwrap();
    ico.write_all(&[0, 0, 1, 0]).unwrap();
    ico.write_all(&(sizes.len() as u16).to_le_bytes()).unwrap();
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (size, png) in &images {
        ico.write_all(&[*size as u8, *size as u8, 0, 0, 1, 0, 32, 0])
            .unwrap();
        ico.write_all(&(png.len() as u32).to_le_bytes()).unwrap();
        ico.write_all(&offset.to_le_bytes()).unwrap();
        offset += png.len() as u32;
    }
    for (_, png) in images {
        ico.write_all(&png).unwrap();
    }
}
