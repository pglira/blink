use chrono::{NaiveDate, NaiveTime};
use eframe::egui::{self, Align, Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::viewer::index::Index;

const THUMB_W: f32 = 160.0;
const THUMB_H: f32 = 100.0;
const THUMB_GAP: f32 = 6.0;
/// Empty strip above the thumbnails so they don't sit flush against the
/// panel divisor / image view.
const TOP_MARGIN: f32 = 6.0;

#[allow(clippy::too_many_arguments)]
pub fn ui(
    ui: &mut Ui,
    index: &Index,
    selected_day: Option<NaiveDate>,
    selected_shot: &mut Option<usize>,
    selected_range: Option<(NaiveTime, NaiveTime)>,
    last_focused: &mut Option<(NaiveDate, usize)>,
) {
    let Some(day) = selected_day else {
        ui.add_space(8.0);
        ui.label("No day selected.");
        return;
    };
    let Some(day_idx) = index.days.get(&day) else {
        return;
    };

    ui.add_space(TOP_MARGIN);

    // Apply the timeline-range filter once and keep a compact list of
    // (original-index, shot) so the strip only paginates over what the
    // user is actually browsing.
    let visible: Vec<(usize, &crate::viewer::index::Shot)> = day_idx
        .shots
        .iter()
        .enumerate()
        .filter(|(_, s)| match selected_range {
            None => true,
            Some((a, b)) => {
                let t = s.time.time();
                t >= a && t <= b
            }
        })
        .collect();
    let n = visible.len();
    if n == 0 {
        ui.label("No shots in selection.");
        return;
    }

    let want_focus = selected_shot.map(|i| (day, i));
    let needs_scroll = want_focus.is_some() && want_focus != *last_focused;

    let item_w = THUMB_W + THUMB_GAP;

    egui::ScrollArea::horizontal()
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            // Reserve the full content width so the scrollbar reflects the
            // real number of items, not just what we paint this frame.
            let content_size = Vec2::new(n as f32 * item_w, THUMB_H);
            let (rect, _) = ui.allocate_exact_size(content_size, Sense::hover());

            // Determine which range of items intersects the visible viewport.
            // Bias by ±1 to cover items being scrolled into view.
            let first = (((viewport.min.x / item_w).floor() as i64) - 1)
                .max(0) as usize;
            let mut last = ((viewport.max.x / item_w).ceil() as usize + 1).min(n);
            let mut first = first.min(n);

            // If the selection just changed and the selected thumb is outside
            // the visible window, extend the rendered range to include it so
            // we have a Response to call scroll_to_me on.
            let selected_pos = selected_shot
                .and_then(|sel| visible.iter().position(|(i, _)| *i == sel));
            if needs_scroll {
                if let Some(p) = selected_pos {
                    first = first.min(p);
                    last = last.max(p + 1);
                }
            }

            for vi in first..last {
                let (i, shot) = visible[vi];
                let x = rect.min.x + vi as f32 * item_w;
                let thumb_rect = Rect::from_min_size(
                    Pos2::new(x, rect.min.y),
                    Vec2::new(THUMB_W, THUMB_H),
                );
                let uri = format!("file://{}", shot.png.display());
                let img = egui::Image::new(uri)
                    .fit_to_exact_size(Vec2::new(THUMB_W, THUMB_H))
                    .sense(Sense::click());
                let resp = ui.put(thumb_rect, img);

                let selected = *selected_shot == Some(i);
                if selected {
                    ui.painter().rect_stroke(
                        thumb_rect.shrink(1.5),
                        3.0,
                        Stroke::new(3.0, Color32::from_rgb(255, 220, 80)),
                    );
                    if needs_scroll {
                        resp.scroll_to_me(Some(Align::Center));
                    }
                }
                let resp = resp.on_hover_text(format!("{}", shot.time.format("%H:%M:%S")));
                if resp.clicked() {
                    *selected_shot = Some(i);
                }
            }
        });

    if needs_scroll {
        *last_focused = want_focus;
    }
}
