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

/// What the school drive shows: the lesson, where it stands and its verdict, with the toast showing now.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
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
}

impl State {
    /// The record's state; the toast is set where the record is taken ([`super::model::HudModel::set_school`]).
    pub fn of(record: &Record) -> State {
        State {
            lesson: record.lesson,
            phase: Phase::of(record.state),
            verdict: (record.verdict != school::NO_VERDICT).then_some(record.verdict),
            time: record.time,
            perfect: record.perfect,
            pass: record.pass,
            toast: None,
        }
    }

    /// Whether the lesson is timed.
    pub fn timed(self) -> bool {
        self.perfect != 0.0 || self.pass != 0.0
    }
}

/// The lesson's brief: its title, instruction and cone guide. `None` past the last lesson.
pub fn brief(lesson: u32) -> Option<Brief> {
    BRIEFS.get(lesson as usize).copied()
}

/// The lesson's number label, as the game's (`RSM_LESSON_NRB1`..`NRB6`, `RSM_LESSON_NRA1`..`NRA11`): group-relative,
/// like the launcher's.
pub fn number(lesson: u32) -> String {
    let n = if lesson < rbr_gameplay::school::Lesson::FIRST_ADVANCED { lesson + 1 } else { lesson - 5 };
    format!("Lesson {n}.")
}

/// A verdict's line: 4 perfect, 1..3 passed with the grade, else failed.
pub fn verdict_text(verdict: u32) -> String {
    match verdict {
        4 => "Perfect".to_owned(),
        n @ 1..=3 => format!("Passed {n}/4"),
        _ => "Failed".to_owned(),
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
const GOLD: Color32 = Color32::from_rgb(255, 196, 0);
const FAILED: Color32 = Color32::from_rgb(220, 70, 60);

/// A time in seconds, as the game's school texts write it (`RSM_TIMELIMITA6` is "Pass Time: 10s").
fn seconds(time: f32) -> String {
    format!("{} s", time as u32)
}

/// Draws the lesson's screens over the HUD: the brief's panel, the incident toast, or the verdict panel.
pub fn paint(ctx: &egui::Context, state: &HudView) {
    let Some(school) = &state.school else { return };
    match school.phase {
        Phase::Brief => brief_panel(ctx, school),
        Phase::Driving => {
            if let Some(toast) = school.toast {
                toast_panel(ctx, toast);
            }
        }
        Phase::Ended => {
            if let Some(verdict) = school.verdict {
                verdict_panel(ctx, school, verdict);
            }
        }
    }
}

/// A centred panel: the title, the lines and the footer, sized to them.
fn panel(painter: &Painter, screen: Rect, title: &str, title_colour: Color32, lines: &[String], footer: &str) {
    let rows = lines.iter().map(|line| line.lines().count()).sum::<usize>();
    let width = 600.0;
    let height = 64.0 + 20.0 * rows as f32 + if footer.is_empty() { 12.0 } else { 44.0 };
    let panel = Rect::from_center_size(screen.center(), Vec2::new(width, height));
    painter.rect_filled(panel, 8.0, PANEL);
    let mut at = Pos2::new(panel.center().x, panel.min.y + 14.0);
    painter.text(at, Align2::CENTER_TOP, title, FontId::proportional(24.0), title_colour);
    at.y += 40.0;
    for line in lines.iter().flat_map(|block| block.lines()) {
        painter.text(at, Align2::CENTER_TOP, line, FontId::proportional(16.0), TEXT);
        at.y += 20.0;
    }
    if !footer.is_empty() {
        painter.text(Pos2::new(at.x, panel.max.y - 26.0), Align2::CENTER_TOP, footer, FontId::proportional(13.0), DIM);
    }
}

/// The brief's panel: the lesson's number and title, the instruction and the cone guide, the times when timed,
/// and the confirm's key. The gamepad's South confirms too, but no HUD string names it.
fn brief_panel(ctx: &egui::Context, school: &State) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("school-brief")));
    let screen = ctx.content_rect();
    let title = match brief(school.lesson) {
        Some(brief) => format!("{} · {}", number(school.lesson), brief.title),
        None => number(school.lesson),
    };
    let mut lines = Vec::new();
    if let Some(brief) = brief(school.lesson) {
        lines.push(brief.instruction.to_owned());
        if brief.cones != "None" {
            lines.push(brief.cones.to_owned());
        }
    }
    if school.timed() {
        lines.push(format!("Pass time: {}", seconds(school.pass)));
        lines.push(format!("Perfect time: {}", seconds(school.perfect)));
    }
    panel(&painter, screen, &title, TEXT, &lines, "Press Enter to start");
}

/// A short toast under the top of the screen.
fn toast_panel(ctx: &egui::Context, toast: Toast) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("school-toast")));
    let screen = ctx.content_rect();
    let panel = Rect::from_center_size(Pos2::new(screen.center().x, screen.min.y + 96.0), Vec2::new(280.0, 44.0));
    painter.rect_filled(panel, 8.0, PANEL);
    painter.text(panel.center(), Align2::CENTER_CENTER, toast.text(), FontId::proportional(20.0), TEXT);
}

/// The end's panel: the verdict, the time and the lesson's times.
fn verdict_panel(ctx: &egui::Context, school: &State, verdict: u32) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("school-verdict")));
    let screen = ctx.content_rect();
    let mut lines = vec![format!("Time: {}", time_label(f64::from(school.time)))];
    if school.timed() {
        lines.push(format!("Pass time: {}", seconds(school.pass)));
        lines.push(format!("Perfect time: {}", seconds(school.perfect)));
    }
    let colour = match verdict {
        4 => GOLD,
        0 => FAILED,
        _ => TEXT,
    };
    panel(&painter, screen, &verdict_text(verdict), colour, &lines, "");
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbr_gameplay::school::Lesson;

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
            assert_eq!(number(lesson.index()), game(&number_key), "{lesson:?}");
            assert_eq!(brief.title, game(lesson.title_key()), "{lesson:?}");
            assert_eq!(brief.instruction, game(&instruction), "{lesson:?}");
            assert_eq!(brief.cones.replace('\n', " "), game(&cones), "{lesson:?}");
        }
        assert_eq!(brief(Lesson::COUNT), None);
    }

    #[test]
    fn verdicts_read_the_grades() {
        assert_eq!(verdict_text(4), "Perfect");
        assert_eq!(verdict_text(1), "Passed 1/4");
        assert_eq!(verdict_text(3), "Passed 3/4");
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
        };
        let state = State::of(&record);
        assert_eq!((state.phase, state.verdict), (Phase::Ended, Some(3)));
        assert!(state.timed());
        assert_eq!(Phase::of(99), Phase::Driving);
    }
}
