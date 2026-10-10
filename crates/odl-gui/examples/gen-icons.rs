//! Regenerates the packaged icons from `src/icon.rs`:
//! `cargo run -p odl-gui --example gen-icons` → packaging/icons/*.png and opendownloader.ico

#[path = "../src/icon.rs"]
mod icon;

use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/icons");
    std::fs::create_dir_all(&out)?;
    for size in [16u32, 24, 32, 48, 64, 128, 256, 512] {
        let path = out.join(format!("{size}x{size}.png"));
        image::save_buffer(&path, &icon::rgba(size), size, size, image::ColorType::Rgba8)?;
        println!("wrote {}", path.display());
    }
    // Windows icon with the usual sizes.
    let frames = [16u32, 24, 32, 48, 64, 128, 256]
        .iter()
        .map(|&s| image::codecs::ico::IcoFrame::as_png(&icon::rgba(s), s, s, image::ExtendedColorType::Rgba8))
        .collect::<Result<Vec<_>, _>>()?;
    let ico = out.join("opendownloader.ico");
    image::codecs::ico::IcoEncoder::new(std::fs::File::create(&ico)?).encode_images(&frames)?;
    println!("wrote {}", ico.display());
    Ok(())
}
