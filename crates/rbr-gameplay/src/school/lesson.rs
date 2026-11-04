//! The rally school's lessons (flow mode 7): each lesson's map, car setup and tyre, and the flow's lesson tests.
//! The lesson is the flow's `+0xca4` (`E59Extra::M5657b0`, 0x5657b0, sets it).

/// The car every lesson drives: `LoadSchool` (0x48df80) loads car 5's setup options and `StartQuickRace` (0x481660)
/// gets car 5 from the school flow (`M0047def0(map, 0, 5, -1)`, `E60Flow::M5693f0` case 2).
pub const CAR: u32 = 5;

/// A rally school lesson, 0 to 16 (6 basic, 11 advanced).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Lesson(u32);

impl Lesson {
    /// The number of lessons.
    pub const COUNT: u32 = 17;
    /// The first advanced lesson: 0-5 are the basic ones (`RSCD_BASIC`), 6-16 the advanced (`RSCD_ADV`).
    pub const FIRST_ADVANCED: u32 = 6;

    /// Lesson `index`, if there is one.
    pub fn new(index: u32) -> Option<Lesson> {
        (index < Self::COUNT).then_some(Lesson(index))
    }

    /// Every lesson in order.
    pub fn all() -> impl Iterator<Item = Lesson> {
        (0..Self::COUNT).map(Lesson)
    }

    /// The flow's `+0xca4` word.
    pub fn index(self) -> u32 {
        self.0
    }

    /// The lesson's map (`E59Gameflow::M5657f0`, 0x5657f0; its default case is lesson 11's 0x56).
    pub fn map(self) -> u32 {
        [81, 87, 83, 82, 80, 79, 78, 77, 76, 84, 85, 86, 73, 88, 89, 75, 74][self.0 as usize]
    }

    /// The car setup `LoadSchool` (0x48df80) parses in place of the car's `setups/d_<surface>.lsp`, by the map:
    /// tarmac on maps 0x49, 0x55 and 0x56, the soft suspension's on 0x58, gravel on the others.
    pub fn setup_file(self) -> &'static str {
        match self.map() {
            0x49 | 0x55 | 0x56 => "Physics/school/tarmac.lsp",
            0x58 => "Physics/school/sfgravel.lsp",
            _ => "Physics/school/gravel.lsp",
        }
    }

    /// The tyre the stage starts on (`E60_M00566250`, 0x566250, in mode 7): lessons 10-12 the dry tarmac tyre 0, the
    /// others the gravel tyre 3.
    pub fn tyre(self) -> u32 {
        if (10..=12).contains(&self.0) {
            0
        } else {
            3
        }
    }

    /// `E59Host90::M00565920` (0x565920): the lessons the flow neither times nor publishes (0, 4, 10, 13, 14, 16).
    pub fn untimed(self) -> bool {
        matches!(self.0, 0 | 4 | 10 | 13 | 14 | 16)
    }

    /// `E59Host90::M00565960` (0x565960): lessons 4, 5, 13 and 16, which the school's pages treat apart.
    pub fn apart(self) -> bool {
        matches!(self.0, 4 | 5 | 13 | 16)
    }

    /// Lesson 5, the ride with Richard Burns (`E71PageOps::M5659a0`, 0x5659a0): no judging, the best result given.
    pub fn ride(self) -> bool {
        self.0 == 5
    }

    /// The lesson's title key in the string tables (`CRSExampleDescUI`'s and `CSchoolAwardUI`'s table).
    pub fn title_key(self) -> &'static str {
        [
            "SU_BASIC_CNTR",
            "BD_AND_G_CHANGE",
            "BRK_T_GRAVEL",
            "POS_A_GEAR_CHNG",
            "QUAL_TRIAL",
            "TAKE_A_RIDE",
            "HBT_GRAVEL",
            "OVRS1_LFBR",
            "OVRS2_LO",
            "PWRSLIDE",
            "TAR_B_HANDLING",
            "BRK_T_TARMAC",
            "HBT_TARMAC",
            "SFT_SUSP",
            "HRD_SUSP",
            "SCND_FLICK",
            "SPST_TRIAL",
        ][self.0 as usize]
    }

    /// The lesson's instruction and cone-guide keys (`CRSLessonInstructionUI`): `B1SI`..`B6SI` for the basic lessons,
    /// `A1SI`..`A12SI` for the advanced (the tables have no `A7`).
    pub fn brief_keys(self) -> (String, String) {
        let tag = match self.0 {
            0..=5 => format!("B{}", self.0 + 1),
            6..=11 => format!("A{}", self.0 - 5),
            _ => format!("A{}", self.0 - 4),
        };
        (format!("{tag}SI"), format!("{tag}SA"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lessons_map_onto_the_school_maps_once_each() {
        let mut maps: Vec<u32> = Lesson::all().map(Lesson::map).collect();
        maps.sort_unstable();
        assert_eq!(maps, (73..=89).collect::<Vec<_>>());
        assert_eq!(Lesson::new(17), None);
    }

    #[test]
    fn the_tarmac_lessons_take_the_tarmac_setup_and_tyre() {
        for lesson in Lesson::all() {
            let tarmac = lesson.setup_file().ends_with("tarmac.lsp");
            assert_eq!(tarmac, (10..=12).contains(&lesson.index()), "{lesson:?}");
            assert_eq!(lesson.tyre() == 0, tarmac, "{lesson:?}");
        }
        assert_eq!(Lesson(13).setup_file(), "Physics/school/sfgravel.lsp");
    }

    #[test]
    fn brief_keys_skip_a7() {
        assert_eq!(Lesson(0).brief_keys(), ("B1SI".into(), "B1SA".into()));
        assert_eq!(Lesson(11).brief_keys(), ("A6SI".into(), "A6SA".into()));
        assert_eq!(Lesson(12).brief_keys(), ("A8SI".into(), "A8SA".into()));
        assert_eq!(Lesson(16).brief_keys(), ("A12SI".into(), "A12SA".into()));
    }
}
