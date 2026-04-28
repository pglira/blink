use chrono::NaiveDate;
use eframe::egui::{self, Ui};

use crate::viewer::index::Index;

pub fn ui(
    ui: &mut Ui,
    index: &Index,
    selected_day: Option<NaiveDate>,
    selected_shot: Option<usize>,
) {
    let Some(day) = selected_day else {
        ui.centered_and_justified(|ui| {
            ui.label("Pick a day from the calendar to begin.");
        });
        return;
    };
    let Some(day_idx) = index.days.get(&day) else {
        ui.centered_and_justified(|ui| {
            ui.label("No screenshots for this day.");
        });
        return;
    };
    let Some(idx) = selected_shot.filter(|i| *i < day_idx.shots.len()) else {
        ui.centered_and_justified(|ui| {
            ui.label("Click a thumbnail or a tick on the timeline.");
        });
        return;
    };
    let shot = &day_idx.shots[idx];

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(shot.time.format("%Y-%m-%d %H:%M:%S").to_string()).strong());
        ui.label(format!("({} of {})", idx + 1, day_idx.shots.len()));
        ui.label("· arrow keys to step");
    });
    ui.separator();

    let avail = ui.available_size();
    ui.centered_and_justified(|ui| {
        let uri = format!("file://{}", shot.png.display());
        ui.add(
            egui::Image::new(uri)
                .max_size(avail)
                .maintain_aspect_ratio(true),
        );
    });
}
