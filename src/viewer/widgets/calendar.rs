use chrono::{Datelike, NaiveDate, NaiveTime};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::viewer::fmt_duration;
use crate::viewer::index::Index;

const CELL: f32 = 30.0;
const GAP: f32 = 4.0;
/// Slack around the grid so the selected-day ring (which strokes ~2 px outside
/// the cell) is never clipped by the painter rect at the grid edges.
const MARGIN: f32 = 4.0;
const COLS: usize = 7;
const WEEKDAYS: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

#[allow(clippy::too_many_arguments)]
pub fn ui(
    ui: &mut Ui,
    index: &Index,
    selected_day: &mut Option<NaiveDate>,
    selected_shot: &mut Option<usize>,
    selected_range: &mut Option<(NaiveTime, NaiveTime)>,
    view_year: &mut i32,
    view_month: &mut u32,
) {
    ui.add_space(4.0);
    ui.heading("Blink");
    ui.add_space(4.0);

    ui.label(format!("Total: {}", fmt_duration(index.total_duration_s())));
    if let Some(day) = *selected_day {
        if let Some(d) = index.days.get(&day) {
            ui.label(format!("{}: {}", day, fmt_duration(d.total_duration_s())));
        }
    }
    ui.separator();

    nav_bar(ui, view_year, view_month);
    ui.add_space(6.0);

    // Heat normalisation: use the visible month's max so colours read well even
    // for months with mostly-light days.
    let max_day = month_max_duration(index, *view_year, *view_month).max(1);

    weekday_header(ui);
    draw_month(
        ui,
        *view_year,
        *view_month,
        index,
        max_day,
        selected_day,
        selected_shot,
        selected_range,
    );

    ui.add_space(8.0);
    if ui.button("Today").clicked() {
        let today = chrono::Local::now().date_naive();
        *view_year = today.year();
        *view_month = today.month();
        *selected_day = Some(today);
        *selected_shot = index
            .days
            .get(&today)
            .and_then(|d| if d.shots.is_empty() { None } else { Some(0) });
        *selected_range = None;
    }
}

fn nav_bar(ui: &mut Ui, year: &mut i32, month: &mut u32) {
    ui.horizontal(|ui| {
        if ui.small_button("«").on_hover_text("Previous year").clicked() {
            *year -= 1;
        }
        if ui.small_button("‹").on_hover_text("Previous month").clicked() {
            shift_month(year, month, -1);
        }
        let label = NaiveDate::from_ymd_opt(*year, *month, 1)
            .unwrap()
            .format("%B %Y")
            .to_string();
        ui.label(egui::RichText::new(label).strong());
        if ui.small_button("›").on_hover_text("Next month").clicked() {
            shift_month(year, month, 1);
        }
        if ui.small_button("»").on_hover_text("Next year").clicked() {
            *year += 1;
        }
    });
}

fn weekday_header(ui: &mut Ui) {
    let row_w = COLS as f32 * CELL + (COLS as f32 - 1.0) * GAP + 2.0 * MARGIN;
    let row_h = 14.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(row_w, row_h), Sense::hover());
    let painter = ui.painter_at(rect);
    for (i, name) in WEEKDAYS.iter().enumerate() {
        let cx = rect.min.x + MARGIN + i as f32 * (CELL + GAP) + CELL / 2.0;
        painter.text(
            Pos2::new(cx, rect.center().y),
            egui::Align2::CENTER_CENTER,
            *name,
            egui::FontId::proportional(11.0),
            Color32::from_gray(160),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_month(
    ui: &mut Ui,
    year: i32,
    month: u32,
    index: &Index,
    max_day: u64,
    selected_day: &mut Option<NaiveDate>,
    selected_shot: &mut Option<usize>,
    selected_range: &mut Option<(NaiveTime, NaiveTime)>,
) {
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let pad = first.weekday().num_days_from_monday() as usize;
    let days_in_month = days_in_month(year, month) as usize;

    let rows = (pad + days_in_month).div_ceil(COLS);
    let grid_w = COLS as f32 * CELL + (COLS as f32 - 1.0) * GAP;
    let grid_h = rows as f32 * CELL + (rows as f32 - 1.0) * GAP;
    let total = Vec2::new(grid_w + 2.0 * MARGIN, grid_h + 2.0 * MARGIN);

    let (rect, _) = ui.allocate_exact_size(total, Sense::hover());
    let painter = ui.painter_at(rect);
    let today = chrono::Local::now().date_naive();

    for d in 1..=days_in_month {
        let idx = pad + d - 1;
        let row = idx / COLS;
        let col = idx % COLS;
        let x = rect.min.x + MARGIN + col as f32 * (CELL + GAP);
        let y = rect.min.y + MARGIN + row as f32 * (CELL + GAP);
        let cell = Rect::from_min_size(Pos2::new(x, y), Vec2::splat(CELL));
        let date = NaiveDate::from_ymd_opt(year, month, d as u32).unwrap();
        let dur = index
            .days
            .get(&date)
            .map(|x| x.total_duration_s())
            .unwrap_or(0);
        let bg = if dur == 0 {
            Color32::from_gray(40)
        } else {
            heat(dur as f32 / max_day as f32)
        };
        painter.rect_filled(cell, 4.0, bg);

        // Day number in the cell, colour chosen for contrast against bg.
        painter.text(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            d.to_string(),
            egui::FontId::proportional(12.0),
            text_on(bg),
        );

        if Some(date) == *selected_day {
            painter.rect_stroke(
                cell.expand(2.0),
                5.0,
                Stroke::new(2.0, Color32::WHITE),
            );
        } else if date == today {
            painter.rect_stroke(
                cell,
                4.0,
                Stroke::new(1.0, Color32::from_rgb(120, 200, 255)),
            );
        }

        let resp = ui.interact(cell, ui.id().with(("cal", year, month, d)), Sense::click());
        if resp.clicked() {
            *selected_day = Some(date);
            *selected_shot = index
                .days
                .get(&date)
                .and_then(|d| if d.shots.is_empty() { None } else { Some(0) });
            *selected_range = None;
        }
        if resp.hovered() {
            resp.on_hover_text(format!("{}\n{}", date, fmt_duration(dur)));
        }
    }
}

fn shift_month(year: &mut i32, month: &mut u32, delta: i32) {
    let total = *year * 12 + (*month as i32 - 1) + delta;
    *year = total.div_euclid(12);
    *month = total.rem_euclid(12) as u32 + 1;
}

fn month_max_duration(index: &Index, year: i32, month: u32) -> u64 {
    index
        .days
        .iter()
        .filter(|(d, _)| d.year() == year && d.month() == month)
        .map(|(_, idx)| idx.total_duration_s())
        .max()
        .unwrap_or(0)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .unwrap();
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    (next - first).num_days() as u32
}

/// Hand-coded viridis-like ramp; avoids pulling a colormap dep.
fn heat(t: f32) -> Color32 {
    const STOPS: [(f32, [u8; 3]); 6] = [
        (0.00, [68, 1, 84]),
        (0.20, [72, 35, 116]),
        (0.40, [64, 67, 135]),
        (0.60, [41, 120, 142]),
        (0.80, [94, 201, 98]),
        (1.00, [253, 231, 37]),
    ];
    let t = t.clamp(0.0, 1.0);
    for w in STOPS.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t <= b.0 {
            let f = (t - a.0) / (b.0 - a.0);
            return Color32::from_rgb(
                lerp(a.1[0], b.1[0], f),
                lerp(a.1[1], b.1[1], f),
                lerp(a.1[2], b.1[2], f),
            );
        }
    }
    let last = STOPS[STOPS.len() - 1].1;
    Color32::from_rgb(last[0], last[1], last[2])
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

/// Pick a legible foreground for a given background using perceived luminance.
fn text_on(bg: Color32) -> Color32 {
    let [r, g, b, _] = bg.to_array();
    let l = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
    if l > 140.0 {
        Color32::from_rgb(20, 20, 20)
    } else {
        Color32::from_rgb(230, 230, 230)
    }
}
