//! The shared widgets of the redesign: page header, row panels, segmented control, switch, buttons, chips, footer
//! and modal. Colours, fonts and the egui style are `theme.rs`; a page uses these and plain egui widgets (dropdowns
//! and selectable rows get their look from the theme).
//!
//! A restyled page is `page_header`, then (when it has a primary action) `footer`, then `body` around its content:
//! the footer must be added before the body, which takes what is left.

use egui::{
    Align, Color32, CornerRadius, Frame, Layout, Margin, Rect, Response, RichText, Sense, Stroke, Ui, Vec2, Widget,
};

use crate::theme::{self, *};

/// The padding of a page's content (the left edge is the title's).
pub const PAGE_LEFT: i8 = 28;
pub const PAGE_RIGHT: i8 = 20;

fn line(color: Color32) -> Stroke {
    Stroke::new(1.0, color)
}

/// A page's content with the page's side padding (a restyled page's body; `page_header` and `footer` have their own).
pub fn body<R>(ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> R {
    let margin = Margin { left: PAGE_LEFT, right: PAGE_RIGHT, top: 0, bottom: 8 };
    Frame::new().inner_margin(margin).show(ui, content).inner
}

/// The page's title (22, semibold), an optional subtitle under it, and `actions` right-aligned (add them in
/// reading order from the right: the first added is the rightmost).
pub fn page_header(ui: &mut Ui, title: &str, subtitle: Option<&str>, actions: impl FnOnce(&mut Ui)) {
    let margin = Margin { left: PAGE_LEFT, right: PAGE_RIGHT, top: 22, bottom: 16 };
    Frame::new().inner_margin(margin).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.label(RichText::new(title).font(theme::title()).color(TEXT));
                if let Some(subtitle) = subtitle {
                    ui.label(RichText::new(subtitle).color(TEXT_2));
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), actions);
        });
    });
}

/// A caption above a panel (12, medium) with an optional right-aligned link; true when the link was clicked.
pub fn group_label(ui: &mut Ui, text: &str, link_text: Option<&str>) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(text).size(12.0).family(medium()).color(TEXT_2));
        if let Some(link_text) = link_text {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| clicked = link(ui, link_text).clicked());
        }
    });
    ui.add_space(2.0);
    clicked
}

/// A hint under a panel or a field (12, dimmed, wrapping).
pub fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(TEXT_3));
}

/// `label` over `under` (12, `color`, wrapping) with a row's padding above and below, `reserve` px kept free on the
/// right for the row's controls.
pub fn label_over(ui: &mut Ui, label: &str, under: &str, color: Color32, reserve: f32) {
    ui.vertical(|ui| {
        ui.set_max_width((ui.available_width() - reserve).max(40.0));
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add_space(8.0);
        ui.label(RichText::new(label).color(TEXT_LABEL));
        ui.label(RichText::new(under).size(12.0).color(color));
        ui.add_space(8.0);
    });
}

/// The rows of a [`panel`].
pub struct Rows<'a> {
    ui: &'a mut Ui,
    /// Nothing is above the next row's divider.
    first: bool,
}

/// A panel of grouped rows (fill, border, radius 10), full width. Add rows with [`Rows::row`], [`Rows::full`] and
/// [`Rows::hint`]; the dividers between them are drawn for you.
pub fn panel<R>(ui: &mut Ui, rows: impl FnOnce(&mut Rows) -> R) -> R {
    let frame = Frame::new().fill(PANEL).stroke(line(LINE)).corner_radius(10);
    frame
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.set_width(ui.available_width());
            rows(&mut Rows { ui, first: true })
        })
        .inner
}

impl Rows<'_> {
    /// The divider above a row, but not the first.
    fn divider(&mut self) {
        if !self.first {
            let rect = self.ui.available_rect_before_wrap();
            self.ui.painter().hline(rect.x_range(), rect.top(), line(LINE));
        }
        self.first = false;
    }

    /// The rows `content` adds, dimmed and inert while `disabled`.
    pub fn disabled_if<R>(&mut self, disabled: bool, content: impl FnOnce(&mut Rows) -> R) -> R {
        let first = self.first;
        let (result, first) = self
            .ui
            .scope(|ui| {
                ui.add_enabled_ui(!disabled, |ui| {
                    let mut rows = Rows { ui, first };
                    (content(&mut rows), rows.first)
                })
                .inner
            })
            .inner;
        self.first = first;
        result
    }

    /// A row of ~42 px: `label` on the left (#c9cacd), `control` right-aligned. The control's widgets are added
    /// right to left: the first added is the rightmost.
    pub fn row<R>(&mut self, label: &str, control: impl FnOnce(&mut Ui) -> R) -> R {
        self.full(|ui| {
            ui.label(RichText::new(label).color(TEXT_LABEL));
            ui.with_layout(Layout::right_to_left(Align::Center), control).inner
        })
    }

    /// A row of the panel's full width: `content` has the row's padding (14 px sides, at least 42 px high), laid out
    /// left to right and centred.
    pub fn full<R>(&mut self, content: impl FnOnce(&mut Ui) -> R) -> R {
        self.sized(42.0, content)
    }

    /// [`Rows::full`] at least `height` high, for a denser list.
    pub fn sized<R>(&mut self, height: f32, content: impl FnOnce(&mut Ui) -> R) -> R {
        self.divider();
        let width = self.ui.available_width();
        self.ui
            .allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::left_to_right(Align::Center), |ui| {
                ui.set_min_height(height);
                ui.set_width(width);
                Frame::new()
                    .inner_margin(Margin::symmetric(14, 0))
                    .show(ui, |ui| {
                        ui.set_width(width - 28.0);
                        ui.horizontal_centered(content).inner
                    })
                    .inner
            })
            .inner
    }

    /// A row with a switch on the right: `label`, and under it `hint` (12, dimmed, wrapping) when there is one. The
    /// response is the switch's (`changed()` when toggled; hover text goes on it).
    pub fn switch_row(&mut self, label: &str, hint: Option<&str>, on: &mut bool) -> Response {
        let Some(hint) = hint else { return self.row(label, |ui| switch(ui, on)) };
        self.full(|ui| {
            label_over(ui, label, hint, TEXT_3, 34.0 + 16.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| switch(ui, on)).inner
        })
    }

    /// A hint line under the row above, inside the panel.
    pub fn hint(&mut self, text: &str) {
        self.first = false;
        Frame::new().inner_margin(Margin { left: 14, right: 14, top: 0, bottom: 10 }).show(self.ui, |ui| {
            ui.set_width(ui.available_width());
            hint(ui, text);
        });
    }
}

/// Text on a painted pill, `colors` giving its fill and text colour from the response; used by the segments, the
/// chips and the tags.
fn pill(
    ui: &mut Ui,
    text: &str,
    font: egui::FontId,
    pad: Vec2,
    radius: u8,
    sense: Sense,
    colors: impl FnOnce(&Response) -> (Color32, Color32),
) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, Color32::WHITE);
    let (rect, response) = ui.allocate_exact_size(galley.size() + 2.0 * pad, sense);
    if ui.is_rect_visible(rect) {
        let (fill, color) = colors(&response);
        ui.painter().rect_filled(rect, CornerRadius::same(radius), fill);
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    response
}

/// A segmented control: a track of segments, the selected one raised. For a choice and for tabs. The response is
/// `changed()` when another segment was clicked.
pub fn segmented<T: PartialEq + Clone>(
    ui: &mut Ui,
    id: impl egui::AsIdSalt,
    value: &mut T,
    options: &[(T, &str)],
) -> Response {
    let mut changed = false;
    // A horizontal inside a right-to-left row lays out right to left: reversed, the segments read in order.
    let mut order: Vec<&(T, &str)> = options.iter().collect();
    if ui.layout().prefer_right_to_left() {
        order.reverse();
    }
    let mut response = ui
        .push_id(id, |ui| {
            Frame::new()
                .fill(TRACK)
                .corner_radius(7)
                .inner_margin(2)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (option, label) in order {
                            let selected = option == value;
                            let font = egui::FontId::new(
                                12.5,
                                if selected { medium() } else { egui::FontFamily::Proportional },
                            );
                            let segment = pill(ui, label, font, Vec2::new(12.0, 5.0), 5, Sense::click(), |r| {
                                match (selected, r.hovered()) {
                                    (true, _) => (SEGMENT, Color32::WHITE),
                                    (false, true) => (white(10), TEXT),
                                    (false, false) => (Color32::TRANSPARENT, TEXT_SOFT),
                                }
                            });
                            if segment.clicked() && !selected {
                                *value = option.clone();
                                changed = true;
                            }
                        }
                    });
                })
                .response
        })
        .inner;
    if changed {
        response.mark_changed();
    }
    response
}

/// A switch (34×20); the response is `changed()` when toggled.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    // Inert and dimmed (the painter's opacity) in a disabled `Ui`, as egui's own widgets.
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(34.0, 20.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let off = SWITCH_OFF;
        let fill = Color32::from_rgb(
            egui::lerp(off.r() as f32..=ACCENT.r() as f32, t) as u8,
            egui::lerp(off.g() as f32..=ACCENT.g() as f32, t) as u8,
            egui::lerp(off.b() as f32..=ACCENT.b() as f32, t) as u8,
        );
        ui.painter().rect_filled(rect, 10, fill);
        let x = egui::lerp((rect.left() + 10.0)..=(rect.right() - 10.0), t);
        ui.painter().circle_filled(egui::pos2(x, rect.center().y), 8.0, Color32::WHITE);
    }
    response
}

/// The primary action's button ([`primary_button`]).
pub struct PrimaryButton<'a> {
    label: &'a str,
    hint: Option<&'a str>,
    min_width: f32,
}

impl PrimaryButton<'_> {
    /// At least this wide (the default is 132).
    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = width;
        self
    }
}

/// The page's main action: accent, 14/600, at least 132×38, with an optional hint (a key such as "↵") after the
/// label. Add it with `ui.add` or `ui.add_enabled`.
pub fn primary_button<'a>(label: &'a str, hint: Option<&'a str>) -> PrimaryButton<'a> {
    PrimaryButton { label, hint, min_width: 132.0 }
}

impl Widget for PrimaryButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let painter = ui.painter();
        let main = painter.layout_no_wrap(self.label.to_owned(), egui::FontId::new(14.0, semibold()), Color32::WHITE);
        let hint = self.hint.map(|h| {
            painter.layout_no_wrap(
                h.to_owned(),
                egui::FontId::new(12.0, egui::FontFamily::Proportional),
                Color32::WHITE,
            )
        });
        let hint_width = hint.as_ref().map_or(0.0, |h| h.size().x + 8.0);
        let size = Vec2::new((main.size().x + hint_width + 44.0).max(self.min_width), 38.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        if ui.is_rect_visible(rect) {
            let fill = if response.hovered() { ACCENT_HOVER } else { ACCENT };
            ui.painter().rect_filled(rect, 8, fill);
            let content = main.size().x + hint_width;
            let x = rect.center().x - content / 2.0;
            ui.painter().galley(egui::pos2(x, rect.center().y - main.size().y / 2.0), main.clone(), Color32::WHITE);
            if let Some(hint) = hint {
                let at = egui::pos2(x + main.size().x + 8.0, rect.center().y - hint.size().y / 2.0);
                ui.painter().galley(at, hint, white(170));
            }
            if response.has_focus() {
                ui.painter().rect_stroke(rect, 8, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Inside);
            }
        }
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}

/// A link (blue, underlined on hover), for the actions beside a caption or in a sentence.
pub fn link(ui: &mut Ui, text: &str) -> Response {
    ui.link(RichText::new(text).size(12.5))
}

/// A 6 px dot and its text (12, wrapping), for a status. In a right-to-left row (a footer's, beside its button) it
/// takes only its own width, centred on the row.
pub fn status_dot(ui: &mut Ui, color: Color32, text: &str) {
    let dot = |ui: &mut Ui| {
        let line = ui.text_style_height(&egui::TextStyle::Body);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(6.0, line), Sense::hover());
        ui.painter().circle_filled(rect.center(), 3.0, color);
    };
    let text = egui::Label::new(RichText::new(text).size(12.0).color(TEXT_2)).wrap();
    if ui.layout().main_dir() == egui::Direction::RightToLeft {
        ui.add(text);
        ui.add_space(8.0 - ui.spacing().item_spacing.x);
        dot(ui);
        return;
    }
    // Left to right in any other layout, the dot on the first line.
    ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        dot(ui);
        ui.add(text);
    });
}

/// A bar pinned to the bottom of the page area, with a border above: the page's summary and primary action.
/// Add it before the page's body. Typical content: a `ui.horizontal` with the summary in a `ui.vertical` and the
/// action in `ui.with_layout(Layout::right_to_left(Align::Center), ..)`.
pub fn footer<R>(ui: &mut Ui, id: impl egui::AsIdSalt, content: impl FnOnce(&mut Ui) -> R) -> R {
    let margin = Margin { left: PAGE_LEFT, right: PAGE_RIGHT, top: 14, bottom: 14 };
    let shown = egui::Panel::bottom(egui::Id::unique(id))
        .frame(Frame::new().fill(FOOTER).inner_margin(margin))
        .show_separator_line(false)
        .show(ui, content);
    let rect = shown.response.rect;
    ui.painter().hline(rect.x_range(), rect.top(), line(LINE_BUTTON));
    shown.inner
}

/// Whether a modal asks to close (the × button, Esc or a click on the backdrop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalResult {
    Open,
    Close,
}

impl ModalResult {
    pub fn closed(self) -> bool {
        self == ModalResult::Close
    }
}

/// A modal dialog: title (16/600), optional subtitle and ×, the scrolling `body`, and a `footer` bar (buttons,
/// right to left like [`Rows::row`] controls). Call it every frame while it is open and close it on
/// [`ModalResult::Close`].
pub fn modal(
    ctx: &egui::Context,
    id: impl egui::AsIdSalt,
    title: &str,
    subtitle: Option<&str>,
    body: impl FnOnce(&mut Ui),
    footer: impl FnOnce(&mut Ui),
) -> ModalResult {
    let frame = Frame::new().fill(MODAL).stroke(line(LINE_MODAL)).corner_radius(12);
    let max_body = (ctx.content_rect().height() - 190.0).max(120.0);
    let mut close = false;
    let response = egui::Modal::new(egui::Id::unique(id)).backdrop_color(BACKDROP).frame(frame).show(ctx, |ui| {
        let width = (ctx.content_rect().width() - 80.0).clamp(320.0, 720.0);
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = 0.0;
        Frame::new().inner_margin(Margin { left: 20, right: 16, top: 16, bottom: 12 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(RichText::new(title).size(16.0).family(semibold()).color(TEXT));
                    if let Some(subtitle) = subtitle {
                        ui.label(RichText::new(subtitle).color(TEXT_2));
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = close_button(ui));
            });
        });
        let rect = ui.available_rect_before_wrap();
        ui.painter().hline(rect.x_range(), rect.top(), line(LINE));
        Frame::new().inner_margin(Margin::symmetric(20, 14)).show(ui, |ui| {
            ui.set_width(width - 40.0);
            egui::ScrollArea::vertical().max_height(max_body).auto_shrink([false, true]).show(ui, body);
        });
        let bar = Frame::new().fill(FOOTER).corner_radius(CornerRadius { sw: 12, se: 12, nw: 0, ne: 0 });
        let shown = bar.inner_margin(Margin::symmetric(20, 12)).show(ui, |ui| {
            ui.set_width(width - 40.0);
            ui.with_layout(Layout::right_to_left(Align::Center), footer);
        });
        let top = shown.response.rect.top();
        ui.painter().hline(shown.response.rect.x_range(), top, line(LINE));
    });
    if close || response.should_close() {
        ModalResult::Close
    } else {
        ModalResult::Open
    }
}

/// A toast: a small card at the bottom right over the page, with a × and `body` (lines, a link). Call it every
/// frame while it shows; true when its × was clicked.
pub fn toast(ctx: &egui::Context, id: impl egui::AsIdSalt, body: impl FnOnce(&mut Ui)) -> bool {
    let mut close = false;
    egui::Area::new(egui::Id::unique(id))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-16.0, -16.0))
        .show(ctx, |ui| {
            Frame::new()
                .fill(MODAL)
                .stroke(line(LINE_MODAL))
                .corner_radius(10)
                .inner_margin(Margin { left: 14, right: 10, top: 10, bottom: 10 })
                .show(ui, |ui| {
                    ui.set_width(340.0);
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width() - 30.0);
                            ui.spacing_mut().item_spacing.y = 4.0;
                            body(ui);
                        });
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = close_button(ui));
                    });
                });
        });
    close
}

/// The modal's × (painted, since no embedded font has the glyph in this weight); true when clicked.
fn close_button(ui: &mut Ui) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
    let color = if response.hovered() { Color32::WHITE } else { TEXT_2 };
    if response.hovered() {
        ui.painter().rect_filled(rect, 6, white(15));
    }
    let c = Rect::from_center_size(rect.center(), Vec2::splat(9.0));
    let stroke = Stroke::new(1.4, color);
    ui.painter().line_segment([c.left_top(), c.right_bottom()], stroke);
    ui.painter().line_segment([c.right_top(), c.left_bottom()], stroke);
    response.on_hover_text("Close (Esc)").clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every widget draws, a segment click changes the value and a switch toggles.
    #[test]
    fn the_widgets_draw_and_change_their_values() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let (mut choice, mut on) = (1, false);
        for _ in 0..2 {
            ctx.run_pass(egui::RawInput::default(), |ctx| {
                ctx.root_ui(|root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        footer(ui, "footer", |ui| _ = ui.add(primary_button("Start", Some("↵"))));
                        page_header(ui, "Title", Some("subtitle"), |ui| _ = link(ui, "link"));
                        assert!(!group_label(ui, "Group", Some("Link")));
                        panel(ui, |rows| {
                            rows.row("Label", |ui| _ = segmented(ui, "seg", &mut choice, &[(1, "One"), (2, "Two")]));
                            rows.hint("hint");
                            rows.row("Switch", |ui| _ = switch(ui, &mut on));
                        });
                        status_dot(ui, GOOD, "found");
                    })
                });
                assert!(!modal(
                    ctx,
                    "modal",
                    "Title",
                    Some("sub"),
                    |ui| _ = ui.label("body"),
                    |ui| _ = ui.button("Close")
                )
                .closed());
            })
            .textures_delta
            .clear();
        }
        assert_eq!((choice, on), (1, false));
    }
}
