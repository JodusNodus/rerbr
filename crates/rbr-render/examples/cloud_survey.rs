//! Full-sky cloud opacity, without game files: cargo run --release -p rbr-render --example cloud_survey -- OUT
//! Optional following arguments: weather control override (or `preset`), width (multiple of 16, default 256), time
//! in seconds (default 0). Example: cloud_survey OUT preset 512 900, to check another resolution and wind phase.
//! Each PNG is azimuth horizontally and sine of elevation vertically, horizon at the top, zenith at the bottom.
//! RGB are low cloud, cirrus and combined opacity. CSV reports area above opacity 0.1 and 0.5, and mean opacity.

use glam::Vec3;
use rbr_render::atmosphere::{Atmosphere, Clouds};
use rbr_render::renderer::{instance, request_gpu, CloudSurvey};
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = std::path::Path::new(args.first().ok_or("usage: cloud_survey OUT [CONTROL|preset] [WIDTH] [SECONDS]")?);
    let control = args.get(1).filter(|s| s.as_str() != "preset").map(|s| s.parse::<f32>()).transpose()?;
    let width = args.get(2).map(|s| s.parse::<u32>()).transpose()?.unwrap_or(256);
    let time = args.get(3).map(|s| s.parse::<f32>()).transpose()?.unwrap_or(0.0);
    if control.is_some_and(|v| !v.is_finite()) || !time.is_finite() {
        return Err("control and time must be finite".into());
    }
    std::fs::create_dir_all(out)?;
    let gpu = request_gpu(&instance(), None)?;
    let survey = CloudSurvey::new(&gpu);
    let mut csv = std::fs::File::create(out.join("coverage.csv"))?;
    writeln!(csv, "preset,section,position,time,control,low_visible,low_opaque,low_mean,cirrus_visible,cirrus_opaque,cirrus_mean,total_visible,total_opaque,total_mean,horizon_opaque")?;
    for (name, clouds) in
        [("clear", Clouds::Clear), ("partly", Clouds::Partly), ("light", Clouds::Light), ("heavy", Clouds::Heavy)]
    {
        let mut coverage = Vec::new();
        let mut horizon_coverage = Vec::new();
        for (seed, section) in
            ["607O_M_lightrain_lightcloud", "41M_crisp_clear", "45O_N_norain_lightcloud", "532O_M_crisp_clear"]
                .iter()
                .enumerate()
        {
            let atmosphere = Atmosphere {
                section: Some((*section).into()),
                clouds: Some(clouds),
                sun_direction: Vec3::new(0.6, -0.8, 0.0),
                ..Atmosphere::default()
            };
            for (position, eye) in
                [Vec3::ZERO, Vec3::new(3000.0, 0.0, 5000.0), Vec3::new(-6000.0, 0.0, 2000.0)].into_iter().enumerate()
            {
                let samples = survey.measure(&atmosphere, eye, time, control, width)?;
                write!(
                    csv,
                    "{name},{section},{position},{time},{}",
                    control.unwrap_or_else(|| atmosphere.cloud_layer())
                )?;
                assert!(samples.iter().all(|p| p.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))));
                for c in 0..3 {
                    let count = samples.len() as f64;
                    let visible = samples.iter().filter(|p| p[c] > 0.1).count() as f64 / count;
                    let opaque = samples.iter().filter(|p| p[c] > 0.5).count() as f64 / count;
                    let mean = samples.iter().map(|p| f64::from(p[c])).sum::<f64>() / count;
                    assert!(mean.is_finite() && (0.0..=1.0).contains(&mean));
                    write!(csv, ",{visible:.6},{opaque:.6},{mean:.6}")?;
                    if c == 2 {
                        coverage.push(opaque);
                    }
                }
                // A 0..10-degree horizon band, also sampled uniformly in solid angle.
                let rows = ((width / 2) as f32 * 10.0_f32.to_radians().sin()).round() as usize;
                let horizon = &samples[..rows * width as usize];
                let opaque = horizon.iter().filter(|p| p[2] > 0.5).count() as f64 / horizon.len() as f64;
                horizon_coverage.push(opaque);
                writeln!(csv, ",{opaque:.6}")?;
                let path = out.join(format!("{name}-{seed}-{position}.png"));
                let mut png = png::Encoder::new(std::fs::File::create(path)?, width, width / 2);
                png.set_color(png::ColorType::Rgb);
                let pixels: Vec<u8> = samples
                    .iter()
                    .flat_map(|p| p[..3].iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
                    .collect();
                png.write_header()?.write_image_data(&pixels)?;
            }
        }
        println!(
            "{name}: opaque sky mean {:.1}%, min {:.1}%, max {:.1}%; horizon {:.1}% ({} fields)",
            100.0 * coverage.iter().sum::<f64>() / coverage.len() as f64,
            100.0 * coverage.iter().copied().fold(1.0, f64::min),
            100.0 * coverage.iter().copied().fold(0.0, f64::max),
            100.0 * horizon_coverage.iter().sum::<f64>() / horizon_coverage.len() as f64,
            coverage.len()
        );
    }
    Ok(())
}
