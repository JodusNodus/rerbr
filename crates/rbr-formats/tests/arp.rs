//! Reads every rally-school animated replay in the retail `Root/ghosts.rbz`
//! (`rbr_formats::arp`), writes it back byte-identical, and samples the demo car's path.
//!
//! Runs only when `RBR_DATA` points at the extracted install tree.

use rbr_formats::arp::{Kind, Replay};
use std::path::Path;

mod common;

#[test]
fn every_retail_arp_parses_round_trips_and_samples() {
    let Some(data) = common::data_dir() else {
        return;
    };
    let mut names = Vec::new();
    common::each_rbz_entry(&Path::new(&data).join("Root/ghosts.rbz"), |name, bytes| {
        if !name.ends_with(".arp") {
            return;
        }
        names.push(name.to_owned());
        let replay = Replay::parse(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(replay.write(), bytes, "{name}: written back differs");

        assert_eq!(replay.sections.len(), 1, "{name}: one section");
        assert!(replay.trailing.is_empty(), "{name}: no trailing snapshots");
        let section = &replay.sections[0];
        assert_eq!((section.tag, section.count as usize), (0, section.chunks.len()), "{name}");
        let head = section.head.as_ref().expect("{name}: a head");
        assert!(head.steer_scale.is_finite(), "{name}");

        let kinds: Vec<Kind> = section.chunks.iter().map(|c| c.kind).collect();
        assert!(kinds.contains(&Kind::Body), "{name}: a body chain");
        assert!(kinds.contains(&Kind::Steer), "{name}: a steer chain");
        for chunk in &section.chunks {
            assert!(!chunk.entries.is_empty(), "{name}: {:?}", chunk.kind);
            assert_eq!(
                chunk.filler.len(),
                chunk.kind.capacity() - chunk.entries.len() * chunk.kind.stride(),
                "{name}: {:?} filler fills the page",
                chunk.kind
            );
        }

        let duration = section.duration();
        assert!(duration.is_finite() && duration > 0.0, "{name}: duration {duration}");
        // The path is finite, unit-quaternion and continuous: sample finer than the
        // 0.08 s body quantum and bound each step well above the files' ~3.5 m max.
        let steps = (duration / 0.02).ceil() as usize;
        let mut prev = None;
        for i in 0..=steps {
            let pose = section.sample_body(i as f32 * 0.02).unwrap_or_else(|| panic!("{name}: no pose at step {i}"));
            assert!(pose.position.iter().all(|p| p.is_finite()), "{name}: step {i}");
            let norm = pose.rotation.iter().map(|q| q * q).sum::<f32>().sqrt();
            assert!((norm - 1.0).abs() < 1e-4, "{name}: step {i} norm {norm}");
            if let Some(prev) = prev {
                let step: f32 = pose.position.iter().zip(prev).map(|(a, b)| (a - b) * (a - b)).sum::<f32>().sqrt();
                assert!(step < 5.0, "{name}: step {i} jumps {step}");
            }
            prev = Some(pose.position);
            let steer = section.sample_steer(i as f32 * 0.02).expect("{name}: no steer");
            assert!(steer.is_finite(), "{name}: step {i}");
        }
        // `Query` clamps at or below 0 to the first entry; past the end it holds the last entry, which the
        // f32 duration only approaches (the span's last quantum boundary rounds down), so bound that gap
        // like any other step.
        assert_eq!(section.sample_body(-1.0), section.sample_body(0.0), "{name}");
        let (end, past) = (section.sample_body(duration).unwrap(), section.sample_body(duration + 100.0).unwrap());
        let gap: f32 = end.position.iter().zip(past.position).map(|(a, b)| (a - b) * (a - b)).sum::<f32>().sqrt();
        assert!(gap < 5.0, "{name}: end gap {gap}");
        eprintln!("{name}: {} chunks, {:.2} s", section.chunks.len(), duration);
    });
    assert_eq!(names.len(), 11, "the eleven rse lessons");
}
