//! What the stage and the car browsers share: the lines of a list, the page frame (header, filter bar, list with its
//! hint bar, inspector), the keys, the highlighted row with its scrolling, and the recent picks. The browsers differ in
//! their scopes, filters, columns and inspector.

use egui::{Align2, Color32, FontId, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke};

use crate::kit;
use crate::theme::{self, TEXT, TEXT_2, TEXT_3};

pub const GREY: Color32 = theme::TEXT_2;
pub const DIM: Color32 = theme::TEXT_3;
pub const STAR: Color32 = theme::WARN;
/// The list's row height, points.
pub const ROW: f32 = 33.0;
/// The list's visible height before the first draw has measured it.
const LIST_HEIGHT: f32 = 420.0;
/// Rows a Page Up or Page Down moves.
const PAGE: usize = 10;
/// The recent picks kept.
pub const RECENT: usize = 6;

/// A line of a list: a header, or an item (its position in the browser's rows).
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Header(String),
    Row(usize),
}

/// `members` under `header`, nothing when there are none.
pub fn block(header: Option<&str>, members: Vec<usize>) -> impl Iterator<Item = Line> {
    let header = if members.is_empty() { None } else { header.map(|h| Line::Header(h.to_owned())) };
    header.into_iter().chain(members.into_iter().map(Line::Row))
}

/// The items of `lines`, in order.
pub fn items(lines: &[Line]) -> impl Iterator<Item = usize> + '_ {
    lines.iter().filter_map(|l| if let Line::Row(k) = l { Some(*k) } else { None })
}

/// Puts `item` first in `recent`, keeping [`RECENT`].
pub fn remember<T: PartialEq>(recent: &mut Vec<T>, item: T) {
    recent.retain(|r| *r != item);
    recent.insert(0, item);
    recent.truncate(RECENT);
}

/// The keys a browser takes before the search box or the page does.
#[derive(Debug, Clone, Copy, Default)]
pub struct Keys {
    pub down: bool,
    pub up: bool,
    pub page_down: bool,
    pub page_up: bool,
    pub enter: bool,
    pub escape: bool,
    /// Ctrl/Cmd+D.
    pub favourite: bool,
}

impl Keys {
    /// The keys, when the list has them: nothing else focused but the search box, and no dropdown open. A focused
    /// button or filter keeps Enter and the arrows (which move the focus); Esc leaves it first.
    pub fn take(ui: &egui::Ui) -> Keys {
        // Esc on another control only leaves it (egui drops the focus before this runs): remembered a pass.
        let elsewhere = egui::Id::unique("browser focus elsewhere");
        let focused_elsewhere = ui.memory(|m| m.focused()).is_some_and(|id| id != search_id());
        let left = ui.data_mut(|d| std::mem::replace(d.get_temp_mut_or(elsewhere, false), focused_elsewhere));
        if !list_has_keys(ui) {
            return Keys::default();
        }
        ui.input_mut(|i| Keys {
            down: i.consume_key(Modifiers::NONE, Key::ArrowDown),
            up: i.consume_key(Modifiers::NONE, Key::ArrowUp),
            page_down: i.consume_key(Modifiers::NONE, Key::PageDown),
            page_up: i.consume_key(Modifiers::NONE, Key::PageUp),
            enter: i.consume_key(Modifiers::NONE, Key::Enter),
            escape: !left && i.consume_key(Modifiers::NONE, Key::Escape),
            favourite: i.consume_key(Modifiers::COMMAND, Key::D),
        })
    }

    /// How far the arrow and page keys move the highlight, rows.
    pub fn movement(&self) -> isize {
        let page = PAGE as isize;
        [(self.down, 1), (self.up, -1), (self.page_down, page), (self.page_up, -page)]
            .into_iter()
            .map(|(pressed, by)| if pressed { by } else { 0 })
            .sum()
    }
}

/// The browsers' search box: the list's keys pass through it.
pub fn search_id() -> egui::Id {
    egui::Id::unique("browser search")
}

/// Whether a list's keys are free: nothing focused but the search box, and no dropdown open.
pub fn list_has_keys(ui: &egui::Ui) -> bool {
    ui.memory(|m| m.focused()).is_none_or(|id| id == search_id()) && !egui::Popup::is_any_open(ui.ctx())
}

/// The highlighted row (its position in the browser's rows) and the list's scrolling.
#[derive(Debug)]
pub struct Nav {
    pub cursor: usize,
    /// The scroll offset and visible height as last drawn.
    scroll: (f32, f32),
    /// Bring the cursor into view at the next draw: `Some(true)` centred.
    reveal: Option<bool>,
    to_top: bool,
    restore_scroll: Option<f32>,
}

impl Default for Nav {
    fn default() -> Self {
        Nav { cursor: 0, scroll: (0.0, LIST_HEIGHT), reveal: None, to_top: false, restore_scroll: None }
    }
}

impl Nav {
    pub fn offset(&self) -> f32 {
        self.scroll.0
    }

    /// Restores a stable item and the last scroll position, including after restarting the launcher.
    pub fn restore(&mut self, cursor: usize, offset: f32) {
        self.cursor = cursor;
        self.scroll.0 = offset;
        self.restore_scroll = Some(offset);
        self.reveal = None;
        self.to_top = false;
    }

    pub fn resume(&mut self) {
        self.restore(self.cursor, self.scroll.0);
    }

    /// Highlights `cursor` and brings it to the middle of the list.
    pub fn show(&mut self, cursor: usize) {
        self.cursor = cursor;
        self.restore_scroll = None;
        self.reveal = Some(true);
    }

    /// Highlights `cursor` where it is.
    pub fn point(&mut self, cursor: usize) {
        self.cursor = cursor;
    }

    /// The cursor's line of `lines`.
    pub fn position(&self, lines: &[Line]) -> Option<usize> {
        lines.iter().position(|l| *l == Line::Row(self.cursor))
    }

    /// Moves the cursor `by` rows down (negative: up) the list, headers skipped.
    pub fn step(&mut self, lines: &[Line], by: isize) {
        let rows: Vec<usize> = items(lines).collect();
        if rows.is_empty() {
            return;
        }
        let next = match rows.iter().position(|&k| k == self.cursor) {
            Some(at) => (at as isize + by).clamp(0, rows.len() as isize - 1) as usize,
            None => 0,
        };
        self.cursor = rows[next];
        self.restore_scroll = None;
        self.reveal = Some(false);
    }

    /// After the list changed: the cursor stays when its row is still there, else it moves to the first row and the
    /// list to its top.
    pub fn settle(&mut self, lines: &[Line]) {
        if self.position(lines).is_some() {
            if self.restore_scroll.is_none() {
                self.reveal.get_or_insert(true);
            }
        } else if let Some(first) = items(lines).next() {
            self.cursor = first;
            self.restore_scroll = None;
            self.to_top = true;
        }
    }

    /// The list's scroll area, set to bring the cursor (on line `position`) into view when asked to.
    pub fn area(&mut self, id: &'static str, height: f32, position: Option<usize>) -> egui::ScrollArea {
        let mut area = egui::ScrollArea::vertical().id_salt(id).max_height(height).auto_shrink([false, true]);
        if let Some(offset) = self.restore_scroll.take() {
            area = area.vertical_scroll_offset(offset);
        } else if std::mem::take(&mut self.to_top) {
            area = area.vertical_scroll_offset(0.0);
        } else if let (Some(centre), Some(at)) = (self.reveal.take(), position) {
            let (offset, visible) = self.scroll;
            let top = at as f32 * ROW;
            let want = if centre {
                (top - visible * 0.4).max(0.0)
            } else if top < offset {
                top
            } else if top + ROW > offset + visible {
                top + ROW - visible
            } else {
                offset
            };
            area = area.vertical_scroll_offset(want);
        }
        area
    }

    /// Keeps where the list is scrolled and how much of it shows.
    pub fn scrolled<R>(&mut self, output: &egui::scroll_area::ScrollAreaOutput<R>) {
        self.scroll = (output.state.clamped_offset().y, output.inner_rect.height());
    }
}

/// What a click in a browser's header asked for.
#[derive(Debug, Default, Clone, Copy)]
pub struct HeaderClicks {
    pub back: bool,
    pub random: bool,
    pub get_more: bool,
}

/// A browser's page header: the title with its subtitle, and on the right Random, "Get more" (a page's: its label and
/// tooltip) or Back (a picker's).
pub fn header(ui: &mut egui::Ui, title: &str, subtitle: Option<&str>, get_more: Option<(&str, &str)>) -> HeaderClicks {
    let mut clicks = HeaderClicks::default();
    kit::page_header(ui, title, subtitle, |ui| {
        match get_more {
            Some((label, tip)) => clicks.get_more = ui.button(label).on_hover_text(tip).clicked(),
            None => clicks.back = ui.button("Back").on_hover_text("Back without changing the choice (Esc)").clicked(),
        }
        clicks.random = ui.button("Random").on_hover_text("A random pick of the list as filtered").clicked();
    });
    clicks
}

/// The filter bar's frame: the search, scopes and dropdowns on the page's padding, with room below.
pub fn bar<R>(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let margin = egui::Margin { left: kit::PAGE_LEFT, right: kit::PAGE_RIGHT, top: 0, bottom: 14 };
    egui::Frame::new().inner_margin(margin).show(ui, content).inner
}

/// The width a segmented control of `labels` takes.
pub fn segmented_width(ui: &egui::Ui, labels: &[&str]) -> f32 {
    let font = egui::FontId::new(12.5, theme::medium());
    let text: f32 =
        labels.iter().map(|l| ui.painter().layout_no_wrap((*l).to_owned(), font.clone(), TEXT).size().x + 24.0).sum();
    text + 4.0
}

/// A dropdown in the filter bar: `shown` is its closed text, the options carry their own labels.
pub fn dropdown<T: Clone + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    width: f32,
    shown: &str,
    value: &mut T,
    options: &[(T, String)],
) {
    egui::ComboBox::from_id_salt(id).width(width).selected_text(shown).show_ui(ui, |ui| {
        for (option, label) in options {
            ui.selectable_value(value, option.clone(), label);
        }
    });
}

/// The search box: it takes focus when `focus` is set, and when the player types with nothing else focused on it.
/// Typing anywhere searches.
pub fn search_box(ui: &mut egui::Ui, hint: &str, query: &mut String, focus: &mut bool, width: f32) {
    let id = search_id();
    let typing = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(_))));
    if (std::mem::take(focus) || typing) && ui.memory(|m| m.focused()) != Some(id) {
        ui.memory_mut(|m| m.request_focus(id));
    }
    ui.add(
        egui::TextEdit::singleline(query)
            .id(id)
            .hint_text(hint)
            .desired_width(width)
            .margin(egui::Margin::symmetric(11, 7))
            .align(egui::Align2::LEFT_CENTER),
    );
}

/// The inspector's width, points.
pub const INSPECTOR: f32 = 380.0;

/// The inspector on the right of the page's body below its filter bar, with a border; call it before drawing the list,
/// which takes what is left. Its `pinned` part (the buttons) keeps its room at the bottom; `scrolling` scrolls above it.
pub fn inspector(ui: &mut egui::Ui, scrolling: impl FnOnce(&mut egui::Ui), pinned: impl FnOnce(&mut egui::Ui)) {
    let rect = ui.available_rect_before_wrap();
    ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, theme::LINE));
    let margin = egui::Margin { left: 20, right: 20, top: 20, bottom: 18 };
    let frame = egui::Frame::new().fill(theme::PAGE).inner_margin(margin);
    let width = INSPECTOR.min(rect.width() * 0.5);
    egui::Panel::right(egui::Id::unique("browser inspector")).exact_size(width).frame(frame).show(ui, |ui| {
        let actions = egui::Frame::new().inner_margin(egui::Margin { top: 12, ..Default::default() });
        egui::Panel::bottom(egui::Id::unique("browser inspector actions"))
            .frame(actions)
            .show_separator_line(false)
            .show(ui, pinned);
        egui::ScrollArea::vertical().id_salt("inspector").auto_shrink([false, false]).show(ui, scrolling);
    });
}

/// The key hints under the list, with `shown` ("31 shown") on the left; `keys` are (keys, action) in reading order.
pub fn hint_bar(ui: &mut egui::Ui, shown: &str, keys: &[(&str, &str)], list: impl FnOnce(&mut egui::Ui)) {
    let frame = egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 20, top: 9, bottom: 9 });
    let bar =
        egui::Panel::bottom(egui::Id::unique("browser hints")).frame(frame).show_separator_line(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(shown).size(11.5).color(TEXT_3));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    for (k, action) in keys.iter().rev() {
                        ui.label(RichText::new(format!("{k} {action}")).size(11.5).color(TEXT_3));
                    }
                });
            });
        });
    let rect = bar.response.rect;
    ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, theme::LINE));
    list(ui);
}

/// Takes the browser's keys and moves the highlight with the arrow and page keys; the keys.
///
/// The keys belong to the list, not to the search box or the Start button.
pub fn step_nav(ui: &egui::Ui, nav: &mut Nav, lines: &[Line]) -> Keys {
    let keys = Keys::take(ui);
    let by = keys.movement();
    if by != 0 {
        nav.step(lines, by);
    }
    keys
}

/// What the keys chose: Enter chooses (when `on_enter` takes the highlighted row), Ctrl/Cmd+D favourites it,
/// Escape goes back.
pub fn key_action<R, A>(
    nav: &Nav,
    rows: &[R],
    keys: &Keys,
    mut on_enter: impl FnMut(&R) -> Option<A>,
    mut on_favourite: impl FnMut(&R) -> A,
    back: A,
) -> Option<A> {
    let mut action = None;
    if let Some(row) = rows.get(nav.cursor) {
        if keys.enter {
            action = on_enter(row);
        }
        if keys.favourite {
            action = Some(on_favourite(row));
        }
    }
    if keys.escape {
        action = Some(back);
    }
    action
}

/// A random startable row of `lines`, picked by the clock.
pub fn random_row(lines: &[Line], startable: impl Fn(usize) -> bool) -> Option<usize> {
    let rows: Vec<usize> = items(lines).filter(|&k| startable(k)).collect();
    if rows.is_empty() {
        return None;
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    Some(rows[nanos as usize % rows.len()])
}

/// The list's scrolled rows: draws `add_contents` for each visible range (which may move the highlight through `nav`),
/// then keeps the scrolling.
pub fn list_rows(
    ui: &mut egui::Ui,
    nav: &mut Nav,
    id: &'static str,
    lines: &[Line],
    mut add_contents: impl FnMut(&mut egui::Ui, &mut Nav, std::ops::Range<usize>),
) {
    let height = ui.available_height().max(ROW * 3.0);
    let area = nav.area(id, height, nav.position(lines));
    let output = ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        area.show_rows(ui, ROW, lines.len(), |ui, range| add_contents(ui, nav, range))
    });
    nav.scrolled(&output.inner);
}

/// The side room of a list row inside its line, points.
const INSET: f32 = 12.0;

/// A list row's slot: its (inset) rect with its click, and the star's click. Not Tab stops: the arrows move through
/// the list.
pub fn row_slot(ui: &mut egui::Ui, k: usize) -> (Rect, egui::Response, egui::Response) {
    let (line, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), Sense::CLICK);
    let rect = line.shrink2(egui::vec2(INSET, 1.0));
    let star_rect = Rect::from_center_size(Pos2::new(rect.left() + 14.0, rect.center().y), egui::vec2(28.0, ROW));
    let star = ui.interact(star_rect, ui.scope_id().with(("star", k)), Sense::CLICK);
    (rect, response, star)
}

/// The left x of each of the right-aligned columns of `widths` (left to right) in a row `rect`, and the x where the
/// name must end.
pub fn columns(rect: Rect, widths: &[f32]) -> (Vec<f32>, f32) {
    let mut x = rect.right() - 12.0;
    let mut lefts: Vec<f32> = widths
        .iter()
        .rev()
        .map(|w| {
            x -= w;
            let left = x;
            x -= 12.0;
            left
        })
        .collect();
    lefts.reverse();
    (lefts, x)
}

/// Where a row's name starts, from the row's left.
pub fn name_x(rect: Rect) -> f32 {
    rect.left() + 34.0
}

/// A row's text at `x`, centred on the row and clipped before `right`.
pub fn paint_cell(painter: &egui::Painter, rect: Rect, x: f32, right: f32, text: &str, font: FontId, color: Color32) {
    let galley = painter.layout_no_wrap(text.to_owned(), font, color);
    let at = Pos2::new(x, rect.center().y - galley.size().y / 2.0);
    let room = Rect::from_min_max(Pos2::new(x, rect.top()), Pos2::new(right.max(x), rect.bottom()));
    painter.with_clip_rect(room).galley(at, galley, color);
}

/// A small label such as "BTB" painted at `x` on the row; its right edge.
pub fn paint_tag(painter: &egui::Painter, rect: Rect, x: f32, text: &str) -> f32 {
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::new(10.5, theme::medium()), TEXT_2);
    let tag = Rect::from_min_size(Pos2::new(x, rect.center().y - 8.0), egui::vec2(galley.size().x + 10.0, 16.0));
    painter.rect_filled(tag, 4.0, theme::white(20));
    painter.galley(tag.center() - galley.size() / 2.0, galley, TEXT_2);
    tag.right()
}

/// A list row's highlight: filled when it holds the cursor, lighter when hovered.
pub fn paint_selection(ui: &egui::Ui, rect: Rect, cursor: bool, hovered: bool) {
    if cursor {
        ui.painter().rect_filled(rect, 5.0, theme::ROW_SELECTED);
    } else if hovered {
        ui.painter().rect_filled(rect, 5.0, theme::white(8));
    }
}

/// A list row's star: filled when a favourite, outlined on the cursor's row or under the mouse (`active`).
pub fn paint_star(painter: &egui::Painter, pos: Pos2, favourite: bool, active: bool) {
    let (star, colour) = match (favourite, active) {
        (true, _) => ("★", STAR),
        (false, true) => ("☆", TEXT_2),
        (false, false) => ("☆", Color32::from_gray(70)),
    };
    painter.text(pos, Align2::CENTER_CENTER, star, FontId::proportional(14.0), colour);
}

/// The list's header row: the column titles over the columns the rows use (`titles` and the `lefts` of
/// [`columns`] for the right-aligned ones; a title marked right is aligned to its column's right edge).
pub fn list_header(ui: &mut egui::Ui, name: &str, widths: &[f32], titles: &[(&str, bool)]) {
    let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), Sense::hover());
    let rect = line.shrink2(egui::vec2(INSET, 0.0));
    let (lefts, name_right) = columns(rect, widths);
    let painter = ui.painter();
    let font = FontId::new(11.5, theme::medium());
    paint_cell(painter, rect, name_x(rect), name_right, name, font.clone(), TEXT_3);
    for ((left, width), (title, right_aligned)) in lefts.iter().zip(widths).zip(titles) {
        let galley = painter.layout_no_wrap((*title).to_owned(), font.clone(), TEXT_3);
        let x = if *right_aligned { left + width - galley.size().x } else { *left };
        painter.galley(Pos2::new(x, rect.center().y - galley.size().y / 2.0), galley, TEXT_3);
    }
}

/// A list header's line (a group of the list: a country, a class, "Cannot start").
pub fn header_line(ui: &mut egui::Ui, name: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), Sense::hover());
    let at = rect.left_bottom() + egui::vec2(INSET + 22.0, -6.0);
    ui.painter().text(at, Align2::LEFT_BOTTOM, name, FontId::new(11.5, theme::medium()), TEXT_3);
}

/// The inspector's Favourite button: a star and the word, the star lit when a favourite.
pub fn favourite_button(ui: &mut egui::Ui, favourite: bool) -> egui::Response {
    let (star, colour) = if favourite { ("★", STAR) } else { ("☆", TEXT_2) };
    let mut job = egui::text::LayoutJob::default();
    RichText::new(format!("{star} ")).size(13.0).color(colour).append_to(
        &mut job,
        ui.style(),
        egui::FontSelection::Default,
        egui::Align::Center,
    );
    RichText::new("Favourite").size(13.0).append_to(
        &mut job,
        ui.style(),
        egui::FontSelection::Default,
        egui::Align::Center,
    );
    let button = egui::Button::new(job).min_size(egui::vec2(0.0, 38.0));
    ui.add(button).on_hover_text("Ctrl/Cmd+D")
}

/// The inspector's primary button over the rest of the row; `enabled` when the row can be chosen.
pub fn drive_button(ui: &mut egui::Ui, label: &str, enabled: bool, tip: &str) -> egui::Response {
    let width = ui.available_width();
    ui.add_enabled(enabled, kit::primary_button(label, None).min_width(width)).on_hover_text(tip)
}

/// The inspector's title (17, semibold) and its dim line under it.
pub fn title(ui: &mut egui::Ui, name: &str, line: &str) {
    ui.add_space(14.0);
    ui.add(egui::Label::new(RichText::new(name).size(17.0).family(theme::semibold()).color(TEXT)).wrap());
    ui.add_space(2.0);
    ui.add(egui::Label::new(RichText::new(line).color(TEXT_2)).wrap());
    ui.add_space(14.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pass with `key` pressed and `focus` asked for (applied the next pass); the list's keys and the button's id.
    fn pass(ctx: &egui::Context, key: Option<Key>, focus: Option<egui::Id>) -> (Keys, egui::Id) {
        let events = key.map(|key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        let input = egui::RawInput { events: events.into_iter().collect(), ..Default::default() };
        let (mut keys, mut button) = (Keys::default(), egui::Id::NULL);
        ctx.run_pass(input, |ctx| {
            ctx.root_ui(|root| {
                egui::CentralPanel::default().show(root, |ui| {
                    keys = Keys::take(ui);
                    ui.add(egui::TextEdit::singleline(&mut String::new()).id(search_id()));
                    button = ui.button("4WD").id;
                    if let Some(id) = focus {
                        ui.memory_mut(|m| m.request_focus(id));
                    }
                })
            });
        })
        .textures_delta
        .clear();
        (keys, button)
    }

    #[test]
    fn the_list_has_the_keys_unless_another_control_is_focused() {
        let ctx = egui::Context::default();
        let (keys, button) = pass(&ctx, Some(Key::ArrowDown), None);
        assert!(keys.down, "nothing focused");
        pass(&ctx, None, Some(search_id()));
        assert!(pass(&ctx, Some(Key::Enter), None).0.enter, "the search box");
        pass(&ctx, None, Some(button));
        let (keys, _) = pass(&ctx, Some(Key::Enter), None);
        assert!(!keys.enter && !keys.down, "a focused filter keeps Enter");
        // Esc first leaves the filter, then goes back.
        assert!(!pass(&ctx, Some(Key::Escape), None).0.escape);
        assert!(pass(&ctx, Some(Key::Escape), None).0.escape);
    }
}
