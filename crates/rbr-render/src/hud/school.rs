//! The rally-school lesson's screens over the HUD, from the live school record (`rbr_recording::school`, SCH1): the
//! brief's panel while the countdown waits for the confirm, a short toast for a too-slow or too-fast incident code,
//! and the verdict panel at the end, in place of the stage result page (which does not show for a school drive).
//!
//! The texts are 1.02's English string table's (`bstrings.eng`): the lesson's number, title, instruction and cone
//! guide; the panels' look is ours, as the rest of the HUD's. The English table is not read at runtime (the HUD has
//! no game data there), so the keys' English text is baked in, as the pause menu's and the retire's are
//! (`super::pause::text`, `super::retire::failed_text`).

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Vec2};
use rbr_recording::school::{self, School as Record};

use super::model::HudView;
use super::time_label;

/// How long an incident code's toast shows, s. Ours.
pub const TOAST_SECONDS: f32 = 2.0;

/// Where the lesson stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    /// The brief: the countdown waits for the confirm.
    #[default]
    Brief,
    /// The lesson is driven.
    Driving,
    /// The drive ended.
    Ended,
}

impl Phase {
    /// The SCH1 state word's phase; anything else is driving.
    pub fn of(state: u32) -> Phase {
        match state {
            school::STATE_BRIEF => Phase::Brief,
            school::STATE_ENDED => Phase::Ended,
            _ => Phase::Driving,
        }
    }
}

/// An incident code's toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toast {
    /// Codes 2 and 3: too slow.
    TooSlow,
    /// Codes 4 and 5: too fast.
    TooFast,
}

impl Toast {
    /// The toast an incident code shows, if any (code 1, a cone or wall, is silent in the original).
    pub fn of(incident: u32) -> Option<Toast> {
        match incident {
            2 | 3 => Some(Toast::TooSlow),
            4 | 5 => Some(Toast::TooFast),
            _ => None,
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Toast::TooSlow => "Too slow",
            Toast::TooFast => "Too fast",
        }
    }
}

/// What the school drive shows: the lesson, where it stands and its verdict, with the toast showing now and
/// the failed end's reasons (the English texts of the record's reason keys, in order).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub lesson: u32,
    pub phase: Phase,
    /// None before the end.
    pub verdict: Option<u32>,
    /// The end's race time, s.
    pub time: f32,
    /// The lesson's times, s (both 0 untimed).
    pub perfect: f32,
    pub pass: f32,
    pub toast: Option<Toast>,
    /// The failed end's reasons, resolved (`resolve`).
    pub reasons: Vec<String>,
}

impl State {
    /// The record's state; the toast is set where the record is taken ([`super::model::HudModel::set_school`]),
    /// the reasons once the English table is known there.
    pub fn of(record: &Record) -> State {
        State {
            lesson: record.lesson,
            phase: Phase::of(record.state),
            verdict: (record.verdict != school::NO_VERDICT).then_some(record.verdict),
            time: record.time,
            perfect: record.perfect,
            pass: record.pass,
            toast: None,
            reasons: Vec::new(),
        }
    }

    /// Whether the lesson is timed.
    pub fn timed(&self) -> bool {
        self.perfect != 0.0 || self.pass != 0.0
    }
}

/// The record's reason keys' English texts, in order; unknown keys are skipped, as the original's null
/// `Resolve` skips them (`CFailedSchoolExplanationUI::Vf0C` 0x542330). `None` (no table yet) resolves nothing.
pub fn resolve(reasons: &[school::Reason], strings: Option<&rbr_formats::strings::Strings>) -> Vec<String> {
    let Some(strings) = strings else { return Vec::new() };
    reasons.iter().filter_map(|reason| strings.get(&reason.key)).map(|text| text.trim_end().to_owned()).collect()
}

/// The lesson's brief: its title, instruction and cone guide. `None` past the last lesson.
pub fn brief(lesson: u32) -> Option<Brief> {
    BRIEFS.get(lesson as usize).copied()
}

/// The lesson's number label, as the game's (`RSM_LESSON_NRB1`..`NRB6`, `RSM_LESSON_NRA1`..`NRA11`): group-relative,
/// like the launcher's.
pub fn number(lesson: u32) -> u32 {
    if lesson < rbr_school::Lesson::FIRST_ADVANCED {
        lesson + 1
    } else {
        lesson - rbr_school::Lesson::FIRST_ADVANCED + 1
    }
}

/// A verdict's line: 4 perfect, 1..3 passed (the grade shows as pips), else failed.
pub fn verdict_text(verdict: u32) -> &'static str {
    match verdict {
        4 => "Perfect",
        1..=3 => "Passed",
        _ => "Failed",
    }
}

/// A lesson's title, instruction and cone guide (`Lesson::title_key`, `brief_keys`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Brief {
    pub title: &'static str,
    pub instruction: &'static str,
    /// `\n`-separated lines, or `"None"`.
    pub cones: &'static str,
}

/// The lessons' briefs in lesson order (see [`brief`]).
const BRIEFS: [Brief; 17] = [
    Brief {
        title: "Start Up And Basic Controls",
        instruction: "Learn the basics of rally driving.",
        cones: "Yellow rectangle - End lesson",
    },
    Brief {
        title: "Basic Driving and Changing Gear",
        instruction: "Practice basic car handling at lower speeds.",
        cones: "None",
    },
    Brief {
        title: "Brake Test: Gravel",
        instruction: "Practice controlled braking on gravel.",
        cones: "Green cones - Brake",
    },
    Brief {
        title: "Road Positioning and Gears",
        instruction: "Practice basic driving and sound race line positioning.",
        cones: "Red cones - Brake\nGreen cones - Throttle\nBlue cones - Guidance",
    },
    Brief {
        title: "Qualification Trial",
        instruction: "Take the basic rally school graduation test.",
        cones: "Red cones - Brake\nGreen cones - Throttle\nBlue cones - Guidance",
    },
    Brief {
        title: "High Speed Passenger Ride",
        instruction: "Richard Burns shows you how it's really done!",
        cones: "None",
    },
    Brief {
        title: "Handbrake Turn: Gravel",
        instruction: "Learn the handbrake turn technique on gravel.",
        cones: "Green cones - Handbrake corner",
    },
    Brief {
        title: "Oversteer: Left Foot Braking",
        instruction: "Learn to create oversteer using left-foot braking.",
        cones: "Red brake cones.\nGreen throttle cones.",
    },
    Brief {
        title: "Oversteer: Throttle Control",
        instruction: "Learn to create oversteer by lifting off the throttle.",
        cones: "Red cones - Lift off throttle\nGreen cones - Apply throttle",
    },
    Brief {
        title: "Powerslide",
        instruction: "Learn to use the powerslide technique.",
        cones: "Blue cones - Initiate the powerslide",
    },
    Brief {
        title: "Basic Handling: Tarmac",
        instruction: "Learn to handle a rally car on tarmac surfaces.",
        cones: "Yellow rectangle - End lesson",
    },
    Brief {
        title: "Brake Test: Tarmac",
        instruction: "Learn to brake efficiently and safely on tarmac.",
        cones: "Green cones - Brake",
    },
    Brief {
        title: "Handbrake Turn: Tarmac",
        instruction: "Learn to apply the handbrake turn technique on tarmac.",
        cones: "Green cones - Handbrake corners",
    },
    Brief { title: "Soft Suspension", instruction: "Try driving with soft suspension.", cones: "None" },
    Brief { title: "Hard Suspension", instruction: "Try driving with hard suspension.", cones: "None" },
    Brief {
        title: "Scandinavian Flick",
        instruction: "Learn to use the Scandinavian Flick technique.",
        cones: "Blue cones - Initiate flick\nGreen cones - Accelerate",
    },
    Brief {
        title: "Special Stage Trial",
        instruction: "Put all you have learned to the final test!",
        cones: "Regular Pacenotes",
    },
];

const PANEL: Color32 = Color32::from_rgba_premultiplied(10, 10, 12, 235);
const TEXT: Color32 = Color32::from_gray(235);
const DIM: Color32 = Color32::from_gray(160);
const RULE: Color32 = Color32::from_gray(60);
const GOLD: Color32 = Color32::from_rgb(255, 196, 0);
const FAILED: Color32 = Color32::from_rgb(220, 70, 60);
const WIDTH: f32 = 560.0;
/// The panel's top, a share of the screen's height down: the results menu's buttons fit under a verdict.
const TOP: f32 = 0.22;

/// A lesson time in whole seconds, as the game's school texts write it (`RSM_TIMELIMITA6` is "Pass Time: 10s").
fn seconds(time: f32) -> String {
    format!("{} s", time as u32)
}

/// Draws the lesson's screens over the HUD: the brief's panel, the incident toast, or the verdict panel.
pub fn paint(ctx: &egui::Context, state: &HudView) {
    let Some(school) = &state.school else { return };
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("school")));
    let screen = ctx.content_rect();
    match (school.phase, school.toast, school.verdict) {
        (Phase::Brief, ..) => brief_panel(&painter, screen, school),
        (Phase::Driving, Some(toast), _) => toast_panel(&painter, screen, toast),
        (Phase::Ended, _, Some(verdict)) => verdict_panel(&painter, screen, school, verdict),
        _ => {}
    }
}

/// The lesson's label: its group-relative number and group, as the school's menu lists them.
fn label(lesson: u32) -> String {
    let group = if lesson < rbr_school::Lesson::FIRST_ADVANCED { "BASIC" } else { "ADVANCED" };
    format!("LESSON {} · {group}", number(lesson))
}

/// A column of centred rows painted top down, `y` the next row's top.
struct Column<'a> {
    painter: &'a Painter,
    x: f32,
    y: f32,
}

impl Column<'_> {
    fn text(&mut self, text: &str, size: f32, colour: Color32) {
        self.painter.text(Pos2::new(self.x, self.y), Align2::CENTER_TOP, text, FontId::proportional(size), colour);
        self.y += size * 1.3;
    }

    fn gap(&mut self, height: f32) {
        self.y += height;
    }

    fn rule(&mut self) {
        self.gap(10.0);
        let half = WIDTH / 2.0 - 40.0;
        self.painter.hline((self.x - half)..=(self.x + half), self.y, (1.0, RULE));
        self.gap(14.0);
    }

    /// Centred rows of wrapped text: a greedy word wrap at `width`, one row per line.
    fn wrapped(&mut self, text: &str, size: f32, colour: Color32, width: f32) {
        let font = FontId::proportional(size);
        let space = self.painter.layout_no_wrap(" ".to_owned(), font.clone(), colour).size().x;
        let mut lines: Vec<String> = Vec::new();
        let mut line = String::new();
        let mut used = 0.0;
        for word in text.split_whitespace() {
            let w = self.painter.layout_no_wrap(word.to_owned(), font.clone(), colour).size().x;
            if !line.is_empty() && used + space + w > width {
                lines.push(std::mem::take(&mut line));
                used = 0.0;
            }
            if !line.is_empty() {
                line.push(' ');
                used += space;
            }
            line.push_str(word);
            used += w;
        }
        if !line.is_empty() {
            lines.push(line);
        }
        for line in lines {
            self.text(&line, size, colour);
        }
    }

    /// The cone guide: each line with a cone of its colour, the lines' block centred.
    fn cones(&mut self, lines: &str) {
        let font = FontId::proportional(15.0);
        let galleys: Vec<_> =
            lines.lines().map(|line| self.painter.layout_no_wrap(line.to_owned(), font.clone(), TEXT)).collect();
        let width = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 22.0;
        let left = self.x - width / 2.0;
        for (line, galley) in lines.lines().zip(galleys) {
            let height = galley.size().y;
            if let Some(colour) = cone_colour(line) {
                cone(self.painter, Pos2::new(left + 7.0, self.y + height / 2.0), colour);
            }
            self.painter.galley(Pos2::new(left + 22.0, self.y), galley, TEXT);
            self.y += height + 4.0;
        }
    }

    /// The lesson's times side by side, each a small label over its value; `reached` (the verdict's tier) is gold.
    fn times(&mut self, school: &State, reached: Option<u32>) {
        let passed = reached.is_some_and(|v| (1..4).contains(&v));
        let cells = [("PASS", school.pass, passed), ("PERFECT", school.perfect, reached == Some(4))];
        for (i, (name, time, hit)) in cells.into_iter().enumerate() {
            let x = self.x + if i == 0 { -80.0 } else { 80.0 };
            let at = |dy| Pos2::new(x, self.y + dy);
            self.painter.text(at(0.0), Align2::CENTER_TOP, name, FontId::proportional(12.0), DIM);
            let colour = if hit { GOLD } else { TEXT };
            self.painter.text(at(16.0), Align2::CENTER_TOP, seconds(time), FontId::proportional(20.0), colour);
        }
        self.y += 44.0;
    }
}

/// The colour a cone-guide line names first, if any.
fn cone_colour(line: &str) -> Option<Color32> {
    let first = line.split_whitespace().next()?;
    Some(match first.to_ascii_lowercase().as_str() {
        "red" => Color32::from_rgb(220, 60, 50),
        "green" => Color32::from_rgb(70, 190, 80),
        "blue" => Color32::from_rgb(70, 130, 230),
        "yellow" => Color32::from_rgb(240, 200, 40),
        _ => return None,
    })
}

/// A small traffic cone centred on `at`.
fn cone(painter: &Painter, at: Pos2, colour: Color32) {
    let (top, base) = (Pos2::new(at.x, at.y - 7.0), at.y + 6.0);
    let shape = egui::Shape::convex_polygon(
        vec![top, Pos2::new(at.x + 4.5, base), Pos2::new(at.x - 4.5, base)],
        colour,
        egui::Stroke::NONE,
    );
    painter.add(shape);
    painter.rect_filled(Rect::from_center_size(Pos2::new(at.x, base + 1.0), Vec2::new(12.0, 2.0)), 1.0, colour);
}

/// A panel behind what `body` paints: an invisible pass measures the body's height, the second paints it.
fn panel(painter: &Painter, screen: Rect, body: impl Fn(&mut Column)) {
    let top = screen.min.y + screen.height() * TOP;
    let mut hidden = painter.clone();
    hidden.set_invisible();
    let mut measure = Column { painter: &hidden, x: 0.0, y: 0.0 };
    body(&mut measure);
    let rect = Rect::from_min_size(Pos2::new(screen.center().x - WIDTH / 2.0, top), Vec2::new(WIDTH, measure.y + 36.0));
    painter.rect_filled(rect, 8.0, PANEL);
    body(&mut Column { painter, x: rect.center().x, y: top + 18.0 });
}

/// The brief's panel: the lesson's label and title, the instruction, the cone guide, the times when timed, and the
/// confirm's key.
fn brief_panel(painter: &Painter, screen: Rect, school: &State) {
    let brief = brief(school.lesson);
    panel(painter, screen, |c| {
        c.text(&label(school.lesson), 13.0, GOLD);
        if let Some(brief) = brief {
            c.text(brief.title, 26.0, TEXT);
            c.gap(2.0);
            c.text(brief.instruction, 16.0, TEXT);
            if brief.cones != "None" {
                c.rule();
                c.cones(brief.cones);
            }
        }
        if school.timed() {
            c.rule();
            c.times(school, None);
        }
        c.gap(10.0);
        c.text("start: Enter", 13.0, DIM);
    });
}

/// A short toast under the progress line.
fn toast_panel(painter: &Painter, screen: Rect, toast: Toast) {
    let panel = Rect::from_center_size(Pos2::new(screen.center().x, screen.min.y + 96.0), Vec2::new(240.0, 40.0));
    painter.rect_filled(panel, 8.0, PANEL);
    painter.text(panel.center(), Align2::CENTER_CENTER, toast.text(), FontId::proportional(20.0), GOLD);
}

/// The end's panel: the lesson, the verdict with its grade, the failed end's reasons, the time and the
/// lesson's times.
fn verdict_panel(painter: &Painter, screen: Rect, school: &State, verdict: u32) {
    let title = brief(school.lesson).map(|b| b.title.to_uppercase());
    panel(painter, screen, |c| {
        c.text(&format!("LESSON {} · {}", number(school.lesson), title.as_deref().unwrap_or_default()), 13.0, DIM);
        let colour = match verdict {
            4 => GOLD,
            1..=3 => TEXT,
            _ => FAILED,
        };
        c.text(verdict_text(verdict), 30.0, colour);
        if verdict == 0 {
            for reason in &school.reasons {
                c.wrapped(reason, 15.0, TEXT, WIDTH - 80.0);
            }
        }
        if verdict > 0 {
            grade(c.painter, Pos2::new(c.x, c.y + 4.0), verdict);
            c.gap(22.0);
        }
        c.text(&time_label(f64::from(school.time)), 22.0, TEXT);
        if school.timed() {
            c.rule();
            c.times(school, Some(verdict));
        }
    });
}

/// Four pips centred on `at`, the grade's filled.
fn grade(painter: &Painter, at: Pos2, verdict: u32) {
    for i in 0..4 {
        let centre = Pos2::new(at.x + (i as f32 - 1.5) * 22.0, at.y + 6.0);
        if i < verdict {
            painter.circle_filled(centre, 6.0, GOLD);
        } else {
            painter.circle_stroke(centre, 6.0, (1.5, DIM));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbr_school::Lesson;

    /// The keys the briefs come from (`Lesson::title_key`, `brief_keys`, and the number labels).
    fn keys(lesson: Lesson) -> (String, (String, String)) {
        let number = if lesson.index() < Lesson::FIRST_ADVANCED {
            format!("RSM_LESSON_NRB{}", lesson.index() + 1)
        } else {
            format!("RSM_LESSON_NRA{}", lesson.index() - Lesson::FIRST_ADVANCED + 1)
        };
        (number, lesson.brief_keys())
    }

    /// The texts are the English table's (`RBR_DATA`).
    #[test]
    fn the_texts_are_the_games() {
        let Some(data) = rbr_formats::data::Data::from_env() else { return };
        let Ok(strings) = rbr_formats::strings::english(&data) else { return };
        let game = |key: &str| strings.get(key).unwrap().trim_end().replace('\n', " ");
        for lesson in Lesson::all() {
            let (number_key, (instruction, cones)) = keys(lesson);
            let brief = brief(lesson.index()).unwrap();
            assert_eq!(format!("Lesson {}.", number(lesson.index())), game(&number_key), "{lesson:?}");
            assert_eq!(brief.title, game(lesson.title_key()), "{lesson:?}");
            assert_eq!(brief.instruction, game(&instruction), "{lesson:?}");
            assert_eq!(brief.cones.replace('\n', " "), game(&cones), "{lesson:?}");
        }
        assert_eq!(brief(Lesson::COUNT), None);
    }

    #[test]
    fn verdicts_read_the_grades() {
        assert_eq!(verdict_text(4), "Perfect");
        assert_eq!(verdict_text(1), "Passed");
        assert_eq!(verdict_text(3), "Passed");
        assert_eq!(verdict_text(0), "Failed");
    }

    #[test]
    fn incidents_map_to_their_toasts() {
        assert_eq!(Toast::of(0), None);
        assert_eq!(Toast::of(1), None);
        assert_eq!(Toast::of(2), Some(Toast::TooSlow));
        assert_eq!(Toast::of(3), Some(Toast::TooSlow));
        assert_eq!(Toast::of(4), Some(Toast::TooFast));
        assert_eq!(Toast::of(5), Some(Toast::TooFast));
        assert_eq!(Toast::of(6), None);
    }

    #[test]
    fn states_follow_the_record() {
        let record = Record { lesson: 2, state: school::STATE_BRIEF, ..Record::default() };
        let state = State::of(&record);
        assert_eq!((state.lesson, state.phase, state.verdict), (2, Phase::Brief, None));
        assert!(!state.timed());
        let record = Record {
            lesson: 2,
            state: school::STATE_ENDED,
            verdict: 3,
            time: 9.4,
            perfect: 9.0,
            pass: 10.0,
            incident: 0,
            ..Record::default()
        };
        let state = State::of(&record);
        assert_eq!((state.phase, state.verdict), (Phase::Ended, Some(3)));
        assert!(state.timed());
        assert_eq!(Phase::of(99), Phase::Driving);
    }
}
