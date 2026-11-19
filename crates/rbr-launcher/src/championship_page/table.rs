//! A table in a panel for the standings and a stage's times: a header, rows of painted cells (numbers in mono), the
//! player's row lit, scrolling when it is taller than the room.

use egui::{Color32, FontId, Rect, Sense, Ui, Vec2};

use crate::theme::*;

const ROW: f32 = 26.0;
const PAD: f32 = 10.0;

/// A column: its heading, width (0: the rest of the width) and alignment.
pub(super) struct Col {
    pub head: String,
    pub width: f32,
    pub right: bool,
}

impl Col {
    pub fn new(head: &str, width: f32, right: bool) -> Col {
        Col { head: head.to_owned(), width, right }
    }
}

/// A cell's text: mono for numbers.
pub(super) struct Cell {
    pub text: String,
    pub color: Color32,
    pub mono: bool,
}

impl Cell {
    pub fn text(text: impl Into<String>, color: Color32) -> Cell {
        Cell { text: text.into(), color, mono: false }
    }

    pub fn num(text: impl Into<String>, color: Color32) -> Cell {
        Cell { text: text.into(), color, mono: true }
    }
}

/// A row: its cells, whether it is the player's and the hover text.
pub(super) struct Line {
    pub cells: Vec<Cell>,
    pub you: bool,
    pub tip: Option<String>,
}

/// The table in a panel, its rows scrolling after `max_height` px.
pub(super) fn table(ui: &mut Ui, id: &str, cols: &[Col], lines: Vec<Line>, max_height: f32) {
    super::common::panel_frame().inner_margin(egui::Margin::symmetric(4, 6)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.set_width(ui.available_width());
        let total = ui.available_width();
        let fixed: f32 = cols.iter().map(|c| c.width).sum();
        let rest = (total - fixed - 2.0 * PAD).max(90.0);
        // Each column's left and right edge.
        let mut x = PAD;
        let edges: Vec<(f32, f32)> = cols
            .iter()
            .map(|c| {
                let w = if c.width > 0.0 { c.width } else { rest };
                let edge = (x, x + w);
                x += w;
                edge
            })
            .collect();
        let place = |ui: &mut Ui, rect: Rect, cells: Vec<(&str, Color32, FontId)>| {
            for ((c, (text, color, font)), (left, right)) in cols.iter().zip(cells).zip(&edges) {
                let clip = Rect::from_min_max(
                    egui::pos2(rect.left() + left, rect.top()),
                    egui::pos2(rect.left() + right - 4.0, rect.bottom()),
                );
                let painter = ui.painter().with_clip_rect(clip);
                let galley = painter.layout_no_wrap(text.to_owned(), font, color);
                let at = if c.right { clip.right() - galley.size().x } else { clip.left() };
                painter.galley(egui::pos2(at, rect.center().y - galley.size().y / 2.0), galley, color);
            }
        };
        if cols.iter().any(|c| !c.head.is_empty()) {
            let (head, _) = ui.allocate_exact_size(Vec2::new(total, 22.0), Sense::hover());
            let font = FontId::new(11.5, medium());
            place(ui, head, cols.iter().map(|c| (c.head.as_str(), TEXT_3, font.clone())).collect());
        }
        egui::ScrollArea::vertical().id_salt(id).max_height(max_height).auto_shrink([false, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for line in lines {
                let (rect, response) = ui.allocate_exact_size(Vec2::new(total, ROW), Sense::hover());
                if line.you {
                    ui.painter().rect_filled(rect, 5, ROW_SELECTED);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 5, white(8));
                }
                let cells = line
                    .cells
                    .iter()
                    .map(|c| {
                        let font = if c.mono { mono(12.0) } else { FontId::new(12.5, egui::FontFamily::Proportional) };
                        let font = if line.you && !c.mono { FontId::new(12.5, medium()) } else { font };
                        (c.text.as_str(), c.color, font)
                    })
                    .collect();
                place(ui, rect, cells);
                if let Some(tip) = &line.tip {
                    response.on_hover_text(tip);
                }
            }
        });
    });
}
