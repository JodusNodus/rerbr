//! The championship's save: a versioned, line-based text in the player's folder.

use super::*;

impl Event {
    /// The event as text for the player's folder.
    pub fn save(&self) -> String {
        let mut out = format!("{SAVE_HEAD}\n");
        let c = &self.config;
        out.push_str(&format!(
            "C|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}\n",
            esc(&c.class),
            esc(&c.car),
            length_save(&c.length),
            pool_save(c.pool),
            surfaces_save(&c.surfaces),
            daylight_save(c.daylight),
            weather_save(c.weather),
            damage_save(c.damage),
            c.difficulty,
            if c.adaptive { 1 } else { 0 },
            c.field_size,
            c.seed,
            skill_save(c.skill),
            c.team
        ));
        out.push_str(&format!("P|{}\n", self.pace));
        for rally in &self.rallies {
            out.push_str(&format!("R|{}\n", esc(&rally.name)));
            for s in &rally.stages {
                let sections: Vec<String> = s
                    .track
                    .sections
                    .iter()
                    .map(|k| {
                        format!(
                            "{}|{}|{}|{}|{}|{}|{}",
                            k.par,
                            k.material,
                            k.checks[0].0,
                            k.checks[0].1,
                            k.checks[1].0,
                            k.checks[1].1,
                            k.accident_table
                        )
                    })
                    .collect();
                out.push_str(&format!(
                    "S|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}\n",
                    esc(&s.stage.key),
                    esc(&s.stage.name),
                    s.stage.surface.name(),
                    if s.stage.community { 1 } else { 0 },
                    s.stage.length,
                    s.clock,
                    conditions_save(s.conditions),
                    s.service_after.map_or(String::new(), |m| m.to_string()),
                    s.reference,
                    sections.join("|")
                ));
            }
        }
        for t in 0..self.teams.len() {
            let (team, costs) = (&self.teams[t], &self.costs[t]);
            let list = |v: &[i32]| v.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
            out.push_str(&format!(
                "M|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}\n",
                esc(&team.name),
                team.budget,
                team.manager_style,
                team.mechanics,
                team.engineering,
                list(&team.specialities),
                list(&team.difficulties),
                team.history,
                team.nationality,
                esc(&team.manager),
                costs.budget,
                costs.service,
                costs.rally,
                costs.style
            ));
        }
        for (e, c) in self.field.iter().zip(&self.contenders) {
            let (d, st, car) = (&c.driver.driver, &c.driver, &c.car);
            let join = |v: &[String]| v.join(",");
            let nums = |v: &[u32]| join(&v.iter().map(u32::to_string).collect::<Vec<_>>());
            let fields = [
                esc(&e.name),
                esc(&e.car),
                e.skill.to_string(),
                e.consistency.to_string(),
                c.team.to_string(),
                c.false_starts.to_string(),
                esc(&d.name),
                d.photo.to_string(),
                d.technique.to_string(),
                d.temperament.to_string(),
                join(&d.skills.map(|v| v.to_string())),
                d.speed.to_string(),
                d.endurance.to_string(),
                d.condition.to_string(),
                d.history.to_string(),
                d.nationality.to_string(),
                esc(&d.codriver),
                st.injury.to_string(),
                st.injury_len.to_string(),
                st.co_injury.to_string(),
                st.co_injury_len.to_string(),
                st.vs_par.to_string(),
                join(&st.sections.map(|v| v.to_string())),
                esc(&car.car.name),
                car.car.control.to_string(),
                car.car.power.to_string(),
                car.car.handling.to_string(),
                car.car.durability.to_string(),
                nums(&car.parts),
                nums(&car.hits),
            ];
            out.push_str(&format!("F|{}\n", fields.join("|")));
        }
        for (rally, stages) in self.results.iter().enumerate() {
            for (stage, (player, ai)) in stages.iter().enumerate() {
                let p = match player {
                    None => "-".to_owned(),
                    Some(r) if r.retired => "R".to_owned(),
                    Some(r) => format!("{}+{}", r.time, r.penalties),
                };
                let a: Vec<String> = ai
                    .iter()
                    .map(|r| match r {
                        AiResult::Time(t) => t.to_string(),
                        AiResult::Retired => "R".to_owned(),
                    })
                    .collect();
                out.push_str(&format!("T|{rally}|{stage}|{p}|{}\n", a.join(",")));
            }
        }
        for (rally, p) in self.penalties.iter().enumerate() {
            out.push_str(&format!("Y|{rally}|{}\n", p.iter().map(f32::to_string).collect::<Vec<_>>().join(",")));
        }
        for (rally, lines) in self.news.iter().enumerate() {
            for line in lines {
                out.push_str(&format!("N|{rally}|{}\n", esc(line)));
            }
        }
        out.push_str(&format!("U|{}\n", turn_save(&self.turn)));
        out
    }

    /// An event read back from [`Event::save`]'s text.
    pub fn load(text: &str) -> Result<Event, String> {
        let mut lines = text.lines();
        match lines.next() {
            Some(SAVE_HEAD) => {}
            Some(head) if head.starts_with("rbr-season ") => {
                return Err(format!("an older save ({head}): not readable"))
            }
            Some(head) => return Err(format!("not an rbr-season save: {head}")),
            None => return Err("empty save".to_owned()),
        }
        let mut config: Option<EventConfig> = None;
        let mut rallies: Vec<Rally> = Vec::new();
        let mut field: Vec<Entrant> = Vec::new();
        let mut contenders: Vec<Contender> = Vec::new();
        let mut teams: Vec<Team> = Vec::new();
        let mut costs: Vec<TeamCosts> = Vec::new();
        let mut pace = None;
        let mut penalties: Vec<Vec<f32>> = Vec::new();
        let mut news: Vec<Vec<String>> = Vec::new();
        let mut results: Vec<Vec<(Option<PlayerResult>, Vec<AiResult>)>> = Vec::new();
        let mut turn: Option<Turn> = None;
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let parts: Vec<String> = split(line);
            let mut f = Fields::new(&parts);
            match f.next().unwrap_or("") {
                "C" => {
                    f.expect(14, "config")?;
                    let (class, car) = (f.text()?, f.text()?);
                    let length = length_load(f.raw()?)?;
                    let pool = pool_load(f.raw()?)?;
                    let surfaces = surfaces_load(f.raw()?)?;
                    let daylight = daylight_load(f.raw()?)?;
                    let weather = weather_load(f.raw()?)?;
                    let damage = damage_load(f.raw()?)?;
                    let difficulty = f.num("difficulty")?;
                    let adaptive = match f.raw()? {
                        "1" => true,
                        "0" => false,
                        _ => return Err("bad adaptive".to_owned()),
                    };
                    let (field_size, seed) = (f.num("field size")?, f.num("seed")?);
                    let skill = skill_load(f.raw()?)?;
                    let team = f.num("team")?;
                    config = Some(EventConfig {
                        class,
                        car,
                        team,
                        length,
                        pool,
                        surfaces,
                        daylight,
                        weather,
                        damage,
                        skill,
                        difficulty,
                        adaptive,
                        field_size,
                        seed,
                    });
                }
                "P" => pace = Some(f.num("pace")?),
                "R" => {
                    f.expect(1, "rally")?;
                    rallies.push(Rally { name: f.text()?, stages: Vec::new() });
                    results.push(Vec::new());
                    penalties.push(Vec::new());
                    news.push(Vec::new());
                }
                "S" => {
                    f.expect(30, "stage")?;
                    let rally = rallies.last_mut().ok_or("stage outside a rally".to_owned())?;
                    let key = f.text()?;
                    let name = f.text()?;
                    let surface = surface_load(f.raw()?)?;
                    let community = match f.raw()? {
                        "1" => true,
                        "0" => false,
                        _ => return Err("bad community".to_owned()),
                    };
                    let length = f.num("length")?;
                    let clock = f.num("clock")?;
                    let conditions = conditions_load(f.raw()?)?;
                    let service_after = match f.raw()? {
                        "" => None,
                        m => Some(m.parse().map_err(|_| "bad service".to_owned())?),
                    };
                    let reference = f.num("reference")?;
                    let mut sections = Vec::new();
                    for _ in 0..3 {
                        sections.push(Section {
                            par: f.num("par")?,
                            material: f.num("material")?,
                            checks: [(f.num("check")?, f.num("check")?), (f.num("check")?, f.num("check")?)],
                            accident_table: f.num("accident table")?,
                        });
                    }
                    let track = Track { stage: 0, wet: 0, sections: [sections[0], sections[1], sections[2]] };
                    rally.stages.push(ItineraryStage {
                        stage: StageRef { key, name, surface, community, length },
                        clock,
                        conditions,
                        reference,
                        track,
                        service_after,
                    });
                    results.last_mut().ok_or("stage outside a rally".to_owned())?.push((None, Vec::new()));
                }
                "M" => {
                    f.expect(14, "team")?;
                    let team = Team {
                        name: f.text()?,
                        budget: f.num("budget")?,
                        manager_style: f.num("style")?,
                        mechanics: f.num("mechanics")?,
                        engineering: f.num("engineering")?,
                        specialities: f.list("specialities")?,
                        difficulties: f.list("difficulties")?,
                        history: f.num("history")?,
                        nationality: f.num("nationality")?,
                        manager: f.text()?,
                    };
                    costs.push(TeamCosts {
                        budget: f.num("budget")?,
                        service: f.num("service")?,
                        rally: f.num("rally cost")?,
                        style: f.num("style")?,
                    });
                    teams.push(team);
                }
                "F" => {
                    f.expect(30, "entrant")?;
                    let (name, car, skill, consistency) =
                        (f.text()?, f.text()?, f.num("skill")?, f.num("consistency")?);
                    let (team, false_starts) = (f.num("team")?, f.num("false starts")?);
                    let driver = rbr_formats::ai_ini::Driver {
                        name: f.text()?,
                        photo: f.num("photo")?,
                        technique: f.num("technique")?,
                        temperament: f.num("temperament")?,
                        skills: f.array("skills")?,
                        speed: f.num("speed")?,
                        endurance: f.num("endurance")?,
                        condition: f.num("condition")?,
                        history: f.num("history")?,
                        nationality: f.num("nationality")?,
                        codriver: f.text()?,
                    };
                    let mut state = crate::ai::DriverState::new(driver);
                    state.injury = f.num("injury")?;
                    state.injury_len = f.num("injury length")?;
                    state.co_injury = f.num("co-driver injury")?;
                    state.co_injury_len = f.num("co-driver injury length")?;
                    state.vs_par = f.num("vs par")?;
                    state.sections = f.array("sections")?;
                    let mut car_state = crate::ai::CarState::new(rbr_formats::ai_ini::Car {
                        name: f.text()?,
                        control: f.num("control")?,
                        power: f.num("power")?,
                        handling: f.num("handling")?,
                        durability: f.num("durability")?,
                    });
                    car_state.parts = f.array("parts")?;
                    car_state.hits = f.array("hits")?;
                    field.push(Entrant { name, car, skill, consistency });
                    contenders.push(Contender { team, driver: state, car: car_state, false_starts });
                }
                "T" => {
                    f.expect(4, "result")?;
                    let (rally, stage) = (f.num::<usize>("result rally")?, f.num::<usize>("result stage")?);
                    let player = match f.raw()? {
                        "-" => None,
                        "R" => Some(PlayerResult { time: 0.0, penalties: 0.0, retired: true }),
                        t => {
                            let (time, pen) = t.split_once('+').ok_or("bad player result".to_owned())?;
                            Some(PlayerResult {
                                time: time.parse().map_err(|_| "bad player time".to_owned())?,
                                penalties: pen.parse().map_err(|_| "bad penalties".to_owned())?,
                                retired: false,
                            })
                        }
                    };
                    let mut ai = Vec::new();
                    let cells = f.raw()?;
                    if !cells.is_empty() {
                        for cell in cells.split(',') {
                            ai.push(match cell {
                                "R" => AiResult::Retired,
                                t => AiResult::Time(t.parse().map_err(|_| "bad AI time".to_owned())?),
                            });
                        }
                    }
                    let row = results
                        .get_mut(rally)
                        .and_then(|r| r.get_mut(stage))
                        .ok_or("result outside the itinerary".to_owned())?;
                    *row = (player, ai);
                }
                "Y" => {
                    f.expect(2, "penalties")?;
                    let rally: usize = f.num("penalty rally")?;
                    let row = penalties.get_mut(rally).ok_or("penalties outside the itinerary".to_owned())?;
                    *row = f.list("penalties")?;
                }
                "N" => {
                    f.expect(2, "news")?;
                    let rally: usize = f.num("news rally")?;
                    news.get_mut(rally).ok_or("news outside the itinerary".to_owned())?.push(f.text()?);
                }
                "U" => {
                    f.expect(1, "turn")?;
                    turn = Some(turn_load(f.raw()?)?);
                }
                _ => return Err(format!("bad save line: {line}")),
            }
        }
        if field.len() != contenders.len() || teams.is_empty() || penalties.iter().any(|p| p.len() != field.len()) {
            return Err("an incomplete field".to_owned());
        }
        Ok(Event {
            config: config.ok_or("no config".to_owned())?,
            rallies,
            field,
            contenders,
            teams,
            costs,
            pace: pace.ok_or("no pace".to_owned())?,
            results,
            penalties,
            news,
            turn: turn.ok_or("no turn".to_owned())?,
        })
    }
}

/// The save's first line.
const SAVE_HEAD: &str = "rbr-season 2";

/// A save line's fields in order, read as the writer wrote them.
struct Fields<'a> {
    parts: std::slice::Iter<'a, String>,
    left: usize,
}

impl<'a> Fields<'a> {
    fn new(parts: &'a [String]) -> Self {
        Fields { parts: parts.iter(), left: parts.len() }
    }

    /// The line's tag (the first field).
    fn next(&mut self) -> Option<&'a str> {
        self.left = self.left.saturating_sub(1);
        self.parts.next().map(String::as_str)
    }

    /// The line has at least `n` fields after its tag.
    fn expect(&self, n: usize, what: &str) -> Result<(), String> {
        if self.left >= n {
            Ok(())
        } else {
            Err(format!("bad {what} line"))
        }
    }

    fn raw(&mut self) -> Result<&'a str, String> {
        self.parts.next().map(String::as_str).ok_or_else(|| "a short line".to_owned())
    }

    fn text(&mut self) -> Result<String, String> {
        self.raw().map(str::to_owned)
    }

    fn num<T: std::str::FromStr>(&mut self, what: &str) -> Result<T, String> {
        self.raw()?.parse().map_err(|_| format!("bad {what}"))
    }

    fn list<T: std::str::FromStr>(&mut self, what: &str) -> Result<Vec<T>, String> {
        let text = self.raw()?;
        if text.is_empty() {
            return Ok(Vec::new());
        }
        text.split(',').map(|v| v.parse().map_err(|_| format!("bad {what}"))).collect()
    }

    fn array<T: std::str::FromStr, const N: usize>(&mut self, what: &str) -> Result<[T; N], String> {
        self.list::<T>(what)?.try_into().map_err(|_| format!("bad {what}"))
    }
}

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '|' => out.push_str("\\p"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

fn split(line: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => cur.push('\\'),
                Some('p') => cur.push('|'),
                Some('n') => cur.push('\n'),
                Some(x) => {
                    cur.push('\\');
                    cur.push(x);
                }
                None => cur.push('\\'),
            }
        } else if c == '|' {
            parts.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    parts.push(cur);
    parts
}

fn length_save(length: &Length) -> String {
    match length {
        Length::Rally { stages } => format!("rally:{stages}"),
        Length::Weekend { stages } => format!("weekend:{stages}"),
        Length::Season { rallies, stages } => format!("season:{rallies}:{stages}"),
    }
}

fn length_load(text: &str) -> Result<Length, String> {
    let mut it = text.split(':');
    match (it.next(), it.next(), it.next(), it.next()) {
        (Some("rally"), Some(n), None, _) => {
            Ok(Length::Rally { stages: n.parse().map_err(|_| "bad rally length".to_owned())? })
        }
        (Some("weekend"), Some(n), None, _) => {
            Ok(Length::Weekend { stages: n.parse().map_err(|_| "bad weekend length".to_owned())? })
        }
        (Some("season"), Some(r), Some(n), None) => Ok(Length::Season {
            rallies: r.parse().map_err(|_| "bad season rallies".to_owned())?,
            stages: n.parse().map_err(|_| "bad season stages".to_owned())?,
        }),
        _ => Err("bad length".to_owned()),
    }
}

fn pool_save(pool: StagePool) -> &'static str {
    match pool {
        StagePool::Retail => "retail",
        StagePool::Community => "community",
        StagePool::All => "all",
    }
}

fn pool_load(text: &str) -> Result<StagePool, String> {
    match text {
        "retail" => Ok(StagePool::Retail),
        "community" => Ok(StagePool::Community),
        "all" => Ok(StagePool::All),
        _ => Err("bad pool".to_owned()),
    }
}

fn surfaces_save(surfaces: &Surfaces) -> String {
    match surfaces {
        Surfaces::Only(s) => format!("only:{}", s.name()),
        Surfaces::Mixed => "mixed".to_owned(),
    }
}

fn surfaces_load(text: &str) -> Result<Surfaces, String> {
    match text {
        "mixed" => Ok(Surfaces::Mixed),
        only => match only.strip_prefix("only:") {
            Some(name) => Ok(Surfaces::Only(surface_load(name)?)),
            None => Err("bad surfaces".to_owned()),
        },
    }
}

fn surface_load(text: &str) -> Result<crate::config::Surface, String> {
    use crate::config::Surface;
    match text {
        "tarmac" => Ok(Surface::Tarmac),
        "gravel" => Ok(Surface::Gravel),
        "snow" => Ok(Surface::Snow),
        _ => Err("bad surface".to_owned()),
    }
}

fn daylight_save(daylight: Daylight) -> &'static str {
    match daylight {
        Daylight::Day => "day",
        Daylight::Realistic => "realistic",
        Daylight::Night => "night",
    }
}

fn daylight_load(text: &str) -> Result<Daylight, String> {
    match text {
        "day" => Ok(Daylight::Day),
        "realistic" => Ok(Daylight::Realistic),
        "night" => Ok(Daylight::Night),
        _ => Err("bad daylight".to_owned()),
    }
}

fn weather_save(weather: Weather) -> &'static str {
    match weather {
        Weather::Dry => "dry",
        Weather::Changeable => "changeable",
        Weather::Wet => "wet",
    }
}

fn weather_load(text: &str) -> Result<Weather, String> {
    match text {
        "dry" => Ok(Weather::Dry),
        "changeable" => Ok(Weather::Changeable),
        "wet" => Ok(Weather::Wet),
        _ => Err("bad weather".to_owned()),
    }
}

fn damage_save(damage: crate::config::Damage) -> &'static str {
    use crate::config::Damage;
    match damage {
        Damage::Off => "off",
        Damage::Reduced => "reduced",
        Damage::Realistic => "realistic",
    }
}

fn damage_load(text: &str) -> Result<crate::config::Damage, String> {
    use crate::config::Damage;
    match text {
        "off" => Ok(Damage::Off),
        "reduced" => Ok(Damage::Reduced),
        "realistic" => Ok(Damage::Realistic),
        _ => Err("bad damage".to_owned()),
    }
}

fn skill_save(skill: crate::config::Skill) -> &'static str {
    use crate::config::Skill;
    match skill {
        Skill::Rookie => "rookie",
        Skill::Professional => "professional",
        Skill::Champion => "champion",
    }
}

fn skill_load(text: &str) -> Result<crate::config::Skill, String> {
    use crate::config::Skill;
    match text {
        "rookie" => Ok(Skill::Rookie),
        "professional" => Ok(Skill::Professional),
        "champion" => Ok(Skill::Champion),
        _ => Err("bad skill".to_owned()),
    }
}

fn conditions_save(conditions: Conditions) -> &'static str {
    match conditions {
        Conditions::Dry => "dry",
        Conditions::Damp => "damp",
        Conditions::Wet => "wet",
    }
}

fn conditions_load(text: &str) -> Result<Conditions, String> {
    match text {
        "dry" => Ok(Conditions::Dry),
        "damp" => Ok(Conditions::Damp),
        "wet" => Ok(Conditions::Wet),
        _ => Err("bad conditions".to_owned()),
    }
}

fn turn_save(turn: &Turn) -> String {
    match turn {
        Turn::Welcome { rally } => format!("welcome:{rally}"),
        Turn::Stage { rally, stage } => format!("stage:{rally}:{stage}"),
        Turn::Service { rally, stage, minutes } => format!("service:{rally}:{stage}:{minutes}"),
        Turn::RallyOver { rally } => format!("rallyover:{rally}"),
        Turn::EventOver => "over".to_owned(),
    }
}

fn turn_load(text: &str) -> Result<Turn, String> {
    let mut it = text.split(':');
    match (it.next(), it.next(), it.next(), it.next(), it.next()) {
        (Some("welcome"), Some(r), None, _, _) => {
            Ok(Turn::Welcome { rally: r.parse().map_err(|_| "bad turn".to_owned())? })
        }
        (Some("stage"), Some(r), Some(s), None, _) => Ok(Turn::Stage {
            rally: r.parse().map_err(|_| "bad turn".to_owned())?,
            stage: s.parse().map_err(|_| "bad turn".to_owned())?,
        }),
        (Some("service"), Some(r), Some(s), Some(m), None) => Ok(Turn::Service {
            rally: r.parse().map_err(|_| "bad turn".to_owned())?,
            stage: s.parse().map_err(|_| "bad turn".to_owned())?,
            minutes: m.parse().map_err(|_| "bad turn".to_owned())?,
        }),
        (Some("rallyover"), Some(r), None, _, _) => {
            Ok(Turn::RallyOver { rally: r.parse().map_err(|_| "bad turn".to_owned())? })
        }
        (Some("over"), None, _, _, _) => Ok(Turn::EventOver),
        _ => Err("bad turn".to_owned()),
    }
}
