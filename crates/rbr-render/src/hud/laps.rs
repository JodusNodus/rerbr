//! The circuit session's lap panel (ours, `notes/session.md`), from the live lap record (`rbr_recording::laps`): the lap
//! in progress and its time, the last and best laps, and the sector just passed against the best lap's. A small panel
//! at the screen's left edge, under the stock HUD's top row.

use egui::{Align2, Color32, FontId, Pos2, Rect, Vec2};
use rbr_recording::laps::Laps;

use super::model::HudView;
use super::time_label;

const PANEL: Color32 = Color32::from_rgba_premultiplied(10, 10, 12, 190);
const TEXT: Color32 = Color32::from_gray(235);
const DIM: Color32 = Color32::from_gray(160);
const AHEAD: Color32 = Color32::from_rgb(90, 210, 110);
const BEHIND: Color32 = Color32::from_rgb(230, 90, 80);
/// The panel's top-left corner from the screen's, and its width.
const OFFSET: Vec2 = Vec2::new(16.0, 120.0);
const WIDTH: f32 = 210.0;

/// What a line's colour says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Dim,
    Ahead,
    Behind,
}

/// The panel's lines, from the top: the lap, its time, the sector just passed against the best lap's (none before the
/// first, or without a best lap to compare), the last and the best lap. Empty before the first lap starts.
pub fn lines(laps: &Laps) -> Vec<(String, Tone)> {
    if laps.lap == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let title = match (laps.laps, laps.finished) {
        (_, true) => "FINISHED".to_owned(),
        (0, false) => format!("LAP {}", laps.lap),
        (n, false) => format!("LAP {} / {n}", laps.lap),
    };
    lines.push((title, Tone::Dim));
    if !laps.finished {
        lines.push((time_label(f64::from(laps.elapsed)), Tone::Normal));
    }
    if let Some((index, time)) = laps.sectors.last().map(|&time| (laps.sectors.len() - 1, time)) {
        let mut line = format!("S{} {}", index + 1, time_label(f64::from(time)));
        let mut tone = Tone::Normal;
        if let Some(best) = laps.best_sectors.get(index) {
            let delta = time - best;
            line += &format!(" {delta:+.2}");
            tone = if delta <= 0.0 { Tone::Ahead } else { Tone::Behind };
        }
        lines.push((line, tone));
    }
    if laps.last > 0.0 {
        lines.push((format!("LAST {}", time_label(f64::from(laps.last))), Tone::Dim));
    }
    if laps.best > 0.0 {
        lines.push((format!("BEST {}", time_label(f64::from(laps.best))), Tone::Dim));
    }
    lines
}

/// Draws the panel over the HUD, on a circuit session.
pub fn paint(ctx: &egui::Context, state: &HudView) {
    let Some(laps) = &state.laps else { return };
    let lines = lines(laps);
    if lines.is_empty() {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("laps")));
    let screen = ctx.content_rect();
    let height = 12.0 + 22.0 * lines.len() as f32;
    let panel = Rect::from_min_size(screen.min + OFFSET, Vec2::new(WIDTH, height));
    painter.rect_filled(panel, 6.0, PANEL);
    for (k, (text, tone)) in lines.iter().enumerate() {
        let colour = match tone {
            Tone::Normal => TEXT,
            Tone::Dim => DIM,
            Tone::Ahead => AHEAD,
            Tone::Behind => BEHIND,
        };
        let size = if k == 1 { 22.0 } else { 15.0 };
        painter.text(
            Pos2::new(panel.min.x + 10.0, panel.min.y + 6.0 + 22.0 * k as f32),
            Align2::LEFT_TOP,
            text,
            FontId::proportional(size),
            colour,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laps() -> Laps {
        Laps {
            lap: 2,
            laps: 3,
            elapsed: 83.456,
            last: 445.25,
            best: 440.0,
            sectors: vec![150.0, 301.5],
            best_sectors: vec![149.0, 302.0],
            ..Laps::default()
        }
    }

    #[test]
    fn the_panel_shows_the_lap_its_time_the_sector_against_the_best_and_the_laps() {
        let lines = lines(&laps());
        let text: Vec<&str> = lines.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(text, ["LAP 2 / 3", "1:23.46", "S2 5:01.50 -0.50", "LAST 7:25.25", "BEST 7:20.00"]);
        assert_eq!(lines[2].1, Tone::Ahead);
    }

    #[test]
    fn a_sector_slower_than_the_best_is_behind_and_one_without_a_best_is_plain() {
        let slower = Laps { sectors: vec![151.0], ..laps() };
        assert_eq!(lines(&slower)[2], ("S1 2:31.00 +2.00".to_owned(), Tone::Behind));
        let first = Laps { best_sectors: Vec::new(), ..slower };
        assert_eq!(lines(&first)[2], ("S1 2:31.00".to_owned(), Tone::Normal));
    }

    #[test]
    fn free_practice_has_no_lap_count_and_the_end_says_finished() {
        let open = Laps { laps: 0, ..laps() };
        assert_eq!(lines(&open)[0].0, "LAP 2");
        let done = Laps { finished: true, ..laps() };
        assert_eq!(lines(&done)[0].0, "FINISHED");
        assert!(lines(&done).iter().all(|(t, _)| t != "1:23.46"));
    }

    #[test]
    fn nothing_shows_before_the_first_lap() {
        assert!(lines(&Laps::default()).is_empty());
    }
}
