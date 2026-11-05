//! Probes the roadside (`rbr_render::roadside`) along a drive's poses for `rbr-sound --roadside`, and times it.
//! Usage: `roadside_probe STAGE POSES OUT`: `POSES` is `rbr-sound --poses`'s CSV (`step,x,y,z,fx,fy,fz`, physics
//! frame), `OUT` gets `step` and the car's left, right and overhead as `distance,size,hardness` each. Prints the
//! probe's cost a call (mean, 99th percentile, worst).

use std::error::Error;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Instant;

use glam::Vec3;
use rbr_formats::data::Data;
use rbr_render::roadside::Roadside;
use rbr_render::stage_probe::StageProbe;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let (stage, poses, out) = match (args.next(), args.next(), args.next()) {
        (Some(s), Some(p), Some(o)) => (s, p, o),
        _ => return Err("usage: roadside_probe STAGE POSES OUT".into()),
    };
    let data = Data::from_env().ok_or("RBR_DATA is not set")?;
    let ground = StageProbe::load(&data, &stage).map(Arc::new);
    let roadside = Roadside::load(&data, &stage, ground);
    eprintln!("roadside_probe: {stage}: {} scenery posts", roadside.posts().len());
    let mut csv = String::from("step,ld,ls,lh,rd,rs,rh,od,os,oh\n");
    let mut times = Vec::new();
    for line in std::fs::read_to_string(&poses)?.lines().skip(1) {
        let v: Vec<f32> = line.split(',').map(str::parse).collect::<Result<_, _>>()?;
        let [step, x, y, z, fx, fy, fz] = v[..] else { return Err(format!("{line}: 7 fields").into()) };
        let started = Instant::now();
        let heard = std::hint::black_box(roadside.probe(Vec3::new(x, y, z), Vec3::new(fx, fy, fz)));
        times.push(started.elapsed().as_secs_f64() * 1e6);
        write!(csv, "{}", step as u32)?;
        for s in heard.sides.iter().chain([&heard.overhead]) {
            write!(csv, ",{:.3},{:.4},{:.3}", s.distance, s.size, s.hardness)?;
        }
        csv.push('\n');
    }
    std::fs::write(out, csv)?;
    times.sort_by(f64::total_cmp);
    let n = times.len().max(1);
    let mean = times.iter().sum::<f64>() / n as f64;
    let p99 = times.get(n * 99 / 100).copied().unwrap_or(0.0);
    let worst = times.last().copied().unwrap_or(0.0);
    eprintln!("roadside_probe: {} probes, {mean:.1} us mean, {p99:.1} us p99, {worst:.1} us worst", times.len());
    Ok(())
}
