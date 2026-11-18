//! The launcher's look (the redesign's dark theme): colour tokens, the bundled fonts and the egui style built from
//! them. [`apply`] runs once per egui context; the kit (`kit.rs`) and the pages take colours and fonts from here.

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Id, Stroke, TextStyle};

// Surfaces.
pub const PAGE: Color32 = Color32::from_rgb(0x1c, 0x1d, 0x1f);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x17, 0x18, 0x1a);
pub const FOOTER: Color32 = Color32::from_rgb(0x1a, 0x1b, 0x1d);
pub const PANEL: Color32 = Color32::from_rgb(0x23, 0x24, 0x27);
pub const MODAL: Color32 = Color32::from_rgb(0x20, 0x21, 0x24);
pub const INPUT: Color32 = Color32::from_rgb(0x15, 0x16, 0x18);
pub const BUTTON: Color32 = Color32::from_rgb(0x2c, 0x2d, 0x31);
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(0x35, 0x36, 0x3a);
pub const TRACK: Color32 = Color32::from_rgb(0x2a, 0x2b, 0x2e);
pub const SEGMENT: Color32 = Color32::from_rgb(0x45, 0x47, 0x4c);
pub const SWITCH_OFF: Color32 = Color32::from_rgb(0x3a, 0x3b, 0x3f);
pub const CHECK_BORDER: Color32 = Color32::from_rgb(0x55, 0x57, 0x5c);
/// The modal's backdrop, rgba(8, 8, 10, .62).
pub const BACKDROP: Color32 = Color32::from_rgba_premultiplied(5, 5, 6, 158);

/// White at `alpha` (not premultiplied: premultiplied white is the same number in all four).
pub const fn white(alpha: u8) -> Color32 {
    Color32::from_rgba_premultiplied(alpha, alpha, alpha, alpha)
}

// Lines: white at 5, 6, 7, 9 and 10 per cent.
pub const LINE: Color32 = white(13);
pub const LINE_SIDEBAR: Color32 = white(15);
pub const LINE_BUTTON: Color32 = white(18);
pub const LINE_MODAL: Color32 = white(23);
pub const LINE_HOVER: Color32 = white(26);
/// A selected sidebar item's background, white 9%.
pub const SIDEBAR_SELECTED: Color32 = white(23);

// Text.
pub const TEXT: Color32 = Color32::from_rgb(0xec, 0xec, 0xed);
pub const TEXT_LIST: Color32 = Color32::from_rgb(0xe4, 0xe5, 0xe7);
pub const TEXT_LABEL: Color32 = Color32::from_rgb(0xc9, 0xca, 0xcd);
pub const TEXT_SOFT: Color32 = Color32::from_rgb(0xb5, 0xb7, 0xbb);
pub const TEXT_2: Color32 = Color32::from_rgb(0x9a, 0x9c, 0xa1);
pub const TEXT_3: Color32 = Color32::from_rgb(0x6c, 0x6e, 0x73);
pub const TEXT_DISABLED: Color32 = Color32::from_rgb(0x7d, 0x7f, 0x84);

// Colours with a meaning.
pub const ACCENT: Color32 = Color32::from_rgb(0x32, 0x84, 0xd0);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x43, 0x93, 0xe1);
pub const PROGRESS: Color32 = Color32::from_rgb(0x3f, 0x90, 0xdd);
pub const LINK: Color32 = Color32::from_rgb(0x7f, 0xb2, 0xec);
pub const GOOD: Color32 = Color32::from_rgb(0x69, 0xc2, 0x7e);
pub const WARN: Color32 = Color32::from_rgb(0xed, 0xb3, 0x45);
pub const ERROR: Color32 = Color32::from_rgb(0xed, 0x75, 0x6e);
/// A selected list row: the accent at 30%.
pub const ROW_SELECTED: Color32 = Color32::from_rgba_premultiplied(15, 40, 63, 77);
/// A ticked list row: the accent at 14%.
pub const ROW_TICKED: Color32 = Color32::from_rgba_premultiplied(7, 18, 29, 36);

/// Geist Medium, for emphasis in rows and buttons.
pub fn medium() -> FontFamily {
    FontFamily::Name("Geist Medium".into())
}

/// Geist SemiBold: titles.
pub fn semibold() -> FontFamily {
    FontFamily::Name("Geist SemiBold".into())
}

/// Geist Mono Medium.
pub fn mono_medium() -> FontFamily {
    FontFamily::Name("Geist Mono Medium".into())
}

/// A page title's font: 22, semibold.
pub fn title() -> FontId {
    FontId::new(22.0, semibold())
}

/// A number or a path in small mono (counts, versions, folders), `size` 11 to 12.
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Sets the fonts and the style on `ctx`, once (later calls on the same context do nothing).
pub fn apply(ctx: &egui::Context) {
    let done = Id::unique("rerbr theme applied");
    if ctx.data(|d| d.get_temp::<bool>(done)).unwrap_or(false) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(done, true));
    ctx.set_fonts(fonts());
    ctx.set_visuals_of(egui::Theme::Dark, visuals());
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(style);
}

/// Geist first, then egui's fonts (Ubuntu, the monochrome emoji and icon fonts) for the glyphs Geist lacks.
fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let bundled: [(&str, &'static [u8]); 5] = [
        ("Geist", include_bytes!("../assets/fonts/Geist-Regular.ttf")),
        ("Geist-Medium", include_bytes!("../assets/fonts/Geist-Medium.ttf")),
        ("Geist-SemiBold", include_bytes!("../assets/fonts/Geist-SemiBold.ttf")),
        ("Geist-Mono", include_bytes!("../assets/fonts/GeistMono-Regular.ttf")),
        ("Geist-Mono-Medium", include_bytes!("../assets/fonts/GeistMono-Medium.ttf")),
    ];
    for (name, bytes) in bundled {
        fonts.font_data.insert(name.to_owned(), FontData::from_static(bytes).into());
    }
    // Hack has ▼ ▲ ● ↵ ↑ ↓, which no other font has: a last fallback for the proportional families too.
    if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional) {
        list.push("Hack".to_owned());
    }
    let mut family = |family: FontFamily, first: &str, rest: FontFamily| {
        let mut list = vec![first.to_owned()];
        list.extend(fonts.families.get(&rest).cloned().unwrap_or_default());
        fonts.families.insert(family, list);
    };
    // The named families fall back to the plain ones' defaults (read before Proportional and Monospace change).
    family(medium(), "Geist-Medium", FontFamily::Proportional);
    family(semibold(), "Geist-SemiBold", FontFamily::Proportional);
    family(mono_medium(), "Geist-Mono-Medium", FontFamily::Monospace);
    family(FontFamily::Proportional, "Geist", FontFamily::Proportional);
    family(FontFamily::Monospace, "Geist-Mono", FontFamily::Monospace);
    fonts
}

fn stroke(color: Color32) -> Stroke {
    Stroke::new(1.0, color)
}

fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PAGE;
    v.window_fill = MODAL;
    v.window_stroke = stroke(LINE_MODAL);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = egui::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(90) };
    v.extreme_bg_color = INPUT;
    v.faint_bg_color = white(6);
    v.code_bg_color = INPUT;
    v.hyperlink_color = LINK;
    v.warn_fg_color = WARN;
    v.error_fg_color = ERROR;
    v.weak_text_color = Some(TEXT_2);
    v.selection.bg_fill = ROW_SELECTED;
    v.selection.stroke = stroke(Color32::WHITE);
    let round = CornerRadius::same(6);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = stroke(LINE);
    w.noninteractive.fg_stroke = stroke(TEXT);
    w.noninteractive.corner_radius = round;
    w.inactive.bg_fill = BUTTON;
    w.inactive.weak_bg_fill = BUTTON;
    w.inactive.bg_stroke = stroke(LINE_BUTTON);
    w.inactive.fg_stroke = stroke(TEXT);
    w.inactive.corner_radius = round;
    w.hovered.bg_fill = BUTTON_HOVER;
    w.hovered.weak_bg_fill = BUTTON_HOVER;
    w.hovered.bg_stroke = stroke(LINE_HOVER);
    w.hovered.fg_stroke = stroke(Color32::WHITE);
    w.hovered.corner_radius = round;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = SEGMENT;
    w.active.weak_bg_fill = SEGMENT;
    w.active.bg_stroke = stroke(LINE_HOVER);
    w.active.fg_stroke = stroke(Color32::WHITE);
    w.active.corner_radius = round;
    w.active.expansion = 0.0;
    w.open.bg_fill = BUTTON_HOVER;
    w.open.weak_bg_fill = BUTTON_HOVER;
    w.open.bg_stroke = stroke(LINE_HOVER);
    w.open.fg_stroke = stroke(Color32::WHITE);
    w.open.corner_radius = round;
    v
}

fn style(style: &mut egui::Style) {
    for (text, size, family) in [
        (TextStyle::Body, 13.0, FontFamily::Proportional),
        (TextStyle::Button, 12.5, FontFamily::Proportional),
        (TextStyle::Small, 11.5, FontFamily::Proportional),
        (TextStyle::Monospace, 12.0, FontFamily::Monospace),
        (TextStyle::Heading, 22.0, semibold()),
    ] {
        style.text_styles.insert(text, FontId::new(size, family));
    }
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(12.0, 5.0);
    style.spacing.interact_size.y = 26.0;
    style.spacing.combo_width = 0.0;
    style.visuals.indent_has_left_vline = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bundled fonts load, and the symbols the pages draw come from the fallbacks.
    #[test]
    fn fonts_cover_the_pages_glyphs() {
        let ctx = egui::Context::default();
        apply(&ctx);
        // The fonts take effect on the next pass.
        ctx.run_pass(Default::default(), |_| {}).textures_delta.clear();
        ctx.run_pass(Default::default(), |ctx| {
            ctx.fonts_mut(|f| {
                for family in [FontFamily::Proportional, FontFamily::Monospace, medium(), semibold(), mono_medium()] {
                    for c in "Aa9★☆⚠▼↵✔".chars() {
                        assert!(f.has_glyph(&FontId::new(12.0, family.clone()), c), "{family:?} {c}");
                    }
                }
            });
        })
        .textures_delta
        .clear();
    }
}
