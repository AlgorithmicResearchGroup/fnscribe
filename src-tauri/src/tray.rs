use crate::state::Phase;
use tauri::AppHandle;
use tauri::image::Image;

pub const TRAY_ID: &str = "fnscribe-tray";
const ICON_SIZE: usize = 18;

pub fn update(app: &AppHandle, phase: Phase) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(icon(phase)));
        let tooltip = match phase {
            Phase::Loading => "FnScribe — loading",
            Phase::Ready => "FnScribe — ready",
            Phase::Recording => "FnScribe — recording",
            Phase::Transcribing => "FnScribe — transcribing",
            Phase::Error => "FnScribe — attention needed",
        };
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

pub fn icon(phase: Phase) -> Image<'static> {
    let mut rgba = vec![0_u8; ICON_SIZE * ICON_SIZE * 4];
    let mut pixel = |x: usize, y: usize| {
        if x < ICON_SIZE && y < ICON_SIZE {
            let offset = (y * ICON_SIZE + x) * 4;
            rgba[offset] = 0;
            rgba[offset + 1] = 0;
            rgba[offset + 2] = 0;
            rgba[offset + 3] = 255;
        }
    };

    match phase {
        Phase::Recording => {
            for y in 4..14 {
                for x in 4..14 {
                    pixel(x, y);
                }
            }
        }
        Phase::Transcribing | Phase::Loading => {
            for center in [4_usize, 9, 14] {
                for y in 8..11 {
                    for x in center.saturating_sub(1)..=(center + 1).min(ICON_SIZE - 1) {
                        pixel(x, y);
                    }
                }
            }
        }
        Phase::Error => {
            for y in 3..12 {
                pixel(8, y);
                pixel(9, y);
            }
            for y in 14..16 {
                pixel(8, y);
                pixel(9, y);
            }
        }
        Phase::Ready => {
            // A compact microphone glyph optimized for an 18 px menu-bar icon.
            for y in 2..10 {
                for x in 7..11 {
                    pixel(x, y);
                }
            }
            for y in 7..12 {
                pixel(5, y);
                pixel(12, y);
            }
            for x in 6..12 {
                pixel(x, 12);
            }
            for y in 12..16 {
                pixel(8, y);
                pixel(9, y);
            }
            for x in 6..12 {
                pixel(x, 16);
            }
        }
    }

    Image::new_owned(rgba, ICON_SIZE as u32, ICON_SIZE as u32)
}
