use chrono::{NaiveDate, NaiveTime};
use eframe::egui::{self, Color32, Sense, Stroke, Ui, Vec2};

use crate::viewer::index::Index;

const THUMB_W: f32 = 160.0;
const THUMB_H: f32 = 100.0;

pub fn ui(
    ui: &mut Ui,
    index: &Index,
    selected_day: Option<NaiveDate>,
    selected_shot: &mut Option<usize>,
    selected_range: Option<(NaiveTime, NaiveTime)>,
) {
    let Some(day) = selected_day else {
        ui.add_space(8.0);
        ui.label("No day selected.");
        return;
    };
    let Some(day_idx) = index.days.get(&day) else {
        return;
    };

    egui::ScrollArea::horizontal()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, shot) in day_idx.shots.iter().enumerate() {
                    if let Some((a, b)) = selected_range {
                        let t = shot.time.time();
                        if t < a || t > b {
                            continue;
                        }
                    }
                    let selected = *selected_shot == Some(i);
                    let uri = format!("file://{}", shot.png.display());
                    let resp = ui.add(
                        egui::Image::new(uri)
                            .fit_to_exact_size(Vec2::new(THUMB_W, THUMB_H))
                            .sense(Sense::click()),
                    );
                    if selected {
                        // Draw the ring INSIDE the thumbnail so it isn't
                        // clipped by the scroll area at the strip's edges.
                        ui.painter().rect_stroke(
                            resp.rect.shrink(1.5),
                            3.0,
                            Stroke::new(3.0, Color32::from_rgb(255, 220, 80)),
                        );
                    }
                    let resp = resp.on_hover_text(format!("{}", shot.time.format("%H:%M:%S")));
                    if resp.clicked() {
                        *selected_shot = Some(i);
                    }
                }
            });
        });
}
