//! The developer commands of `rbr-dirt-rally`: reading a car's or a stage's files by hand.
use std::path::Path;

/// The commands for reading a car's or a stage's files by hand, and the unknown ones.
pub fn run(command: &str, mut args: impl Iterator<Item = String>) -> Result<(), String> {
    match command {
        "xml" => {
            let path = args.next().ok_or("xml needs a binary XML file")?;
            let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
            fn show(e: &crate::binxml::Element, depth: usize) {
                println!("{}{} {:?} {}", "  ".repeat(depth), e.name, e.attrs, e.text.as_deref().unwrap_or(""));
                e.children.iter().for_each(|c| show(c, depth + 1));
            }
            show(&crate::binxml::parse(&bytes)?, 0);
            Ok(())
        }
        "pacenotes" => pacenotes(args),
        "pacenotes-eval" => pacenotes_eval(),
        "pssg-tree" => {
            let path = args.next().ok_or("pssg-tree needs a .pssg")?;
            let depth: usize = args.next().map_or(Ok(3), |d| d.parse().map_err(|e| format!("depth: {e}")))?;
            fn show(n: crate::pssg::Node, depth: usize, left: usize) {
                let attrs: Vec<String> = n
                    .attrs()
                    .filter(|(k, _)| !matches!(*k, "size" | "id"))
                    .map(|(k, v)| match crate::pssg::decode_str(v) {
                        Some(s) => format!("{k}={s}"),
                        None if v.len() == 4 => format!("{k}={}", u32::from_be_bytes([v[0], v[1], v[2], v[3]])),
                        None => format!("{k}=[{} bytes]", v.len()),
                    })
                    .collect();
                println!("{}{} {} {}", "  ".repeat(depth), n.name(), n.id().unwrap_or(""), attrs.join(" "));
                if left > 0 {
                    n.children().for_each(|c| show(c, depth + 1, left - 1));
                }
            }
            crate::pssg::Pssg::read(Path::new(&path))?.roots().for_each(|n| show(n, 0, depth));
            Ok(())
        }
        "car-textures" => {
            let path = args.next().ok_or("car-textures needs a livery .pssg")?;
            for t in crate::texture::read_car(&crate::pssg::Pssg::read(Path::new(&path))?) {
                println!("{:<40} {}x{} {:?} {} levels", t.name, t.width, t.height, t.format, t.levels);
            }
            Ok(())
        }
        "car-texture" => {
            let (path, name, out) = (args.next(), args.next(), args.next());
            let (Some(path), Some(name), Some(out)) = (path, name, out) else {
                return Err("car-texture needs a .pssg, a texture name and an output .dds".into());
            };
            let textures = crate::texture::read_car(&crate::pssg::Pssg::read(Path::new(&path))?);
            let texture = textures.iter().find(|t| t.name == name).ok_or("no such texture")?;
            std::fs::write(out, texture.dds()).map_err(|e| e.to_string())
        }
        "car-info" => {
            let path = args.next().ok_or("car-info needs a <car>_highlod.pssg")?;
            let pssg = crate::pssg::Pssg::read(Path::new(&path))?;
            let parts =
                if pssg.by_id("LOD0_").is_some() { crate::car::parts(&pssg)? } else { crate::car::interior(&pssg)? };
            print!("{}", crate::car::info(&parts, args.next().as_deref() == Some("-v")));
            print!("{}", crate::car::materials(&pssg));
            // An interior's needles, by its config beside it, in DiRT's frame.
            if let Ok(config) = std::fs::read_to_string(Path::new(&path).with_extension("xml")) {
                print!("{}", rbr_formats::car_gauges::write(&crate::needles::read(&pssg, &config)));
            }
            Ok(())
        }
        other => Err(format!("unknown command {other}; see --help")),
    }
}

/// `pacenotes ROUTE [--line]`: the route's calls against the corners and notes placed on its line.
fn pacenotes(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let id = args.next().ok_or("pacenotes needs LOCATION/AREA/route_N")?;
    let install = crate::cli::install(None)?;
    let route = install.route(&id)?;
    let course = crate::route::course(&route)?;
    let (line, start, finish, _) = crate::import::centre_line(&route, &course)?;
    // The line as the stage keeps it: 50 m before the start, 2.5 m apart.
    let from = (start - 50.).max(0.);
    let line = line.stretch(from, (finish + 40.).min(line.length()) - from, true, 2.5);
    let start = start - from;
    if args.next().as_deref() == Some("--line") {
        for (i, p) in line.points.iter().enumerate() {
            println!("L\t{:.1}\t{:.2}\t{:.2}\t{:.2}\t{:.2}", line.at[i] - start, p[0], p[1], p[2], line.width[i]);
        }
    }
    let calls = crate::route::calls(&route)?;
    let placed = crate::pacenotes::place(&calls, &line.points, &line.at, start);
    let all = crate::pacenotes::notes(&calls, &line.points, &line.at, start, finish);
    let count = |t: u32| all.iter().filter(|n| n.tag == t).count();
    eprintln!(
        "{} notes: {} corners, {} crests, {} jumps, {} distance keys",
        all.len(),
        placed.len(),
        count(16),
        count(20),
        count(13)
    );
    println!("call\tdr_m\tsubcalls(type/angle/mA/mB/link)\ttag\tnote_m\tcorner(start-end m, turn, radius)");
    for (i, call) in calls.iter().enumerate() {
        let subs: Vec<String> = call
            .subcalls
            .iter()
            .map(|s| format!("{}/{}/{}/{}/{}", s.kind, s.angle, s.modifier_a, s.modifier_b, s.distance_link))
            .collect();
        let notes: Vec<_> = placed.iter().filter(|p| p.call == i).collect();
        let at = line.at.partition_point(|a| *a < start + call.distance).min(line.at.len() - 1);
        let info = format!("z={:.1} width={:.1}", line.points[at][2], line.width[at]);
        if notes.is_empty() {
            println!("{i}\t{:.1}\t{}\t-\t-\t{info}", call.distance, subs.join(" "));
        }
        for p in notes {
            let corner = p.corner.map_or("none".into(), |c| {
                format!(
                    "{:.0}-{:.0} {}{:.0} r{:.0}",
                    c.start - start,
                    c.end - start,
                    if c.left { "L" } else { "R" },
                    c.turn,
                    c.radius
                )
            });
            println!(
                "{i}\t{:.1}\t{}\t{}\t{:.1}\t{corner}",
                call.distance,
                subs.join(" "),
                p.note.tag,
                p.note.distance - start
            );
        }
    }
    Ok(())
}

/// `pacenotes-eval`: how the calls place on every rally route's line, per area (routes without calls are skipped).
fn pacenotes_eval() -> Result<(), String> {
    use std::collections::BTreeMap;
    let install = crate::cli::install(None)?;
    // Per area: notes, matched, side agreements, |turn - angle| of the matched, notes closer than 15 m.
    let mut areas: BTreeMap<String, (usize, usize, usize, Vec<f64>, usize, Vec<f64>, usize, Vec<f64>)> =
        BTreeMap::new();
    for route in install.routes().into_iter().filter(|r| r.is_rally()) {
        let calls = crate::route::calls(&route)?;
        if calls.is_empty() {
            continue;
        }
        let course = crate::route::course(&route)?;
        let (line, start, finish, _) = crate::import::centre_line(&route, &course)?;
        let from = (start - 50.).max(0.);
        let line = line.stretch(from, (finish + 40.).min(line.length()) - from, true, 2.5);
        let start = start - from;
        let placed = crate::pacenotes::place(&calls, &line.points, &line.at, start);
        let row = areas.entry(format!("{}/{}", route.location, route.area)).or_default();
        row.0 += placed.len();
        for p in &placed {
            if let Some(c) = p.corner {
                row.1 += 1;
                row.2 += usize::from(c.left == p.tag_left);
                row.3.push((c.turn - p.angle).abs());
                row.5.push(c.start - start - calls[p.call].distance);
            }
        }
        {
            let v = crate::pacenotes::speeds(&line.points, &line.at);
            let mut seen = std::collections::HashSet::new();
            let mut d: Vec<f64> = placed
                .iter()
                .filter_map(|p| {
                    let c = p.corner?;
                    let spoken = start + calls[p.call].distance;
                    let i = line.at.partition_point(|a| *a < spoken).min(v.len() - 1);
                    seen.insert(p.call).then(|| c.start - spoken - 3.0 * v[i])
                })
                .collect();
            d.sort_by(f64::total_cmp);
            let m = d.get(d.len() / 2).copied().unwrap_or(0.0);
            row.7.extend(d.iter().map(|x| x - m));
        }
        let mut flipped = calls.clone();
        flipped.iter_mut().flat_map(|c| &mut c.subcalls).for_each(|s| s.kind = 3 - s.kind.min(2));
        let f = crate::pacenotes::place(&flipped, &line.points, &line.at, start)
            .iter()
            .filter(|p| p.corner.is_some())
            .count();
        row.6 += f;
        row.4 += placed.windows(2).filter(|w| w[1].note.distance - w[0].note.distance < 15.0).count();
    }
    let pct = |v: &mut Vec<f64>, q: f64| {
        v.sort_by(f64::total_cmp);
        v.get(((v.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0.0)
    };
    let mad = |v: &mut Vec<f64>| {
        let m = pct(v, 0.5);
        let mut d: Vec<f64> = v.iter().map(|x| (x - m).abs()).collect();
        1.4826 * pct(&mut d, 0.5)
    };
    println!(
        "area\tnotes\tmatched%\tside%\tflipped%\tmedian|d|\tp90|d|\tclose<15m\tlead_q1\tlead_med\tlead_q3\tresid_mad"
    );
    let mut total = (0, 0, 0, Vec::new(), 0, Vec::new(), 0, Vec::new());
    for (area, mut r) in areas {
        println!(
            "{area}\t{}\t{:.0}\t{:.0}\t{:.0}\t{:.1}\t{:.1}\t{}\t{:.0}\t{:.0}\t{:.0}\t{:.0}",
            r.0,
            100.0 * r.1 as f64 / r.0 as f64,
            100.0 * r.2 as f64 / r.1.max(1) as f64,
            100.0 * r.6 as f64 / r.0 as f64,
            pct(&mut r.3, 0.5),
            pct(&mut r.3, 0.9),
            r.4,
            pct(&mut r.5, 0.25),
            pct(&mut r.5, 0.5),
            pct(&mut r.5, 0.75),
            mad(&mut r.7)
        );
        total.0 += r.0;
        total.1 += r.1;
        total.2 += r.2;
        total.3.extend(r.3);
        total.4 += r.4;
        total.5.extend(r.5);
        total.6 += r.6;
        total.7.extend(r.7);
    }
    println!(
        "ALL\t{}\t{:.0}\t{:.0}\t{:.0}\t{:.1}\t{:.1}\t{}\t{:.0}\t{:.0}\t{:.0}\t{:.0}",
        total.0,
        100.0 * total.1 as f64 / total.0 as f64,
        100.0 * total.2 as f64 / total.1.max(1) as f64,
        100.0 * total.6 as f64 / total.0 as f64,
        pct(&mut total.3, 0.5),
        pct(&mut total.3, 0.9),
        total.4,
        pct(&mut total.5, 0.25),
        pct(&mut total.5, 0.5),
        pct(&mut total.5, 0.75),
        mad(&mut total.7)
    );
    Ok(())
}
