use std::sync::Arc;

use chrono::{Datelike, NaiveDate, NaiveTime};
use eframe::egui::{self, Context};

use crate::viewer::index::Index;
use crate::viewer::widgets;

pub struct ViewerApp {
    pub index: Arc<Index>,
    pub selected_day: Option<NaiveDate>,
    pub selected_shot: Option<usize>,
    pub selected_range: Option<(NaiveTime, NaiveTime)>,
    /// Which month the calendar is currently displaying.
    pub view_year: i32,
    pub view_month: u32,
    /// Last (day, shot index) the thumbnail strip auto-scrolled to. Used to
    /// trigger `scroll_to_me` only on actual selection changes, so the user's
    /// manual scrolling isn't fought every frame.
    pub last_focused: Option<(NaiveDate, usize)>,
}

impl ViewerApp {
    pub fn new(index: Index) -> Self {
        let selected_day = index.days.keys().next_back().copied();
        let selected_shot = selected_day
            .as_ref()
            .and_then(|d| index.days.get(d))
            .and_then(|d| if d.shots.is_empty() { None } else { Some(d.shots.len() - 1) });
        let anchor = selected_day.unwrap_or_else(|| chrono::Local::now().date_naive());
        Self {
            index: Arc::new(index),
            selected_day,
            selected_shot,
            selected_range: None,
            view_year: anchor.year(),
            view_month: anchor.month(),
            last_focused: None,
        }
    }
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Outermost: timeline along the bottom (full window width).
        egui::TopBottomPanel::bottom("timeline")
            .resizable(false)
            .min_height(64.0)
            .show(ctx, |ui| {
                widgets::timeline::ui(
                    ui,
                    &self.index,
                    self.selected_day,
                    &mut self.selected_shot,
                    &mut self.selected_range,
                );
            });

        // Calendar + totals on the left.
        egui::SidePanel::left("calendar")
            .resizable(true)
            .default_width(260.0)
            .show(ctx, |ui| {
                widgets::calendar::ui(
                    ui,
                    &self.index,
                    &mut self.selected_day,
                    &mut self.selected_shot,
                    &mut self.selected_range,
                    &mut self.view_year,
                    &mut self.view_month,
                );
            });

        // Thumbnails along the bottom of the remaining (right) area.
        egui::TopBottomPanel::bottom("thumbs")
            .resizable(true)
            .default_height(140.0)
            .min_height(80.0)
            .show(ctx, |ui| {
                widgets::thumbs::ui(
                    ui,
                    &self.index,
                    self.selected_day,
                    &mut self.selected_shot,
                    self.selected_range,
                    &mut self.last_focused,
                );
            });

        // Image view fills the rest.
        egui::CentralPanel::default().show(ctx, |ui| {
            widgets::image_view::ui(ui, &self.index, self.selected_day, self.selected_shot);
        });

        self.handle_keys(ctx);
    }
}

impl ViewerApp {
    fn handle_keys(&mut self, ctx: &Context) {
        let (prev, next) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
            )
        });
        if !(prev || next) {
            return;
        }
        let Some(day) = self.selected_day else { return };
        let Some(day_idx) = self.index.days.get(&day) else { return };
        let n = day_idx.shots.len();
        if n == 0 {
            return;
        }
        let cur = self.selected_shot.unwrap_or(0);
        let new = if next {
            (cur + 1).min(n - 1)
        } else {
            cur.saturating_sub(1)
        };
        self.selected_shot = Some(new);
    }
}
