//! PSnippet logo: white "P" on a rounded blue square, drawn procedurally (no binary asset).
//! Shared by the tray icon, the window icon and (via build.rs) the .exe icon resource.

/// Accent blue (matches `zs_ui::ACCENT`).
pub const BLUE: [u8; 4] = [38, 100, 200, 255];
pub const GREY: [u8; 4] = [140, 140, 140, 255];

/// Coverage (0..=1) of the white "P" at unit coordinates (x, y in 0..1).
fn in_p(x: f32, y: f32) -> bool {
    let stem = (0.28..=0.40).contains(&x) && (0.17..=0.83).contains(&y);
    let top = (0.28..=0.52).contains(&x) && (0.17..=0.28).contains(&y);
    let mid = (0.28..=0.52).contains(&x) && (0.49..=0.60).contains(&y);
    let (dx, dy) = (x - 0.52, y - 0.385);
    let d2 = dx * dx + dy * dy;
    let ring = x >= 0.52 && (0.105 * 0.105..=0.215 * 0.215).contains(&d2);
    stem || top || mid || ring
}

fn in_bg(x: f32, y: f32) -> bool {
    // Rounded square with corner radius 0.22.
    let r = 0.22f32;
    let cx = x.clamp(r, 1.0 - r);
    let cy = y.clamp(r, 1.0 - r);
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

/// RGBA pixels (row-major, top-down) of the logo at `n`×`n`, 4×4 supersampled.
pub fn rgba(n: u32, bg: [u8; 4]) -> Vec<u8> {
    const SS: u32 = 4;
    let mut px = vec![0u8; (n * n * 4) as usize];
    for y in 0..n {
        for x in 0..n {
            let (mut bgc, mut fgc) = (0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (x * SS + sx) as f32 / (n * SS) as f32 + 0.5 / (n * SS) as f32;
                    let v = (y * SS + sy) as f32 / (n * SS) as f32 + 0.5 / (n * SS) as f32;
                    if in_bg(u, v) {
                        bgc += 1;
                        if in_p(u, v) {
                            fgc += 1;
                        }
                    }
                }
            }
            let total = SS * SS;
            let i = ((y * n + x) * 4) as usize;
            if bgc > 0 {
                let a = bgc as f32 / total as f32;
                let w = fgc as f32 / bgc as f32; // white share inside the covered area
                for c in 0..3 {
                    px[i + c] = (bg[c] as f32 * (1.0 - w) + 255.0 * w).round() as u8;
                }
                px[i + 3] = (a * 255.0).round() as u8;
            }
        }
    }
    px
}

/// Encode several sizes as an .ico (32-bit BMP/DIB entries — accepted by every Windows version).
#[allow(dead_code)] // used by build.rs only
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let mut out = vec![0, 0, 1, 0];
    out.extend((sizes.len() as u16).to_le_bytes());
    let mut images: Vec<Vec<u8>> = Vec::new();
    for &n in sizes {
        let rgba = rgba(n, BLUE);
        let mut d = Vec::new();
        d.extend(40u32.to_le_bytes()); // BITMAPINFOHEADER
        d.extend((n as i32).to_le_bytes());
        d.extend((2 * n as i32).to_le_bytes()); // height doubled (XOR + AND mask)
        d.extend(1u16.to_le_bytes());
        d.extend(32u16.to_le_bytes());
        d.extend([0u8; 24]);
        for y in (0..n).rev() {
            for x in 0..n {
                let i = ((y * n + x) * 4) as usize;
                d.extend([rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]); // BGRA
            }
        }
        let mask_row = n.div_ceil(32) * 4;
        d.extend(vec![0u8; (mask_row * n) as usize]); // alpha channel carries transparency
        images.push(d);
    }
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&n, d) in sizes.iter().zip(&images) {
        out.push(if n >= 256 { 0 } else { n as u8 });
        out.push(if n >= 256 { 0 } else { n as u8 });
        out.extend([0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((d.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += d.len() as u32;
    }
    for d in images {
        out.extend(d);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_has_transparent_corners_and_white_stem() {
        let n = 64;
        let px = rgba(n, BLUE);
        assert_eq!(px.len(), (n * n * 4) as usize);
        assert_eq!(px[3], 0, "corner is transparent");
        let i = ((32 * n + 22) * 4) as usize; // on the stem (x≈0.34, y=0.5)
        assert_eq!(&px[i..i + 3], &[255, 255, 255]);
        let j = ((32 * n + 56) * 4) as usize; // right side, background blue
        assert_eq!(&px[j..j + 4], &BLUE);
    }

    #[test]
    fn ico_header_is_valid() {
        let b = ico(&[16, 32]);
        assert_eq!(&b[..4], &[0, 0, 1, 0]);
        assert_eq!(u16::from_le_bytes([b[4], b[5]]), 2);
    }
}
