//! Dropping files or folders on the launcher window adds them, as the Import page does.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::Form;

/// Whether the window takes dropped files: winit 0.30 has no drag and drop on native Wayland, so there Import
/// does not offer it (the Import page says "Add files or folders").
static DROP_WORKS: AtomicBool = AtomicBool::new(true);

/// Marks the window as one that takes no dropped files (native Wayland).
pub fn no_drop() {
    DROP_WORKS.store(false, Ordering::Relaxed);
}

/// Whether files dropped on the window are added.
pub fn drop_works() -> bool {
    DROP_WORKS.load(Ordering::Relaxed)
}

/// The hint over the window while files hover over it.
const HINT: &str = "Drop to add\nStages, cars, co-drivers, pace notes, or the game disc image (.iso)";
/// What the hint says while the viewer runs.
const CLOSE_FIRST: &str = "Close the drive first\nThen drop again";

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

    /// Dims the window, with a rounded highlighted border inset from its edges, the title and what is accepted.
    fn drop_hint_text(ctx: &egui::Context, text: &str) {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop hint")));
        let screen = ctx.content_rect();
        painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(200));
        let accent = crate::pages::GOOD;
        painter.rect(
            screen.shrink(24.0),
            24.0,
            accent.gamma_multiply(0.12),
            egui::Stroke::new(3.0, accent),
            egui::StrokeKind::Inside,
        );
        let (title, line) = text.split_once('\n').unwrap_or((text, ""));
        let middle = screen.center();
        painter.text(
            middle - egui::vec2(0.0, 16.0),
            egui::Align2::CENTER_CENTER,
            title,
            egui::FontId::proportional(44.0),
            egui::Color32::WHITE,
        );
        painter.text(
            middle + egui::vec2(0.0, 28.0),
            egui::Align2::CENTER_CENTER,
            line,
            egui::FontId::proportional(18.0),
            egui::Color32::GRAY,
        );
    }
}
