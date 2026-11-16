//! Dropping a co-driver or pace-note download (a .zip, .7z or folder) on the launcher window adds it, as "Add
//! folder…" and "Add archive…" do.

use crate::pages::Page;
use crate::Form;

impl Form {
    /// Takes the first file dropped on the window this frame to the Settings page and adds it; draws the hint
    /// while files hover over the window. Nothing while the viewer runs or an import does.
    pub fn codriver_drop(&mut self, ctx: &egui::Context) {
        let (hovering, dropped) = ctx.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.clone()));
        if self.child.is_some() || self.codriver_job.is_some() {
            return;
        }
        if hovering {
            Self::drop_hint(ctx);
        }
        let Some(path) = dropped.first() else { return };
        let path = path.path().to_path_buf();
        self.page = Page::Settings;
        self.import_codriver_from(path.to_string_lossy().into_owned());
    }

    /// The hint over the window while files hover over it.
    pub fn drop_hint(ctx: &egui::Context) {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop hint")));
        let screen = ctx.content_rect();
        painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(190));
        painter.text(
            screen.center(),
            egui::Align2::CENTER_CENTER,
            "Drop to add a co-driver or pace notes",
            egui::FontId::proportional(24.0),
            egui::Color32::WHITE,
        );
    }
}
