//! The retail sound data every model plays (`RBR_DATA`, and `RBR_PATCH` for the 1.02 `audio.ini`): each car's bank
//! loads, with the tyre, rumble, wetness, gravel-hit, off-road, impact, suspension, exhaust, fire and countdown samples and the co-driver's English words, numbers and damage
//! calls decoded; the scripted events' samples; the service park's banks.

use rbr_formats::data::Data;
use rbr_sound::data::CarBank;
use rbr_sound::menu::{MenuBank, MenuMixer};

#[test]
fn every_car_bank_loads_with_the_codriver() {
    let Some(data) = Data::from_env() else {
        eprintln!("RBR_DATA is not set; skipped");
        return;
    };
    for car in 0..8 {
        let bank = CarBank::load(&data, car).unwrap_or_else(|e| panic!("car {car}: {e}"));
        assert!(bank.surfaces.iter().flatten().all(|w| !w.samples.is_empty()), "car {car}: a surface loop is empty");
        assert_eq!((bank.offroad.len(), bank.ranged.len(), bank.screams.len()), (7, 20, 2));
        assert!(!bank.go.samples.is_empty());
        let counts =
            [&bank.grus, &bank.squeaks, &bank.bottomings, &bank.exhaust_pops, &bank.pops, &bank.bangs].map(Vec::len);
        assert_eq!(counts, [20, 18, 4, 11, 11, 6], "car {car}: effect banks");
        assert!([&bank.rumble, &bank.wetness, &bank.fire].iter().all(|w| !w.samples.is_empty()));
        let words = &bank.codriver.words;
        // Every word id but PLUS and MINUS (no takes in the archive) has at least one take.
        let silent: Vec<u32> = words.iter().filter(|(_, takes)| takes.is_empty()).map(|(&id, _)| id).collect();
        assert_eq!(silent.len(), 2, "car {car}: silent words {silent:x?}");
        assert!(words.len() >= 54);
        assert_eq!(bank.codriver.numbers.len(), 15);
        assert!(bank.codriver.damage[1..].iter().all(|w| w.rate > 0 && !w.samples.is_empty()));
        assert!(!bank.helicopter.samples.is_empty() && !bank.bird_hit.samples.is_empty());
    }
}

#[test]
fn the_service_park_plays() {
    let Some(data) = Data::from_env() else {
        eprintln!("RBR_DATA is not set; skipped");
        return;
    };
    let bank = MenuBank::load(&data).expect("menu bank");
    assert_eq!(bank.park.iter().map(Vec::len).collect::<Vec<_>>(), [1; 6]);
    let mut mixer = MenuMixer::new(bank, 44100, 1.0);
    mixer.set_park(true);
    let mut out = vec![[0.0; 2]; 44100 * 2];
    mixer.render(&mut out);
    let rms = (out[44100..].iter().map(|f| f[0] * f[0]).sum::<f32>() / 44100.0).sqrt();
    assert!(rms > 0.01, "rms {rms}");
}

#[test]
fn one_shots_are_refused_at_the_sound_limit_and_ranged_impacts_steal() {
    use rbr_sound::mixer::{Mixer, SOUND_LIMIT};
    use rbr_sound::voice::{Command, Shot};
    let Some(data) = Data::from_env() else {
        eprintln!("RBR_DATA is not set; skipped");
        return;
    };
    let bank = CarBank::load(&data, 0).expect("car 0");
    let mut mixer = Mixer::new(&bank, 44100, 1.0);
    let squeak = |i| Command::Shot { shot: Shot::Squeak(i), gain: 1.0 };
    // The requests until a frame all see its count: 9 sounds always count, so the squeaks filling the rest are made.
    let room = (SOUND_LIMIT - 9) as u32;
    mixer.apply(&(0..room as usize).map(squeak).collect::<Vec<_>>());
    mixer.render(&mut [[0.0; 2]; 64]);
    mixer.apply(&[Command::Frame]);
    mixer.apply(&[squeak(0), Command::Shot { shot: Shot::Ranged(0), gain: 1.0 }]);
    assert_eq!(mixer.tally()["Squeak"], [room, 1]);
    assert_eq!(mixer.tally()["Ranged"], [1, 0]);
}

#[test]
fn the_speech_queue_drops_nothing_past_ten_entries() {
    use rbr_sound::mixer::Mixer;
    use rbr_sound::voice::{Command, Speech};
    let Some(data) = Data::from_env() else {
        eprintln!("RBR_DATA is not set; skipped");
        return;
    };
    let bank = CarBank::load(&data, 0).expect("car 0");
    let mut mixer = Mixer::new(&bank, 44100, 1.0);
    let one = &bank.codriver.numbers[0];
    let seconds = one.samples.len() as f32 / one.rate as f32;
    mixer.apply(&[Command::Speech(Speech::Number(1)); 12]);
    let mut block = vec![[0.0f32; 2]; (seconds * 44100.0) as usize];
    // The 12th word plays after eleven others (the original's queue of 10 would be silent here).
    for _ in 0..11 {
        mixer.render(&mut block);
    }
    mixer.render(&mut block);
    assert!(block.iter().flatten().any(|s| s.abs() > 1e-4), "the twelfth queued word was dropped");
}

/// The engine's firing period in samples at `rpm`, from the autocorrelation of its render about the expected lag.
fn firing_period(mixer: &mut rbr_sound::mixer::Mixer, rpm: u32) -> usize {
    use rbr_sound::engine::EngineNoise;
    use rbr_sound::voice::Command;
    let noise = EngineNoise { target_rpm: rpm, stopped: false, load: 50, cylinders: [true; 4] };
    // The slew moves 500 rpm per input.
    for _ in 0..40 {
        mixer.apply(&[Command::Engine(noise)]);
        mixer.render(&mut [[0.0; 2]; 2048]);
    }
    assert_eq!(mixer.rpm(), rpm);
    let mut out = vec![[0.0f32; 2]; 16384];
    mixer.apply(&[Command::Engine(noise)]);
    mixer.render(&mut out);
    let x: Vec<f32> = out.iter().map(|s| s[0] + s[1]).collect();
    let expected = (44100.0 / (rpm as f32 * 0.033_333_335)) as usize;
    let score = |lag: usize| x.iter().zip(&x[lag..]).map(|(a, b)| a * b).sum::<f32>();
    (expected * 4 / 5..expected * 6 / 5).max_by(|&a, &b| score(a).total_cmp(&score(b))).unwrap()
}

#[test]
fn the_engine_pitch_keeps_rising_past_9000_rpm() {
    use rbr_sound::mixer::Mixer;
    let Some(data) = Data::from_env() else {
        eprintln!("RBR_DATA is not set; skipped");
        return;
    };
    let bank = CarBank::load(&data, 0).expect("car 0");
    let mut mixer = Mixer::new(&bank, 44100, 1.0);
    let (at_9000, at_12000) = (firing_period(&mut mixer, 9000), firing_period(&mut mixer, 12000));
    eprintln!("firing period: {at_9000} samples at 9000 rpm, {at_12000} at 12000");
    assert!(at_12000 * 5 < at_9000 * 4, "the pitch stopped rising: {at_9000} then {at_12000}");
}
