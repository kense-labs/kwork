//! Pure safe-Rust ISO/IEC 18004 QR Code (Byte mode, Versions 1–6, EC Level L) encoder
//! and crisp vector GPUI canvas renderer for LAN & mobile pairing links.

use gpui::{prelude::*, *};

/// Encoded square QR matrix (`true` = dark module).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QrMatrix {
    pub size: usize,
    pub modules: Vec<bool>,
}

// Version 1..=6 parameters for Error Correction Level L (total codewords, EC codewords per single block, byte capacity).
const VERSION_SPEC_L: [(usize, usize, usize); 6] = [
    (26, 7, 17),    // V1: 21x21
    (44, 10, 32),   // V2: 25x25
    (70, 15, 53),   // V3: 29x29
    (100, 20, 78),  // V4: 33x33
    (134, 26, 106), // V5: 37x37
    (172, 36, 134), // V6: 41x41 (note: V6-L uses 2 blocks of (68 data, 18 ec))
];

impl QrMatrix {
    pub fn get(&self, x: usize, y: usize) -> bool {
        x < self.size && y < self.size && self.modules[y * self.size + x]
    }

    /// Encode UTF-8 string up to 134 bytes into an ISO/IEC 18004 QR Code matrix.
    pub fn encode(payload: &str) -> Option<Self> {
        let bytes = payload.as_bytes();
        let ver_idx = VERSION_SPEC_L.iter().position(|(_, _, cap)| bytes.len() <= *cap)?;
        let version = ver_idx + 1;
        let (total_cw, ec_total, _) = VERSION_SPEC_L[ver_idx];
        let data_cw_len = total_cw - ec_total;

        // 1. Bitstream: Byte mode (0100) + 8-bit length + bytes + terminator + pad 0xEC/0x11
        let mut bits: Vec<bool> = Vec::with_capacity(data_cw_len * 8);
        push_bits(&mut bits, 0b0100, 4);
        push_bits(&mut bits, bytes.len() as u32, 8);
        for &b in bytes {
            push_bits(&mut bits, b as u32, 8);
        }
        let max_bits = data_cw_len * 8;
        let term = (max_bits - bits.len()).min(4);
        for _ in 0..term {
            bits.push(false);
        }
        while bits.len() % 8 != 0 {
            bits.push(false);
        }
        let mut data_cw = Vec::with_capacity(data_cw_len);
        for chunk in bits.chunks_exact(8) {
            let mut v = 0u8;
            for &bit in chunk {
                v = (v << 1) | u8::from(bit);
            }
            data_cw.push(v);
        }
        let pads = [0xECu8, 0x11u8];
        let mut pi = 0;
        while data_cw.len() < data_cw_len {
            data_cw.push(pads[pi % 2]);
            pi += 1;
        }

        // 2. Reed-Solomon EC & interleaving (V1..=5 have 1 block; V6-L has 2 blocks of 68 data + 18 EC)
        let final_cw = if version < 6 {
            let ec = rs_compute_remainder(&data_cw, ec_total);
            let mut all = data_cw;
            all.extend_from_slice(&ec);
            all
        } else {
            let b1 = &data_cw[..68];
            let b2 = &data_cw[68..];
            let ec1 = rs_compute_remainder(b1, 18);
            let ec2 = rs_compute_remainder(b2, 18);
            let mut all = Vec::with_capacity(total_cw);
            for i in 0..68 {
                all.push(b1[i]);
                all.push(b2[i]);
            }
            for i in 0..18 {
                all.push(ec1[i]);
                all.push(ec2[i]);
            }
            all
        };

        // 3. Matrix construction
        let side = 17 + version * 4;
        let mut modules = vec![false; side * side];
        let mut is_func = vec![false; side * side];

        let mut set_func = |x: usize, y: usize, dark: bool| {
            if x < side && y < side {
                modules[y * side + x] = dark;
                is_func[y * side + x] = true;
            }
        };

        // Timing patterns
        for i in 0..side {
            set_func(6, i, i % 2 == 0);
            set_func(i, 6, i % 2 == 0);
        }

        // 3 Finder patterns + separators
        for (fx, fy) in [(3isize, 3isize), ((side as isize) - 4, 3), (3, (side as isize) - 4)] {
            for dy in -4..=4isize {
                for dx in -4..=4isize {
                    let xx = fx + dx;
                    let yy = fy + dy;
                    if xx >= 0 && xx < side as isize && yy >= 0 && yy < side as isize {
                        let dist = dx.abs().max(dy.abs());
                        set_func(xx as usize, yy as usize, dist != 2 && dist != 4);
                    }
                }
            }
        }

        // Alignment pattern for V2..=6 (single center at (side - 7, side - 7))
        if version >= 2 {
            let cx = (side - 7) as isize;
            let cy = (side - 7) as isize;
            for dy in -2..=2isize {
                for dx in -2..=2isize {
                    let dist = dx.abs().max(dy.abs());
                    set_func((cx + dx) as usize, (cy + dy) as usize, dist != 1);
                }
            }
        }

        // Reserve format information areas + dark module
        set_func(8, side - 8, true);
        for i in 0..9 {
            if i != 6 {
                is_func[8 * side + i] = true;
                is_func[i * side + 8] = true;
            }
        }
        for i in 0..8 {
            is_func[8 * side + (side - 1 - i)] = true;
            is_func[(side - 1 - i) * side + 8] = true;
        }

        // 4. Zigzag data placement
        let mut cw_bits = Vec::with_capacity(final_cw.len() * 8 + 7);
        for b in final_cw {
            push_bits(&mut cw_bits, b as u32, 8);
        }
        // Remainder bits (7 bits for V2..=6, 0 for V1)
        if version >= 2 {
            for _ in 0..7 {
                cw_bits.push(false);
            }
        }

        let mut bit_idx = 0usize;
        let mut right = (side as isize) - 1;
        let mut upward = true;
        while right >= 1 {
            if right == 6 {
                right -= 1;
            }
            for step in 0..side {
                let y = if upward { side - 1 - step } else { step };
                for j in 0..2isize {
                    let x = (right - j) as usize;
                    if !is_func[y * side + x] {
                        let dark = cw_bits.get(bit_idx).copied().unwrap_or(false);
                        modules[y * side + x] = dark;
                        bit_idx += 1;
                    }
                }
            }
            upward = !upward;
            right -= 2;
        }

        // 5. Apply Mask 0 ((x + y) % 2 == 0) & write Format Information for (EC Level L = 01, Mask 0 = 000)
        for y in 0..side {
            for x in 0..side {
                if !is_func[y * side + x] && (x + y) % 2 == 0 {
                    modules[y * side + x] = !modules[y * side + x];
                }
            }
        }
        write_format_bits(&mut modules, side, 0b01, 0);

        Some(Self {
            size: side,
            modules,
        })
    }
}

fn push_bits(out: &mut Vec<bool>, val: u32, len: usize) {
    for i in (0..len).rev() {
        out.push(((val >> i) & 1) != 0);
    }
}

fn write_format_bits(modules: &mut [bool], side: usize, ec_bits: u32, mask: u32) {
    let data = (ec_bits << 3) | mask;
    let mut rem = data << 10;
    for i in (0..5).rev() {
        if ((rem >> (i + 10)) & 1) != 0 {
            rem ^= 0x537 << i;
        }
    }
    let bits = ((data << 10) | rem) ^ 0x5412;
    let bit = |i: usize| ((bits >> i) & 1) != 0;

    for i in 0..6 {
        modules[i * side + 8] = bit(i);
    }
    modules[7 * side + 8] = bit(6);
    modules[8 * side + 8] = bit(7);
    modules[8 * side + 7] = bit(8);
    for i in 9..15 {
        modules[8 * side + (14 - i)] = bit(i);
    }
    for i in 0..8 {
        modules[8 * side + (side - 1 - i)] = bit(i);
    }
    for i in 8..15 {
        modules[(side - 15 + i) * side + 8] = bit(i);
    }
}

fn gf_mul(mut x: u8, mut y: u8) -> u8 {
    let mut z = 0u8;
    for _ in 0..8 {
        if (y & 1) != 0 {
            z ^= x;
        }
        let hi = (x & 0x80) != 0;
        x <<= 1;
        if hi {
            x ^= 0x1D;
        }
        y >>= 1;
    }
    z
}

fn rs_compute_remainder(data: &[u8], degree: usize) -> Vec<u8> {
    let mut divisor = vec![0u8; degree];
    divisor[degree - 1] = 1;
    let mut root = 1u8;
    for _ in 0..degree {
        for j in 0..degree {
            divisor[j] = gf_mul(divisor[j], root);
            if j + 1 < degree {
                divisor[j] ^= divisor[j + 1];
            }
        }
        root = gf_mul(root, 0x02);
    }
    let mut result = vec![0u8; degree];
    for &b in data {
        let factor = b ^ result[0];
        result.rotate_left(1);
        result[degree - 1] = 0;
        for (r, &d) in result.iter_mut().zip(&divisor) {
            *r ^= gf_mul(d, factor);
        }
    }
    result
}

/// Render a crisp vector QR code card with quiet zone in pure GPUI.
pub fn render_qr_canvas(payload: &str, box_px: f32, fg: Hsla, bg: Hsla) -> AnyElement {
    let Some(qr) = QrMatrix::encode(payload) else {
        return div().size(px(box_px)).rounded(px(8.)).bg(bg).into_any_element();
    };
    let total_cells = (qr.size + 4) as f32;
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            window.paint_quad(fill(bounds, bg).corner_radii(px(8.)));
            let w = f32::from(bounds.size.width);
            let cell = (w / total_cells).max(1.0);
            let pad = (w - cell * (qr.size as f32)) * 0.5;
            for y in 0..qr.size {
                for x in 0..qr.size {
                    if qr.get(x, y) {
                        let rect = Bounds::new(
                            point(
                                bounds.origin.x + px(pad + (x as f32) * cell),
                                bounds.origin.y + px(pad + (y as f32) * cell),
                            ),
                            size(px(cell + 0.35), px(cell + 0.35)),
                        );
                        window.paint_quad(fill(rect, fg));
                    }
                }
            }
        },
    )
    .size(px(box_px))
    .flex_none()
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::QrMatrix;

    #[test]
    fn qr_encoder_builds_valid_finder_patterns_across_versions() {
        let short = QrMatrix::encode("http://127.0.0.1:4777?code=849201").expect("v2/v3 qr");
        assert!(short.size >= 25);
        // Top-left 7x7 finder corners & center must be dark
        assert!(short.get(0, 0));
        assert!(short.get(6, 0));
        assert!(short.get(3, 3));
        assert!(!short.get(1, 1));

        let long = QrMatrix::encode(
            "http://192.168.1.42:54321/guest/550e8400-e29b-41d4-a716-446655440000?token=abcdef123456",
        )
        .expect("v5/v6 qr");
        assert!(long.size <= 41);
        assert!(long.get(long.size - 1, 0));
    }
}
