//! The bushes and bendable trees a drive's car body is in (`rbr_render::verge::touches`), for `rbr-sound --verge`.
//! Usage: `verge_probe STAGE POSES OUT`: `POSES` is `rbr-sound --poses`'s CSV (`step,x,y,z,fx,fy,fz`, physics
//! frame), `OUT` gets `step,id,depth,density,branches` a contact.

use std::error::Error;
use std::fmt::Write as _;

use glam::Vec3;
use rbr_formats::data::Data;
use rbr_render::roadside::Roadside;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let (stage, poses, out) = match (args.next(), args.next(), args.next()) {
        (Some(s), Some(p), Some(o)) => (s, p, o),
        _ => return Err("usage: verge_probe STAGE POSES OUT".into()),
    };
    let data = Data::from_env().ok_or("RBR_DATA is not set")?;
    let roadside = Roadside::load(&data, &stage, None);
    let grown = roadside.posts().iter().filter(|p| p.growth.is_some()).count();
    let mut csv = String::from("step,id,depth,density,branches\n");
    let (mut steps, mut contacts) = (0, 0);
    for line in std::fs::read_to_string(&poses)?.lines() {
        let Ok(v) = line.split(',').map(str::parse).collect::<Result<Vec<f32>, _>>() else { continue };
        let [step, x, y, z, fx, fy, fz] = v[..] else { return Err(format!("{line}: 7 fields").into()) };
        let touches = rbr_render::verge::touches(&roadside, Vec3::new(x, y, z), Vec3::new(fx, fy, fz));
        steps += usize::from(touches[0].is_some());
        for t in touches.iter().flatten() {
            contacts += 1;
            writeln!(csv, "{},{},{:.3},{:.2},{}", step as u32, t.id, t.depth, t.density, u8::from(t.branches))?;
        }
    }
    std::fs::write(out, csv)?;
    eprintln!("verge_probe: {stage}: {grown} bushes and bendable trees, {steps} steps in contact, {contacts} contacts");
    Ok(())
}
