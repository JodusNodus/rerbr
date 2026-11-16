//! Dropping files or folders on the launcher window adds them, as the Settings page's Add content does.

use crate::Form;

/// The hint over the window while files hover over it.
const HINT: &str = "Drop to add stages, cars, co-drivers or pace notes";
/// What the hint says while the viewer runs.
const CLOSE_FIRST: &str = "Close the drive first";

impl Form {
    /// Takes the files dropped on the window this frame and adds them; draws the hint while files hover over the
    /// window. A drop while the viewer runs says to close it; one while an add runs is ignored.
    pub fn content_drop(&mut self, ctx: &egui::Context) {
        let (hovering, dropped) = ctx.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.clone()));
        if self.content.busy() {
            return;
        }
        if hovering {
            Self::drop_hint_text(ctx, if self.child.is_some() { CLOSE_FIRST } else { HINT });
        }
        let paths: Vec<_> = dropped.iter().map(|f| f.path().to_path_buf()).collect();
        self.add_content(paths);
    }

    /// The hint over the window while files hover over it.
    #[cfg(test)]
    pub fn drop_hint(ctx: &egui::Context) {
        Self::drop_hint_text(ctx, HINT);
    }

    fn drop_hint_text(ctx: &egui::Context, text: &str) {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop hint")));
        let screen = ctx.content_rect();
        painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(190));
        painter.text(
            screen.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(24.0),
            egui::Color32::WHITE,
        );
    }
}
