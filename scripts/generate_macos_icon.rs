//! Render the existing application SVG into an iconset; use iconutil for ICNS.
//! Compile with the project's resvg 0.45.1 rlib, like generate_windows_icon.rs.
use std::{env, fs, path::Path};

fn main() {
    let args: Vec<_> = env::args().collect();
    assert_eq!(args.len(), 3, "Expected input.svg output.iconset");
    let tree =
        resvg::usvg::Tree::from_data(&fs::read(&args[1]).unwrap(), &Default::default()).unwrap();
    let output = Path::new(&args[2]);
    fs::create_dir_all(output).unwrap();
    for size in [16_u32, 32, 128, 256, 512] {
        for scale in [1_u32, 2] {
            let pixels = size * scale;
            let mut pixmap = resvg::tiny_skia::Pixmap::new(pixels, pixels).unwrap();
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::from_scale(
                    pixels as f32 / tree.size().width(),
                    pixels as f32 / tree.size().height(),
                ),
                &mut pixmap.as_mut(),
            );
            let suffix = if scale == 2 { "@2x" } else { "" };
            pixmap
                .save_png(output.join(format!("icon_{size}x{size}{suffix}.png")))
                .unwrap();
        }
    }
}
