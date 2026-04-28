//! Procedural eye-icon rendering shared by the tray and the viewer window.
//!
//! The geometry is defined for a 32 px canvas and scaled linearly to whatever
//! `size` the caller asks for; this keeps tray (32 px) and viewer-window
//! (e.g. 128 px) icons visually identical.

/// Per-pixel byte order of the returned buffer.
pub enum ByteOrder {
    /// `[A, R, G, B]` per pixel — what ksni wants on the wire.
    Argb,
    /// `[R, G, B, A]` per pixel — what eframe / egui `IconData` wants.
    Rgba,
}

/// Active icon: open blue eye with a white pupil.
pub fn active(size: u32, order: ByteOrder) -> Vec<u8> {
    let scale = size as f32 / 32.0;
    let h = 8.0 * scale;
    let r_sq = (13.0 * scale).powi(2);
    let pupil_sq = (3.0 * scale).powi(2);
    render(size, order, move |dx, dy| {
        if dx * dx + dy * dy <= pupil_sq {
            return Some([255, 255, 255, 255]);
        }
        let d1 = dx * dx + (dy - h) * (dy - h);
        let d2 = dx * dx + (dy + h) * (dy + h);
        if d1 <= r_sq && d2 <= r_sq {
            Some([255, 60, 130, 220])
        } else {
            None
        }
    })
}

/// Paused icon: a closed-eye horizontal pill.
pub fn paused(size: u32, order: ByteOrder) -> Vec<u8> {
    let scale = size as f32 / 32.0;
    let half_w = 8.0 * scale;
    let half_h = 2.0 * scale;
    render(size, order, move |dx, dy| {
        let in_pill = if dx.abs() <= half_w {
            dy.abs() <= half_h
        } else {
            let ex = dx.abs() - half_w;
            ex * ex + dy * dy <= half_h * half_h
        };
        if in_pill {
            Some([255, 140, 140, 140])
        } else {
            None
        }
    })
}

fn render(
    size: u32,
    order: ByteOrder,
    pixel: impl Fn(f32, f32) -> Option<[u8; 4]>,
) -> Vec<u8> {
    let n = size as usize;
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let mut data = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            // Pixel shaders above always return ARGB-style [A, R, G, B].
            let argb = pixel(dx, dy).unwrap_or([0, 0, 0, 0]);
            let i = (y * n + x) * 4;
            match order {
                ByteOrder::Argb => data[i..i + 4].copy_from_slice(&argb),
                ByteOrder::Rgba => {
                    data[i] = argb[1];
                    data[i + 1] = argb[2];
                    data[i + 2] = argb[3];
                    data[i + 3] = argb[0];
                }
            }
        }
    }
    data
}
