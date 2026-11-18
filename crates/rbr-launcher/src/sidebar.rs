//! The window's shell: the 220 px sidebar (brand, the Drive and Library groups, a status area, Import and
//! Settings), the Settings group's header and the shortcuts.

use std::time::Duration;

use egui::{Align, Color32, Frame, Id, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2};

use crate::kit;
use crate::pages::{Page, Section};
use crate::physics::PhysicsProfile;
use crate::theme::*;
use crate::Form;

/// The sidebar's width.
pub const WIDTH: f32 = 220.0;
/// How often the sidebar looks for new downloads (the Import badge), while the Import page is not open.
const SCAN_EVERY: f64 = 2.0;

/// A sidebar item: its label, and what it shows on the right (a count in mono or a badge); the response is the
/// whole row's.
fn item(ui: &mut Ui, label: &str, selected: bool, count: Option<String>) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 6, SIDEBAR_SELECTED);
    } else if response.hovered() {
        painter.rect_filled(rect, 6, white(10));
    }
    let color = if selected { Color32::WHITE } else { TEXT_LABEL };
    let font = egui::FontId::proportional(13.0);
    painter.text(rect.left_center() + Vec2::new(10.0, 0.0), egui::Align2::LEFT_CENTER, label, font, color);
    if let Some(count) = count {
        let at = rect.right_center() - Vec2::new(10.0, 0.0);
        painter.text(at, egui::Align2::RIGHT_CENTER, count, mono(11.5), TEXT_3);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A count badge on the right of an item (Import), accent-coloured.
fn badge(ui: &mut Ui, row: egui::Rect, n: usize) {
    let text = n.to_string();
    let galley = ui.painter().layout_no_wrap(text, egui::FontId::new(11.0, semibold()), Color32::WHITE);
    let width = (galley.size().x + 10.0).max(18.0);
    let rect = egui::Rect::from_center_size(
        egui::pos2(row.right() - 10.0 - width / 2.0, row.center().y),
        Vec2::new(width, 18.0),
    );
    ui.painter().rect_filled(rect, 9, Color32::from_rgb(0x3b, 0x7c, 0xd6));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
}

/// The group's heading (11.5, medium, dim).
fn heading(ui: &mut Ui, text: &str, above: f32) {
    ui.add_space(above);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(RichText::new(text).size(11.5).family(medium()).color(TEXT_3));
    });
    ui.add_space(2.0);
}

impl Form {
    /// What the sidebar says next to a page: a count in the Library.
    fn count(&self, page: Page) -> Option<String> {
        match page {
            Page::Cars => Some(self.catalog.cars.len().to_string()),
            Page::Stages => Some(self.catalog.maps.len().to_string()),
            _ => None,
        }
    }

    /// The modifier's name on the shortcuts' hover texts.
    fn modifier() -> &'static str {
        if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        }
    }

    /// The shell's navigation: the sidebar and the shortcuts. Returns the page a click or a shortcut opens.
    pub(crate) fn nav_ui(&mut self, ctx: &egui::Context) -> Option<Page> {
        if let Some(section) = self.page.section() {
            self.last_pages[section.index()] = self.page;
        }
        let mut open = ctx.input_mut(|i| self.page.shortcut(i, &self.last_pages));
        let frame = Frame::new().fill(SIDEBAR).inner_margin(Margin { left: 12, right: 12, top: 18, bottom: 12 });
        ctx.root_ui(|root| {
            let shown = egui::Panel::left("sidebar")
                .exact_size(WIDTH)
                .resizable(false)
                .frame(frame)
                .show_separator_line(false)
                .show(root, |ui| self.sidebar_ui(ui, &mut open));
            let rect = shown.response.rect;
            root.painter().vline(rect.right() - 0.5, rect.y_range(), Stroke::new(1.0, LINE_SIDEBAR));
        });
        open
    }

    fn sidebar_ui(&mut self, ui: &mut Ui, open: &mut Option<Page>) {
        ui.spacing_mut().item_spacing = Vec2::new(0.0, 1.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new("ReRBR").size(15.0).family(semibold()).color(TEXT));
            ui.label(RichText::new(env!("CARGO_PKG_VERSION")).font(mono(11.0)).color(TEXT_3));
        });
        let modifier = Self::modifier();
        for (n, (section, name)) in Section::ALL.into_iter().take(2).enumerate() {
            heading(ui, name, 20.0);
            for &(page, label) in section.pages() {
                let tip = format!("{modifier}+{}: {name}; {modifier}+Tab: the next page", n + 1);
                if item(ui, label, self.page == page, self.count(page)).on_hover_text(tip).clicked() {
                    *open = Some(page);
                }
            }
        }
        // Pinned to the bottom, added from the bottom up: Settings, Import, a divider, the status area.
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            let modifier = Self::modifier();
            let settings_on = self.page.section() == Some(Section::Settings);
            if item(ui, "Settings", settings_on, None).on_hover_text(format!("{modifier}+3")).clicked() {
                *open = Some(self.last_pages[Section::Settings.index()]);
            }
            self.poll_downloads(ui.ctx());
            let recent = self.content.recent.list.len();
            let import = item(ui, "Import", self.page == Page::Import, None);
            if recent > 0 {
                badge(ui, import.rect, recent);
            }
            let tip = if recent > 0 {
                format!("{recent} recent downloads ReRBR can add")
            } else {
                "Add downloads or files".into()
            };
            if import.on_hover_text(tip).clicked() {
                *open = Some(Page::Import);
            }
            ui.add_space(5.0);
            let (rule, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(rule, 0, LINE_SIDEBAR);
            ui.vertical(|ui| self.status_ui(ui, open));
        });
    }

    /// The status area: the game folder, the physics, what the install lacks (with its fix), the status text and
    /// the update link.
    fn status_ui(&mut self, ui: &mut Ui, open: &mut Option<Page>) {
        let readiness = self.readiness();
        Frame::new().inner_margin(Margin { left: 10, right: 10, top: 10, bottom: 12 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 7.0;
            if self.paths.data.is_dir() {
                kit::status_dot(ui, GOOD, "Game folder found");
            } else {
                kit::status_dot(ui, ERROR, "Game folder not found");
            }
            if self.drive_physics() != PhysicsProfile::Ngp {
                kit::status_dot(ui, WARN, "Experimental physics");
            }
            match &readiness {
                Some(issue) => {
                    kit::status_dot(ui, WARN, &issue.text);
                    // Not on the Settings page, which has the fixes in its rows.
                    if let Some(fix) = issue.fix.filter(|_| self.page != Page::Settings) {
                        if ui.button(fix.label()).clicked() {
                            self.apply(fix);
                            *open = None;
                        }
                    }
                }
                None if !self.status.is_empty() => {
                    ui.add(egui::Label::new(RichText::new(&self.status).size(12.0).color(TEXT_2)).wrap());
                }
                None => {}
            }
            ui.with_layout(Layout::top_down(Align::Min), crate::update::link);
        });
    }

    /// Looks for recent downloads every [`SCAN_EVERY`] seconds (every frame while a scan runs; the Import page does
    /// it itself), for the Import badge.
    fn poll_downloads(&mut self, ctx: &egui::Context) {
        if self.page == Page::Import || self.child.is_some() {
            return;
        }
        let key = Id::unique("sidebar downloads polled");
        let now = ctx.input(|i| i.time);
        let last = ctx.data(|d| d.get_temp::<f64>(key));
        if self.content.recent.scanning() || last.is_none_or(|t| now - t >= SCAN_EVERY) {
            ctx.data_mut(|d| d.insert_temp(key, now));
            let owned = self.owned_content();
            self.content.recent.poll(ctx, owned);
            ctx.request_repaint_after(Duration::from_secs_f64(SCAN_EVERY));
        }
    }

    /// The Settings group's header: the title and the General, Controls and Graphics & sound tabs. Returns the page
    /// a tab opens.
    pub(crate) fn settings_header(&mut self, ui: &mut Ui) -> Option<Page> {
        kit::page_header(ui, "Settings", None, |_| {});
        let mut page = self.page;
        ui.scope(|ui| {
            // Under the title, closer than the header's own bottom padding.
            ui.add_space(-8.0);
            Frame::new().inner_margin(Margin { left: kit::PAGE_LEFT, right: 0, top: 0, bottom: 10 }).show(ui, |ui| {
                kit::segmented(ui, "settings tabs", &mut page, Section::Settings.pages());
            });
        });
        (page != self.page).then_some(page)
    }
}
