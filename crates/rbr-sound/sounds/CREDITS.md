# Sound credits

Recordings embedded in `rbr-sound` (`data.rs`, `include_bytes!`) for the water sounds (`tyre::Water`). Every other
sound comes from the player's own `audio.dat`. All files are mono 16-bit PCM WAV at 44100 Hz, made from the
Freesound HQ previews (128 kbit/s MP3; the originals need a login).

| File | Source | Author | Licence | Edits |
|---|---|---|---|---|
| `splash1.wav`, `splash2.wav`, `splash3.wav` | [VW Golf GTI Water Skid (Zoom H2n M&S).wav](https://freesound.org/people/EverydaySounds/sounds/268625/) | EverydaySounds | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) | The car's three runs through water (4.36-5.70 s, 16.62-17.95 s, 30.76-31.90 s), mixed to mono, 40 Hz high-pass, 15 ms fade in, 250 ms fade out, the loudest 0.3 s at -14 dBFS RMS. |
| `wet_road.wav` | [Passing Car (Wet road)](https://freesound.org/people/Breviceps/sounds/462862/) | Breviceps | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) | The pass's peak (2.0-2.8 s, about 100 km/h), mixed to mono, 40 Hz high-pass, the level flattened (50 ms RMS), looped with a 100 ms crossfade, -12 dBFS RMS. |
