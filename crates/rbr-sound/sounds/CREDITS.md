# Sound credits

Recordings embedded in `rbr-sound` (`data.rs`, `include_bytes!`) for the water sounds (`tyre::Water`) and the
windscreen's wipers, splats and in-car rain (`data::GlassBank`). Every other
sound comes from the player's own `audio.dat`. All files are mono 16-bit PCM WAV at 44100 Hz, made from the
Freesound HQ previews (128 kbit/s MP3; the originals need a login).

| File | Source | Author | Licence | Edits |
|---|---|---|---|---|
| `splash1.wav`, `splash2.wav`, `splash3.wav` | [VW Golf GTI Water Skid (Zoom H2n M&S).wav](https://freesound.org/people/EverydaySounds/sounds/268625/) | EverydaySounds | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) | The car's three runs through water (4.36-5.70 s, 16.62-17.95 s, 30.76-31.90 s), mixed to mono, 40 Hz high-pass, 15 ms fade in, 250 ms fade out, the loudest 0.3 s at -14 dBFS RMS. |
| `wet_road.wav` | [Passing Car (Wet road)](https://freesound.org/people/Breviceps/sounds/462862/) | Breviceps | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) | The pass's peak (2.0-2.8 s, about 100 km/h), mixed to mono, 40 Hz high-pass, the level flattened (50 ms RMS), looped with a 100 ms crossfade, -12 dBFS RMS. |
| `wiper_wet1.wav`, `wiper_wet2.wav`, `wiper_motor.wav` | [Car wipers](https://freesound.org/people/domenicodr/sounds/607129/) | domenicodr | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) | In-car wipers (tagged rain): the strokes at 2.495-2.945 s (turning at the top) and 3.190-3.640 s (setting out), 80 Hz high-pass, 5 ms fade in, 120 ms fade out, the loudest 50 ms at -15 and -18 dBFS; the motor from the gaps between strokes (2.80-3.19, 3.50-3.86, 4.16-4.54, 4.85-5.21 s) joined with 40 ms crossfades, 150 Hz high-pass, 3 kHz low-pass, looped with an 80 ms crossfade, -24 dBFS RMS. All mixed to mono. |
| `wiper_dry1.wav`, `wiper_dry2.wav` | [Car wipers scrape on dry glass.wav](https://freesound.org/people/tosha73/sounds/547356/) | tosha73 | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) | The strokes at 21.075-21.525 s and 22.095-22.545 s, mixed to mono, 80 Hz high-pass, 5 ms fade in, 120 ms fade out, the loudest 50 ms at -15 and -18 dBFS. |
| `rain_light.wav` | [Light Rain inside a car, driver seat](https://freesound.org/people/leocb/sounds/849916/) | leocb | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) | 20.0-23.25 s, mixed to mono, 60 Hz high-pass, looped with a 250 ms crossfade, -22 dBFS RMS. |
| `rain_heavy.wav` | [Rain on the roof of car inside 5](https://freesound.org/people/Cinetony/sounds/672703/) | Cinetony | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) | 40.0-43.05 s, mixed to mono, 60 Hz high-pass, looped with a 250 ms crossfade, -24.7 dBFS RMS (peak-limited at -0.1 dBFS). |
| `glass_splat1.wav`, `glass_splat2.wav`, `glass_splat3.wav` | [agua contra el auto.wav](https://freesound.org/people/Mirandat7/sounds/614993/) | Mirandat7 | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) | The three buckets of water hitting the car (1.86-2.41 s, 7.70-8.25 s, 10.57-11.12 s), mixed to mono, 60 Hz high-pass, 5 ms fade in, 200 ms fade out, the loudest 0.1 s at -16 dBFS. |
