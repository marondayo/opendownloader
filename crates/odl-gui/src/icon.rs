//! The app icon (a green rounded square with a white download arrow), drawn in code so the
//! window icon, the packaged PNGs and the Windows .ico all come from one source.
//! `cargo run -p odl-gui --example gen-icons` regenerates the files in `packaging/icons`.

/// RGBA pixels of the icon at `size`×`size`.
pub fn rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut out = vec![0u8; (size * size * 4) as usize];
    let (margin, radius) = (s * 0.06, s * 0.22);
    // 4×4 supersampling keeps small sizes (16 px) crisp.
    const SS: u32 = 4;
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let fx = x as f32 + (sx as f32 + 0.5) / SS as f32;
                    let fy = y as f32 + (sy as f32 + 0.5) / SS as f32;
                    let (lo, hi) = (margin + radius, s - margin - radius);
                    let dx = (lo - fx).max(fx - hi).max(0.0);
                    let dy = (lo - fy).max(fy - hi).max(0.0);
                    if (dx * dx + dy * dy).sqrt() > radius {
                        continue;
                    }
                    let t = fy / s;
                    let cx = s * 0.5;
                    let shaft = (fx - cx).abs() <= s * 0.075 && fy >= s * 0.2 && fy <= s * 0.5;
                    let head = fy >= s * 0.42 && fy <= s * 0.66 && (fx - cx).abs() <= (s * 0.66 - fy) * 0.95;
                    let base = fy >= s * 0.74 && fy <= s * 0.82 && fx >= s * 0.27 && fx <= s * 0.73;
                    let c = if shaft || head || base {
                        [255.0, 255.0, 255.0]
                    } else {
                        [104.0 - 30.0 * t, 196.0 - 40.0 * t, 84.0 - 24.0 * t]
                    };
                    acc[0] += c[0];
                    acc[1] += c[1];
                    acc[2] += c[2];
                    acc[3] += 1.0;
                }
            }
            if acc[3] == 0.0 {
                continue;
            }
            let i = ((y * size + x) * 4) as usize;
            let n = acc[3];
            out[i] = (acc[0] / n) as u8;
            out[i + 1] = (acc[1] / n) as u8;
            out[i + 2] = (acc[2] / n) as u8;
            out[i + 3] = (n / (SS * SS) as f32 * 255.0).round() as u8;
        }
    }
    out
}
