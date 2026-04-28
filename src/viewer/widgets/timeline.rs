use chrono::{NaiveDate, NaiveTime, Timelike};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::viewer::fmt_duration;
use crate::viewer::index::{DayIndex, Index};

pub fn ui(
    ui: &mut Ui,
    index: &Index,
    selected_day: Option<NaiveDate>,
    selected_shot: &mut Option<usize>,
    selected_range: &mut Option<(NaiveTime, NaiveTime)>,
) {
    let Some(day) = selected_day else {
        ui.add_space(8.0);
        ui.label("Select a day from the calendar to see its timeline.");
        return;
    };
    let Some(day_idx) = index.days.get(&day) else {
        ui.add_space(8.0);
        ui.label("No screenshots this day.");
        return;
    };

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(day.to_string()).strong());
        ui.label(format!(
            "· {} shots · {}",
            day_idx.shots.len(),
            fmt_duration(day_idx.total_duration_s())
        ));
        if let Some((a, b)) = *selected_range {
            let count = day_idx
                .shots
                .iter()
                .filter(|s| {
                    let t = s.time.time();
                    t >= a && t <= b
                })
                .count();
            ui.label(format!(
                "· selection {}–{} · {} shots",
                a.format("%H:%M"),
                b.format("%H:%M"),
                count
            ));
            if ui.small_button("clear").clicked() {
                *selected_range = None;
            }
        }
    });

    let avail = ui.available_width();
    let height = 36.0;
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(avail, height), Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 3.0, Color32::from_gray(28));

    // Hour grid + labels. Tick density adapts to width so labels never
    // collide: ~50 px is enough for "HH:00".
    let label_step = pick_label_step(rect.width());
    for h in 0..=24 {
        let x = rect.min.x + (h as f32 / 24.0) * rect.width();
        let major = h % 6 == 0;
        painter.line_segment(
            [Pos2::new(x, rect.max.y - 8.0), Pos2::new(x, rect.max.y)],
            Stroke::new(
                1.0,
                if major {
                    Color32::from_gray(150)
                } else {
                    Color32::from_gray(70)
                },
            ),
        );
        if h < 24 && h % label_step == 0 {
            painter.text(
                Pos2::new(x + 3.0, rect.min.y + 1.0),
                egui::Align2::LEFT_TOP,
                format!("{h:02}:00"),
                egui::FontId::monospace(10.0),
                Color32::from_gray(170),
            );
        }
    }

    // Range overlay drawn under the ticks so ticks remain visible.
    if let Some((a, b)) = *selected_range {
        let xa = rect.min.x + time_frac(a) * rect.width();
        let xb = rect.min.x + time_frac(b) * rect.width();
        let sel_rect = Rect::from_min_max(
            Pos2::new(xa.min(xb), rect.min.y + 14.0),
            Pos2::new(xa.max(xb), rect.max.y - 8.0),
        );
        painter.rect_filled(
            sel_rect,
            0.0,
            Color32::from_rgba_unmultiplied(120, 200, 255, 50),
        );
    }

    for (i, shot) in day_idx.shots.iter().enumerate() {
        let f = time_frac(shot.time.time());
        let x = rect.min.x + f * rect.width();
        let selected = *selected_shot == Some(i);
        let (color, w) = if selected {
            (Color32::from_rgb(255, 220, 80), 2.5)
        } else {
            (Color32::from_rgb(120, 200, 255), 1.5)
        };
        painter.line_segment(
            [Pos2::new(x, rect.min.y + 14.0), Pos2::new(x, rect.max.y - 8.0)],
            Stroke::new(w, color),
        );
    }

    // Hover tooltip → nearest shot's timestamp.
    if let Some(pos) = resp.hover_pos() {
        if let Some(i) = nearest_shot(rect, pos.x, day_idx) {
            let shot = &day_idx.shots[i];
            resp.clone()
                .on_hover_text(format!("{}", shot.time.format("%H:%M:%S")));
        }
    }

    // Drag → range select. Tiny drags collapse to a click → select shot.
    if resp.drag_started() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let t = frac_to_time(((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0));
            *selected_range = Some((t, t));
        }
    }
    if resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let t = frac_to_time(((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0));
            if let Some((anchor, _)) = *selected_range {
                *selected_range = Some((anchor.min(t), anchor.max(t)));
            }
        }
    }
    if resp.drag_stopped() {
        if let Some((a, b)) = *selected_range {
            let xa = time_frac(a) * rect.width();
            let xb = time_frac(b) * rect.width();
            if (xb - xa).abs() < 3.0 {
                *selected_range = None;
                if let Some(pos) = resp.interact_pointer_pos() {
                    if let Some(i) = nearest_shot(rect, pos.x, day_idx) {
                        *selected_shot = Some(i);
                    }
                }
            }
        }
    } else if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if let Some(i) = nearest_shot(rect, pos.x, day_idx) {
                *selected_shot = Some(i);
                *selected_range = None;
            }
        }
    }
}

/// Pick the smallest "every N hours" label step (from {1, 2, 3, 6}) that
/// keeps adjacent labels at least ~50 px apart, so they don't overlap.
fn pick_label_step(width: f32) -> u32 {
    const MIN_PX_PER_LABEL: f32 = 52.0;
    let px_per_hour = width / 24.0;
    for &step in &[1u32, 2, 3, 6] {
        if px_per_hour * step as f32 >= MIN_PX_PER_LABEL {
            return step;
        }
    }
    6
}

fn time_frac(t: NaiveTime) -> f32 {
    t.num_seconds_from_midnight() as f32 / 86_400.0
}

fn frac_to_time(f: f32) -> NaiveTime {
    let secs = (f * 86_400.0).clamp(0.0, 86_399.0) as u32;
    NaiveTime::from_num_seconds_from_midnight_opt(secs, 0).unwrap()
}

fn nearest_shot(rect: Rect, x: f32, day_idx: &DayIndex) -> Option<usize> {
    if day_idx.shots.is_empty() {
        return None;
    }
    let target = ((x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
    let mut best = (f32::INFINITY, 0usize);
    for (i, shot) in day_idx.shots.iter().enumerate() {
        let d = (time_frac(shot.time.time()) - target).abs();
        if d < best.0 {
            best = (d, i);
        }
    }
    Some(best.1)
}
