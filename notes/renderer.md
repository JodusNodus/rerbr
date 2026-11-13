# Renderer and visual systems

`crates/rbr-render`: the wgpu renderer, the driving window (`viewer`) and every visual system. Cameras, HUD, live
mode and the launcher: [cameras-hud-launcher.md](cameras-hud-launcher.md); sound: [sound.md](sound.md). Module
map: [crates/rbr-render/README.md](../crates/rbr-render/README.md). Dropped detail (surveys, tuning and timing runs):
rbr-recomp `rust/rbr-render/README.md`, `notes/visual-enhancements.md`.

## Decisions

- Presentation never changes the physics: it reads frames and payloads after a step and writes nothing back.
- Visuals and sound may be better than the original. The base look follows 1.02's data and render states;
  `--enhance` additions are ours.
- Particles aim to look good, not to match 1.02: no bit-exact state, no harness comparison.
- The windscreen gets a modern water-on-glass system in place of 1.02's screen-fixed `NParticleSystem2D` sprites.
- macOS (Metal) and Linux (Vulkan), portable code. `shader_check.rs` validates every WGSL module and override set
  through naga to SPIR-V as wgpu's Vulkan backend does. Reversed-edge `smoothstep` is undefined on Metal.
- wgpu 30, naga 30, egui 0.37 (2026-10-25, from 27 and 0.33). wgpu 30 checks the fragment stage's sampled textures at
  the pipeline layout (the reflections read 17; `request_gpu` asks the adapter's limit), refuses to reconfigure a
  surface while another thread submits (the loading thread: `present::configure` reports it and the windows try
  again next frame), and reports a hidden macOS window as occluded instead of waiting up to a second for a drawable
  (`present::Acquired::Hidden`: the viewer runs the frame undrawn, the live process stepping, at most every 16 ms).
  egui 0.37 has no default features (`default_fonts` named) and draws glyphs a few levels apart from 0.33.
- Windows ships DXC (`dxcompiler.dll` beside the programs, `scripts/package.sh`): wgpu 30's `Dx12Compiler::Auto`
  loads it and falls back to FXC without it. Under WARP (Windows 11 26100, a VM without a GPU) FXC's shaders crash the
  live drive at its first frame, in `d3d10warp!ShaderJIT::ReadPointerOperand` compiling a pixel shader (3 of 3 runs,
  heap corruption or access violation); with DXC it drives (2026-11-03). Static runs never hit that shader.
- The vertex entry points read `instance_index` themselves and hand it on (`drawn` in `shader.wgsl`). naga 30's HLSL
  adds wgpu's first instance to DX12's `SV_InstanceID`, which starts at 0 in every draw, only where an entry point
  reads it, not in a function it passes the vertex to (`naga::back::FunctionCtx::is_fixed_function_input`). Read in
  `placed`, the car's parts kept in its model space (instance 1, the live drive) stayed at the stage's origin on
  Windows: no body, only the wheels and the seats (2026-11-03).
- Highest quality always: the full `PS_*.ini`, the high renderer's LOD and visibility distances.
- The backdrop `(1,3)` comes in two kinds, and both must look right: low terrain with baked vertex light (retail,
  most community stages) and upright photo panorama strips with alpha sky and white vertex colour (many community stages, 462
  Odry; "Panorama backdrops" below). Backdrop code may not take it as flat, facing up or reaching the horizon: the eye
  can see over a strip's top. Check changes on both (41M and 462M, 138M).

## Architecture

- Data only through `rbr-formats`. `RBR_DATA` is the retail 1.0 install; `RBR_PATCH` (1.02's `Main_group`)
  overrides it per file (`rbr_formats::data::Data`), e.g. the car's `Fog/` shaders.
- Conventions (`coords.rs`, the one place): stage positions as stored, left-handed, y up, matching wgpu clip space;
  reversed depth (stage positions `@invariant` for the depth prepass on Vulkan); front faces clockwise; vertex colour
  D3DCOLOR (B, G, R, A); no sRGB conversion, non-sRGB 8-bit surface as D3D9.
- Physics is z-up: `stage = (x, z, y)`. The car model's front is `-z`, `+x` its left; `model_to_body` is a mirror so
  model to stage is a proper rotation. A part sits at `F * P * v`: `F` the node's chained world, `P` the 14-word
  transform `T * R1 * R2 * S * R2^-1` (`HH_H7ReadTransform` 0x1000c7a0, `coords::part_to_car`).
- `renderer/`: pipelines per render state, BC uploads, shadows, reflection cubes, the frame's passes (`frame.rs`).
  One sample a pixel: cut-outs (`cutout`) anti-aliased by TAA (below), the alpha test alone with
  `--anti-aliasing off`. 4x MSAA, alpha to coverage and `depth_copy.rs` are gone (2026-10-26); reflections and
  light passes read the single-sample depth directly. `--reflections high|medium|low|off` (`Reflections`, `enhance.rs`; `on` = high); low halves the steps (wet 41M 14.2 to
  12.8 ms, 2.6% of pixels over 8; 45O similar, 2026-10-26). Off: wet roads and water take the cube and sky alone
  (wet 1080p 394M 26.4 to 10.5 ms; 41M similar, 2026-10-25).
  Reflections trace at half size (`reflection_composite.wgsl`): 41M
  chase 12.8 to 6.5 ms (45O similar); thin community-car reflections softer. Additions off: only water differs (44E max 17/255;
  71M 52 pixels over 8) (2026-10-26).
  Rays (2026-11-05, `ssr.wgsl`, `renderer/depth_pyramid.rs`; after AMD FidelityFX SSSR, ideas only): each is a GGX
  visible-normal sample (Heitz 2018) of the surface's cone (alpha `SSR_ALPHA` 0.25 of its roughness times stretch,
  the outer 30% of the lobe left out) traversed through a nearest-depth pyramid (R32F, 9 levels, built each frame by
  compute) coarse to fine, `Reflections::rays` the rays and the cells a ray visits; a ray behind something skips 4
  cells, doubling. Replaced the linear march with a hand-spread cone and its two entry points (one for the additions,
  one without): screen reflections at high, RX 9060 XT, wet 41M 1080p 1.63 to 1.05 ms, 4K 7.7 to 3.97 ms; 45O chase
  0.84 to 0.83 and 3.34 to 3.00 ms; medium and low 0.05 ms slower where little reflects (the pyramid). Dry and fog
  byte-identical; night lamp streaks on wet tarmac a little crisper, lakes' bank reflections fuller; 41M's roadside
  sign reflects shorter and one at the bend not at all (the pixel-exact march stops at the verge grass the linear
  steps stepped over). Not run on Metal (naga's SPIR-V check only).
  Cost at 4K (2026-11-06, RX 9060 XT, offscreen, medians of 3 interleaved; LA wet 127 quattro chase, LAc its cockpit,
  B 450 heavy rain 131 cockpit, dry 21 chase): the composite writes the frame with its reflection in one pass (no copy
  and blend; the glass and effects after it, timed as "particles, glass, rain"), nothing is traced when nothing
  reflecting is in view, the full-size trace is depth-tested, the pyramid's first two levels are one dispatch. Screen
  reflections LA 5.78 to 4.78 ms, LAc 5.14 to 2.58 (+1.57 glass), B 5.90 to 2.76 (+2.40 glass), dry 1.21 to 0.99;
  4K shots RMSE at most 0.00035. Where the full-size trace goes, LA: about 1 ms a pixel whatever the rays (0.76 of it
  `puddled`), 0.7 to 0.8 ms a ray, scene mips 0.33, composite 0.41, pyramid 0.16; the hits' colour fetch is
  negligible. Above 1440 rows Ultra traces split (visual trade, user decision 2026-11-06: for everyone, no setting):
  the reflecting draws write their surface (normal, kind, weight) at full size, the rays go from the 2x2's most
  reflecting pixel at half size, and each pixel takes the hits by depth and by how like its own normal theirs is
  (`NORMAL_MATCH`). Then the resolve (`fs_resolve`) rebuilds the hit at the texels' mean hit distance along the
  mirrored direction of the pixel's own normal and of their mean normal, and adds the frame's brightness difference
  between the two, kept within the texels' range and at least half their reflection: the ripples come back crisp as at
  full size (Luceram RMSE 0.0192, Pengonpohja 0.0134; no specks over 30 frames at 60 fps). Screen reflections LA 2.89
  ms, LAc 1.79, B 1.82; GPU frame LA 19.65 to 17.85. Reading at the pixel's own direction alone was darker and
  patchier (RMSE 0.033); the colour difference unclamped left lone dark, blue and yellow pixels. Tried and dropped:
  half size above 1440 rows (LA 2.29 ms, but the ripple in the car's image gone), persistent ray lanes (slower), a
  central ray plus refine, a 1440p-sized trace (ripples lost), one ray a pixel interleaved (sparkle), a mirror ray
  only (another look), finest pyramid level 1 (-0.2 ms, streaks), a longer skip (no gain), normal matching on puddles
  only (softer puddles).
  The car's glass in the cockpit (2026-11-06, `car_glass` in `renderer/frame.rs`: the car's draws that write no depth,
  the panes and the interior set's sun strip) goes into the frame after the rays are traced, before the composite: the
  depth under it is the stage's, so a ray that met the stage there read the glass's colour (the Lancer's red strip in
  the far puddles, wet 21O). The windscreen's film still sees it; outside the car it stays in the frame (the car's
  image in the road keeps its windows). 21O cockpit 4K: scene 5.74 to 5.19 ms, screen reflections 1.89 to 2.32, GPU
  frame within noise; chase and dry byte-identical.
- `--anti-aliasing taa|off` (`AntiAliasing`, default `Taa`, ours after gp4's `taa.rs`; `4x` parses as `taa`): one
  sample, the main view's projection jittered and resolved into the post pass's scene, sharpened under TAA
  (`taa.rs`, `taa.wgsl`, `post.rs`: Halton 2,3, 8 phases an output pixel, `taa::phases`, 18 at 67 % and 32 at 50 %;
  the reprojection, history, glass mask and reset rules are there).
  Measured 2026-10-26 (RX 9060 XT radv, `--enhance all`, 1440p, Laplacian variance): 4x / TAA / TAA sharpened
  forest chase 251 / 136 / 230, cockpit 174 / 95 / 120 (TAA roughly halves the aliasing, sharpening restores about
  half; dry, wet, rain and night scenes similar). `--anti-aliasing off` frames are byte-identical to before in
  13 scenes. Tried and removed: a negative texture mip bias (0 to -1; 201 to 205, no change).
  Motion (`renderer/motion.rs`, `motion.wgsl`, 2026-11-01, after UE's velocity pass): after the main view's stage pass
  the meshes that move of themselves are drawn again with `Equal` depth into an `Rgba16Float` target: the uv motion
  since the last frame, z 1 for moving, 2 for the car's meshes. The car's model-space parts use its previous body; the
  meshes given in stage space (wheels, steering wheel, wipers, ghost, characters, fences, props) keep their last drawn
  positions per vertex buffer (vertex buffer 1). Every depth-writing draw, blended too: the gauges' markings and the
  M3's grille are blended draws that write depth, and without them the glass mask took them new each frame (fog
  Gabiria M3 cockpit RMSE 698). The resolve takes that motion where z is set, else the camera's from the depth; it
  replaced the car's screen box and the steering wheel's and wipers' rooms (`taa::Room`). The box gave the road under
  the car the car's motion (smeared gravel behind a chase car, 208 Helette); the wheels now carry their roll. 34 stills
  within RMSE 9/65535 of before, `--anti-aliasing off` byte-identical; wipers and wheel as solid as before (GC8 Luceram
  rain, 8-frame sequence). Pass 0.06-0.34 ms, 1080p RX 9060 XT (208 chase the most); TAA as before.
  Upscale (`fs_upscale` in `taa.wgsl`, 2026-11-01, after Qualcomm's Snapdragon GSR 2, BSD-3): below 100 % render scale
  the resolve writes the output's size (history and post pass there) instead of the post pass stretching the frame. Each
  output pixel gathers the 3x3 jittered samples by a Gaussian of their distance in output pixels, at a share of 0.1 over
  the scale per unit of weight (`taa.rs` `KERNEL_AREA`); the clip box is the unweighted 3x3 (weighted by nearness:
  stair-steps, speckled grass); the motion is the 3x3's nearest depth's; where clipped hard or moving fast (3/4 at 32
  output pixels a frame, 64 before 2026-11-05: a chase camera's roadside smudged) the history gives way to a Catmull-Rom of the frame held
  within the 3x3 (the nearest sample drew blocks, a Gaussian smudged). Textures biased `log2(scale)` in the main view
  (`texture_bias` in `shader.wgsl`; Laplacian variance 50 % flyby 122 to 163). Rain and particles go into the frame after
  a copy (`Taa::keep`) and their difference is added, out of the history. The reference shots draw one or two frames, so
  compare 32-frame sequences (`--sequence 32 --sequence-step 12`, last frame). Laplacian variance, 1080p, chase 208 /
  cockpit Fiesta / flyby Clio / rain cockpit GC8: native 572 / 437 / 739 / 1980; old stretch 67 % 112 / 129 / 153 / 608,
  50 % 43 / 71 / 62 / 276; upscale 67 % 442 / 273 / 382 / 1151, 50 % 206 / 206 / 219 / 716; FSR 3 (`render/fsr-motion`,
  RCAS 0.8) 67 % 480 / 268 / 351 / 821, 50 % 311 / 204 / 231 / 571. By eye even with FSR on the car and dash, FSR smoother
  on fast-moving edges (wiper, a passing car's roof). Resolve 0.32-0.59 ms at 1080p (FSR 0.86-1.04, old 0.05-0.09),
  scene 3.2 to 1.8 ms at 50 % (RX 9060 XT radv). 100 % and `--anti-aliasing off` byte-identical to before. Untested on
  Metal.
  Road detail at speed (2026-11-05): each frame's resampling of the history blurs what moves (a half-pixel shift passes
  nothing at Nyquist, whatever the kernel), so the near road lost most of its grain while driving. Measured on 32-frame
  live sequences (75 km/h, step 12 and step 24 for twice the motion; Gabiria tarmac, 532 gravel; road-crop Laplacian
  variance, 1080p, still / moving before / after / TAA off): tarmac chase 747 / 221 / 442 / 1010, gravel chase 1071 / 345
  / 699 / 1514, tarmac cockpit 536 / 337 / 401 / 901, gravel cockpit 475 / 149 / 178 / 900; 67 % cockpit tarmac 244 /
  147 / 235, gravel 174 / 53 / 93. Fix: the new frame's share rises 0.3 by 32 pixels a frame (`fs_taa`, was 0.1 by 16) and
  the upscale's frame alone 3/4 by 32 (was 64); edges (guard rail, fence wire) still smooth, still frames unchanged,
  the 36 reference shots within 0.35/65535 RMSE, no added work. Tried: a sharper history cubic (Keys a = -1.5: wavy
  ridges on gravel; -1: +10-35 %, dropped for the simpler change); a negative mip bias was tried before (above).
  Glass over the car (the dash below the windscreen, the cabin behind the windows, lamp lenses) gathers as the rest:
  the mask takes the frame new only where the stage is behind the glass (2026-10-31). The lower windscreen (alpha about
  0.9 at its grazing angle) was taken new every frame, its dash edges and reflected cabin never smoothed; its
  frame-to-frame change halves (Fiesta Rally3 cockpit), rain drops and wipers as before. All 34 shots change: cockpits'
  lower windscreens, chase views' rear windows and tail lights smoother, the view through them slightly softer.
  Dark specks on the car's silhouette in a still chase view (3 pixels over 20 luma below their neighbours; 34 with
  off) are the thin dark sliver's real coverage, not a history lock: the raw jittered frames alternate dark and
  bright at those pixels.
- Pipelines are specialised by override constants (`pipelines.rs` `PipelineKey`: `LIT`, `INSIDE`, `CABIN_SET`,
  `FRAME_KIND`, `KNOWN`, `DRAW_*`; each carries only the code it runs).
  Dead code still held registers: 200-470 byte spills per thread in the stage and car pipelines on an M1 Pro (Metal System Trace, no Dynamic Caching). Native x 0.75 on the M1 Pro, every enhancement: road and chase
  51-57 to 15-20 ms, cockpit 188 to 20 ms; frames as before within a few edge pixels. Live at 1280x720: 107 to 156
  fps; full-screen cockpit 12 to 54 (2026-10-25). The fog, the sun's cascades and the car's shadow still spill
  (~150-310 bytes) in the stage pipelines.
- Small arrays indexed in loops (`harmonics`, `texel_filter`) are written out: Metal kept them in memory.
- Reflections medium, low and off differ from high by up to 35, 58 and 79 in the wet 45_O scene (1280x720, Xsara,
  every enhancement; 2026-10-26, RX 9060 XT, radv).
- Tried and removed (no gain on an RX 9060 XT, 2026-10-26): the car's shadow filter taps (`Quality`, `--shadows`,
  `SHADOW_TAPS`, `CONTACT_TAPS`; medium saved ~0.2 ms but differed from high in the cockpit with sun through the
  windows by mean 0.3-0.7, 1% of pixels over 5-10/255, so the filter stays high's 4 and 6; `--shadows X` and
  `shadows =` were read and ignored, taken out 2026-11-05), the stage's weather in the pipelines (`WEATHER_KNOWN`, `Conditions`), the
  day-only pipelines (`DAY_ONLY`, `night_probes`), the split reflection pipelines (`REFLECT_WATER`; one pipeline
  draws water and wet road, the draw's mode says which).
- Passes over the kept frame draw into it (`Renderer::over_attachment` in `renderer/frame.rs`);
  `viewer track-20_N --anti-aliasing off` failed validation before.
- The car's shadow map's depth range about each 4x4 texel cell (`ShadowRange`, `shadow_range.wgsl`,
  `renderer/shadows.rs`): the cockpit gains little, its own faces vary within the filter.
- The car's shadow layers draw only the parts the view or the outside views show (`GpuMesh::present`, 2026-10-30):
  outside, the hidden interior and the other state of a lamp or window cast no more. 580 208 chase 1080p 1.03 to
  0.71 ms; cockpits byte-identical, outside views within 0.4 % RMSE (shadow edges by a few pixels, the interior
  through the rear window).
- The sky draws after the opaque meshes at depth 0 (`PipelineKey::sky`): the frame matches the sky-first order
  (13 scenes) but for `--anti-aliasing off`, where 10 cut-out edge pixels on track-54_E differ by up to 3/255. 4K
  cockpit on Semetin at dusk in heavy rain, every enhancement: 36.7 to 31.3 ms (M3 Pro, 2026-10-20).
- Run to run (Metal, M-series, 2026-10-20): 18 of 18 runs match (without the depth prepass's `discard`, 66_E's
  spectators and twigs differed in ~1 run in 3). Still open: with 4x MSAA only,
  ground triangle edges differ by up to 35/255 (33_N at 2000, ~half the runs) while uploads, draw lists and matrices
  are bit-identical; from the fourth frame on, 24 of 24 runs match, so captures comparing frames render a few frames
  first (`--frames 3`).
- Inputs: the recording frame and the live records (`rbr_recording` documents the FRM1/PTC1/PEV1/DMG1/DET1/CLK1
  layouts); playback's `PTC1`/`PEV1` from `RBR_PARTICLE_EVENTS` (`particles/sidecar.rs`), else none.
- Presentation clocks step 7 ms per payload period; a live answer of several periods is one game frame.

## Live frame loop and input latency

`bin/viewer/window.rs`, `live/mod.rs`; measured with `RBR_LATENCY=script` (`bin/viewer/latency.rs`: a ±0.5 steering
flip every 500 ms, ms from the flip being due to each stage, summary at exit; on screen by Wayland's presentation
feedback, or on macOS the Metal drawable's `presentedTime`: wgpu keeps the drawable, so `shown/metal.rs` makes
the layer a run-time subclass of its class whose `nextDrawable` keeps it). Ours, per frame, in this order:

- acquire the surface's texture (the loop's wait: a free image, the refresh under fifo; `desired_maximum_frame_latency`
  2, `window.rs`);
- the gamepads' poll (the pause menu's navigation) and the input thread's last poll (`window/feed.rs`): the input read;
- `Live::pump`: the payloads due at 7 ms per period (at 60 fps 2-3 a frame, all with this read), then up to 6 ms
  waiting for their answers (one frame of 5-6 NGP steps: p50 1.5 ms, p95 2.2 ms with the pipes), so the frame draws
  the step of the input it read;
- the playback shows the newest answer; the crew's wheel and hands take the car's control the last step took (CTL1,
  the filtered value, not the device value);
- CPU build and encode (6-8 ms at 4K cockpit), submit, present; GPU 10 ms (4K, 4x AA, cachy's RTX 2080 Super Max-Q).

cachy, 4K 60 Hz, scripted flip, 3 runs each, p50 (p95) ms from the flip: before, input read 7.5 (16), answer
taken 25 (33-45), submitted 41 (49-62), GPU done 51 (58-71); after: 9 (16), 11 (16-32), 18 (21-49), 28 (30-58).
On screen 41 (46) with direct scan-out, 61 (61) composited; `play.sh` sets `MANGOHUD=0` (it added p95 tail). With
`MANGOHUD=1` (set for every program by the user's `environment.d`) the viewer crashed at every exit after a live drive
(SIGSEGV or `free(): invalid pointer`, 47 cores from 2026-10-31; reproduced 2/2, 0/2 without it): dropping the window's
wgpu instance unloaded the Vulkan loader, the driver and the layers before the process's exit handlers ran. The window
keeps its instance to the end (`window.rs`): 4/4 clean exits (2026-11-03).

Uploads (2026-11-02, M3 Pro, `scripts/bench.py` fullscreen, GPU-bound at ~51 ms): the large writes (the car's moving
parts, grass, drops, particles, marks, motion, dials; ~6.5 MB in ~37 writes a frame) go through `uploads.rs`
(`StagingBelt`, 4 MB chunks), submitted on their own before the sun's maps, reflection cube and mirrors read them;
`Queue::write_buffer` made a staging buffer a write and filled fresh pages. Hidden and skipped frames submit them
(`Renderer::skip_frame`; unsubmitted writes kept their memory). With the dials below, 3 runs each: render 3.94 to
3.12 ms, scene update 2.19 to 1.62, finish scene commands 1.68 to 1.22; 19.3 to 19.5 fps.

Live (`rbr-standalone --live`, the only live mode; the viewer has no pacing of its own): a frame per physics step on
its own clock (a late wake's frame takes the missed steps, cut at 0.1 s), each with the newest input; the input thread
(`window/feed.rs`) polls the devices every 1 ms (50 ms keepalive, one-shot flags ORed until a frame takes them); an
STP1 record (`rbr_recording::realtime`) after each stepped frame carries force, control and input number.
`--frames-out` logs each
frame's digest; `--frames FILE|-` replays such a log; the tools that step a drive themselves (`Live::lockstep`:
captures, `--screenshot --at`, `rbr-sound`) write one unchecked frame a step (1/720 s under NGP). cachy, 35 s
scripted: flip to first step p50 1.64 (p95 1.84) ms; 720 steps/s, one per frame (3 of 25175 two); wake p50 0.075 ms
late (p99 0.28); on screen 42 (51). Wheel: 526 level changes/s against 43/s paced (RSF about 510). Replays: scripted
17999 of 17999, autopilot 17994 of 17994 frames equal.

RSF (drive2 sessions, RSS1): NGP's input thread polls the wheel every 1.1 ms (POL1), one 1/720 s step per frame at
about 580 fps; a poll change reaches the car's control in p50 1.0 ms (p95 1.6), its step's end in 1.2 (1.7). Ours stays
a 60 Hz render frame behind that for the picture; the input and the force are RSF's in real time.

Video: `viewer --replay DRIVE.rrp --video FILE` (`bin/viewer/video.rs`; `--video-fps`, default 60 at 1920x1080;
ffmpeg `libx264 medium crf 18`, `aac 192k`). A `.webp` path writes an animated WebP (`libwebp_anim`, no sound), and
`--video-span FROM,TO` encodes only those game seconds (no sound; the second before warms TAA and the probes up).
The sound renders offline (`Player::offline`); window and video share `draw::draw_car` and `draw::draw_particles`.
Start-orbit frames vary between repeated exports. Kormoran I + Fiesta
Rally3 Evo, 1280x720: 63.9 fps render, replay 16920 of 16920 frames equal, video and audio durations match.

## Triple screens (ours, 2026-11-08)

`viewer --triple DIAGONAL,BEZEL,DISTANCE[,ANGLE]` (inches, mm, cm, degrees or `auto`); geometry `triple.rs`, renderer
`renderer/views.rs`, viewer `bin/viewer/triple.rs`.

- Geometry (`triple::Rig`): three like screens, each side hinged on the centre's outer frame edge and turned in by the
  side angle (auto: facing the eye, `tan(a/2) = (w/2 + bezel) / distance`); its image starts past its own frame. The
  cameras' field of view is the centre screen's (the eye sits at the distance that gives it), the sides follow at their
  angle and frames; `Rig::true_fov` is the seat distance's. Each side view is off axis (`coords::off_axis`);
  `Lens::views` gives left, centre, right.
- Renderer (`Renderer::render_view`, `FrameView`): a view keeps its temporal anti-aliasing, the motion's last view and
  the clouds' march, parked while another draws; the frame's first view (the centre) draws what they share: the sun's
  cascades fitted to every view's slices, the sun's maps, the reflection cubes, the mirrors (each from the view showing
  most of its glass), the grass, the motion's meshes. Like-sized views share the other targets; each view's commands
  are submitted before the next. The exposure follows the centre (later views ease over 0 s).
- Per view on the CPU: particle billboards (`Drawn::view`; `particles::facing` takes out an off-axis view's share along
  its axis); the characters' cells and the fences' frustum test take any view.
- Window: without `--triple-monitors` the window holds the three in thirds (spanned display, or windowed): the post
  pass writes each into its viewport, the window is cleared first, and the HUD, menus and loading screen lay out in
  the centre third (`Overlay::set_region`). `--triple-monitors L,C,R` (winit monitor names) opens a borderless window
  per monitor, the main one on the centre; a frame acquires all three or draws none. `--screenshot` writes the three
  side by side; `--video` stays single.
- Checked 2026-11-08 in a headless sway (`WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=3`): both modes on autopilot
  drives (Gabiria, Fiat 131), three 1280x720 windows at 120 fps; single view against main: 5 of 6 reference shots
  equal, golden-532-fiesta-chase 4 px by 1/255 (the facing). Not on real triple hardware; side windows of another
  size get the centre's frame scaled.

## Faithful and ours

| System | Faithful | Ours |
| --- | --- | --- |
| Stage | slots, render states, LOD lists, visibility cells, lanes, props, light maps | LOD 600 m, texture fallback, water surface |
| Sky | `TrackSettings.ini` values, linear fog, cloud face | section pick, sky, haze caps, tone curve |
| Car | parts, switch states, classes, draw lists | lighting, reflection cubes, sun and contact shadows |
| Car shadow decal | `RenderShadows` 0x605670 | body pose as its frame |
| Lights | switching, part predicates | headlight key, night light, no flares |
| Damage | deformation, scrape, crack projection, dirt level | crack texture, glass grime, detached motion |
| Particles | emitters, classes, handlers, rain instances | step, pools, lighting, sparks, chimney smoke |
| Glass | wiper angles and timing, bug data | the water-on-glass model |
| Skid marks | emission, rings, fade, strips | frame-time accumulator, depth bias |
| Ghost | mesh, look, pose, files | race-time origin, best ghost |
| Characters | placement, Lua, triggers, clips, crew IK | shading, CPU skinning |
| Fences | `CFenceHandler` cloth | inside test, body placement |
| Dials | RBRHUD data | all drawing |

## Stage

- Geometry `(1,0)` slots 0, 5-8, its light map, mip bias and culling as stored (`stage/mesh.rs` `DrawState`).
- LOD (chunk 0x6c): list 1 within 600 m, list 3 beyond 550 m, list 2 always (`stage/mesh.rs` `LOD_DISTANCE`;
  reference installs run low's 200 m).
- Visibility cells `(2,10)` (`visibility.rs` `CELL_REACH`/`DRAW_REACH`/`CUBE_REACH`): ours draws to 3 km, so from
  a crest the stage shows rather than the coarse backdrop (61_M from 80 m up: 27k to 70k triangles).
- The backdrop's pieces have no bounds (no reach, no levels); each view culls them by their vertices' box
  (`GpuMesh::backdrop_bounds`, 2026-10-31): 580 chase 2.04M to 1.85M triangles, surroundings cube face 0.51 to
  0.35 ms, frames byte-identical.
- Load (`Renderer::with_progress`): the meshes' drawn vertices (`sun::with_normals`, `foliage::with_plants`, ...) are
  made over the load's threads 1024 meshes at a time, the uploads in order (2026-10-31): 580 renderer creation to the
  sky occlusion bake 5.0 to 4.1 s, byte-identical. The cockpit eye's fit (`views::cockpit_back`, twice a load) casts
  its lines only at the interior triangles whose boxes come within reach of the eye: the 208's car load 1.9 to 1.0 s,
  the 42 community cars' mounts identical. The glass's film samples (`film_corners`, `Tints::film_tints`) go over the
  threads too: `glass::Drawn::load` 0.56 to 0.25 s. The road and ground marks (`Stage::mark_surfaces`) share one load
  of the collision ground and one lookup per ground triangle: 580's stage load 2.99 to 2.14 s. The road's edges
  (`puddles::mark_roads`) map only the film triangles' edges, the others' looked up and the edge distances found over
  the threads: 2.14 to 1.66 s. The grass's ground texels (`grass::add`, decoded and blurred per ground texture) are
  made over the threads before the ground triangles number them: 1.14 to 0.39 s. The bounce bake's surface maps leave
  out the grass's unfilled tile meshes, as the shadow casters do: 580's bounce 1.6 to 0.8 s.
- Garbage geometry (2026-11-04): what lies over 50 km from the origin (`coords::STAGE_LIMIT`, `on_stage`) is left
  out of the world shadow layer's box, the height fog's ground and the sky occlusion's tiles (boxes cut to it).
  SS Ai-Petri and Uchan-Su (325-328) hold 1224 triangles and block boxes about `f32::MAX`: the world layer spread over
  them and lit the stage as at night under a white haze, and the tile grid wrapped and panicked; Inland Road II's
  stray vertex 19 000 km off asked a 342 MB tile buffer. The grass's light readback (`Renderer::grass_light`) reads
  in parts of the device's largest buffer (Aguimes 562: 381 MB over 256).
- Missing textures (2026-11-04, user decision, were magenta): a draw's first texture that did not load draws plain
  (white: the vertex colour), a cut-out or blended draw's not at all, a second texture adds nothing
  (`renderer/textures.rs` `bind_key`). Savopohja's road, Ponteareas' tent, El Fargue's far cards.
- BTB stages (RallySimFans track folders, `stage/btb.rs`, `rbr_btb::scene`, `Lod::Band`): [btb.md](btb.md#drawing).
- Object lanes `(1,1)` (`stage::OBJECT_ALPHA_REF`, kind-byte address modes).
- Interactive props `(1,6)` (`stage/mod.rs` `InteractivePose`): every retail record drawn at its placement while its
  cell shows; live moves a woken prop by its solid's pose. The car's glass (no depth write) goes over them.
  Scenery physics: [physics.md](physics.md).
- Sky faces `(1,3)` and `Clouds.lbs` (`stage/mod.rs` `global_clouds`): the face the section's `CloudName` names,
  camera-anchored, before the stage. Water `(1,8)` is our surface (`water.wgsl`); reflection items `(1,7)`
  withheld; the sky dome (`skyboxes.lbs`) is not drawn.
- Textures: weather/wear fallback as requested (`stage/mod.rs` `Weather::fallbacks`; `tests/stage_data.rs`).
- Texture size: from the first mip that fits the adapter's 2D limit (`renderer/textures.rs` `dds_texture`; stage
  484's backdrop is 16384 x 8192, RX 9060 XT allows 32768; checked capped at 8192, 2026-10-27).
- Ours: ground detail noise, the backdrop's own texture as its detail (`backdrop_detail` in `shader.wgsl`), road
  shine on specular slots 6 and 8 (the game's formula, our `c90`); the wet film is the `wet` addition.
- Shader hashes (`hash`, `landing_hash` in `shader.wgsl`, `soft_hash` in particles) are Hoskins's "Hash without Sine"
  (2026-11-01). The sine hash's large arguments came out differently on Metal and RADV, so the ground detail's
  patches (up to 22 %) and the exposure after them differed: the same shot at the same commit was mean 13-14 of 255
  apart between the Mac and the desktop, now 2.2-3.2 (Gabiria fly-by, Komarov chase; the rest foliage edges and the
  CPU particles' libm).
- Terrain detail (ours, always on; `terrain_detail.wgsl`, `terrain_detail.rs`, 2026-10-28): the draw's most
  detailed texture window tiled mirrored and triplanar adds its high pass on the near ground (Luceram's 4096
  `z*` atlases, past the upscaler's cap). Load 63-72 ms (52M, 417O); scene pass 1080p 417O rain fly-by 2.74 to 2.84
  ms (golden fly-by, 580 chase and 1440p similar; `RERBR_NO_TERRAIN_DETAIL` A/B). The windows are found over the
  load's threads (2026-10-31): 580's 0.51 s to about 0.05.
- Steep banks (ours; `terrain_bank`, `bank_low` in `terrain_detail.wgsl`, 2026-10-31): past about 50 degrees
  (`terrain_steep`) the stretched base colour is the textures' mean widened along the footprint's major axis to 256
  texels (`TERRAIN_BANK_WIDE`; the fall-line bands average out, the along-slope patches stay), structured by the
  window's triplanar luma high pass, the paved road kept out (`paved_share`). The footprint there is minified 2 to
  4 times yet streaked, so no magnification gate, and the ground mark covers only half of 135's bank, so no ground
  gate (both tried). Weighted per texture by its bank share (`terrain_detail::bank_shares`, `mode.bank`, 2026-11-01):
  of its steep near-ground faces, how few stand within about 12 degrees of vertical. Ground atlases under 1 %
  (135, Luceram, Sturec), Sardian's facades and Sturec's walls 99 to 100 %: ungated they turned windows plaid and
  stone walls to mush. A uv test (projected from above or not) failed: the banks' uvs follow their faces, the
  streaks are painted in. 135's cockpit bank loses its smears for mottled grass (Clio, 19:30, frame 4900; the gate
  keeps 92 to 100 % of it); scene pass 1080p 3.66 to 3.75 ms there; Sardian byte-identical, banks elsewhere calmer.

## Atmosphere and sky

`atmosphere.rs` picks one `Maps/TrackSettings.ini` section by stage and weather preset (`viewer --sky` overrides;
every retail row of an installed stage but `69S`'s clear ones names one). `viewer --fog off|light|heavy` puts our
height fog over any section; `--clouds` and `--precipitation` put any sky over any row. Haze is the linear fog's
range, at least 600 m without `UseFog` (retail 69's light fog, stages 71 and 45's rows at 25..215 m hid the stage in a
white wall; other frames byte-identical, 2026-10-27). The sky gradient, doubled clouds and tone curve are ours.

- Backdrop `(1,3)` (the superbowl): every item drawn whole with the stage's matrix, its scattering depth by
  `Superbowl_Scale`; without `UseFog` its haze is the stage's range over the scale (`Atmosphere::superbowl_fog`), so
  the backdrop next to the road stays clear.
- Height fog with `Enhance::lighting` (`height_fog.rs`): the mist at half `FogEnd` (`heavyfog`), three quarters
  (`lightfog`) or all of it at the low ground, thinning with height; the sky to 100 km. Fogged at its own distance,
  as the stage (an earlier 3 km floor left 122 Bruchsal's tree lines as ghosts and its flat backdrop a pale slab from
  above, 2026-10-27); without ground about the eye or at night the linear fog stays, the backdrop at the stage's
  range.
- The sun's light in the air with `Enhance::lighting` (`light_shafts.wgsl`, ours, 2026-10-27, in place of the
  screen-space shafts), by day under a sky not overcast: a quarter-size march to the depth, the cascades times the
  volumetric clouds' shadow map, jittered and blurred; negative light, so only shaded air darkens (adding
  in-scatter instead laid a pale veil over the far land). Plus a glow within 350 m, so the sun shines through the
  canopy in rays. Shows in fog and low sun through trees; in clear air faint (13:00 mean under 0.4/255). 0.16-0.18
  ms at 2560x1440 (RX 9060 XT; 0.01-0.14 before). Additions off, overcast, night, night fog and dusk byte-identical
  to before. Tried and dropped: a CPU dust-grid splat for the wheel spray (no visible difference).
  Glow gate fix (2026-11-05): the glow's share in shade (`GLOW_SHADE`) was judged along the ray to the surface only,
  so the shaded air just before a tree's dark side switched the glow on for near trees and off for the gaps open to
  the sky: tree outlines filled with flame-shaped haze, neighbours dark (Falstone II 103O 17:20 light cloud fly-by
  frame 11000; same before the glare and GT7, so since `3c9cad09`). Now judged over the whole 350 m (8 shade-only steps
  past the surface), and past the 150 m cascades the air takes the far layer's shade (was all sunlit). Shots RMSE:
  Falstone 0.043, fog Luceram chase 0.0045, the rest under 0.0025, 20 byte-identical; light shafts 0.11 to 0.18 ms
  at 1080p. Tried and dropped: `GLOW_SHADE` 0.1 (a wash over day Luceram cockpit, RMSE 0.015).
- The backdrop's sky occlusion is weighed against its own meshes alone, not the strip it continues under (with the
  strip as cover its vertices went near black, 41M raised camera, 2026-10-27; `renderer/occlusion.rs`). The strip's
  own occlusion is unchanged; load +0.0 to 0.1 s (Semetin 0.2 to 0.3 s).
- The backdrop's sun lookups sit 80 m higher along the ray (`lifted` in `shader.wgsl`), so the stage's edge and
  trees tens of metres over the backdrop do not shade it while farther ridges still do (41M raised camera, 12:00 and
  17:30, 2026-10-27). The other way, its casters sit 200 m lower (`sun::backdrop_drop`), so the backdrop casts
  nothing on the stage ground it lies over (the Capo di Feno patch: a sea sheet 120-200 m over the ground; 200 m
  clears it, Valstagna's gorge still shades the strip). Frame and load time within noise (41M 0.94 to 0.97 ms, Capo
  2.50 to 2.53 ms at 1440p). Past a map's far plane those casters are clipped, not held on it (`vs_sun`, 2026-11-01):
  a held vertex bent its triangle toward the sun, so under a low sun the near cascades shaded the road the world layer
  lit, with a lit band from the 14 m split that moved with the car (Torre Vecchia 337M 05:30 cockpit replay). 31 of
  the 34 reference shots byte-identical, 3 by 1 to 10 pixels at 1/255.
- Panorama backdrops: on 60 of 356 installed community stage folders the backdrop is at least half upright faces, on some a
  ring of photo strips with alpha sky (462-467 Odry: 4 items, 22 triangles, 2.5-2.8 km out, white vertex colour,
  `HOR_2`/`Hor_11` 4096x512; also 138-140, 149, 166-169, 436). Where the eye is above a strip's top the sky dome
  shows below the horizon: with `Enhance::lighting` it takes the horizon's light there (`SKY_HORIZON` in
  `shader.wgsl`), not the tables' grey ground (also seen from crests on retail 41M, 45O, 61M) (2026-10-27).

## Time of day and night

`--clock HH:MM` (`relight.rs`; each variant letter is its own bake, so the bake's sun is fitted from the vertex
light). A solar model per rally moves the sun. Below -7 degrees: night (`night.wgsl`, `lamps.wgsl`): moonlight, the
lamps as main or dipped beams with a shadow map, stars, wet-road lamp glints. Shared bakes with M names (73, 83-89)
probably fit the wrong hour.

- Variants are one stage under another light: the same geometry (99-100% of vertex positions) and textures but the
  sky, the vertex light, the shadow light maps (`shadow_0.dat`) and the section (2026-10-23, every installed variant).
- Every stage is relit (`--no-relight` keeps the bake): at `--clock`, else under the bake's own sun at its hour
  (where the rally's path never climbs as high, as on the British stages, the path is lifted to it), and the viewer
  gives the start board that hour (`--start-time`, not on a championship stage).
- The start board (ours; `start_board.rs`): red during the countdown, green for 5 s from the go (none on a jump
  start's go), unlit lamps darkened to a quarter. Lit lamps glow at any hour, the digits by night and dusk, still
  fogged (2026-10-27, track-54_E). Since 2026-11-05 the lit lamps shine at 2500 cd/m2 (an LED display's) in their
  signal's colour (`start_board::LAMPS`: red, a 505 nm green), not their texture's pale red: 45N countdown, the red
  lamp by day saturated red (was salmon), at night a red glare about a bright core (was a flat cream square). A
  signal's 12000 cd/m2 drew it pale by day (255, 205, 164; GT7's chroma fade past its peak).
- Baked shading lives in three places. The shadow light maps paint the bake's cast shadows: with `Enhance::lighting`
  they are neither loaded nor used (Semetin 176_M: 165 maps, 350 MB); the cascades, the far layer and the world layer
  (`sun::world`) cast for any sun, and at night for the moon (no measurable frame cost, 2026-10-25). The backdrop
  takes no sky occlusion (baking it left it near 0 for +0.1-0.3 s load). The vertex light less the bake's sun is the
  ambient occlusion, or the computed one with `Enhance::lighting` (the bake then gives only the hue, so the crowds'
  saturated clothes are not lifted past their texture). No stage paints shadows into its track textures (every
  installed variant, 2026-10-23).
- The stage's own sun is the fitted bake sun where the fit holds (r from 0.35, `relight::own_sun`): 40 variants'
  `SunDir` lie over 20 degrees off it; weak fits keep `SunDir`.
- The eye's white balance (`day_light` in `skylight.wgsl`, with `lighting`): CIECAM02's degree of adaptation `D`
  toward the open ground's light, less toward a warm one (user's call, 2026-11-01): `D` falls to 0.4 of itself as that
  light's red over blue grows to 1.35 times noon's. At full `D` a low sun's warmth was balanced away (Torre Vecchia
  337M 05:30, sun 13 degrees up, its light 2.13 times noon's red over blue: the lit road neutral, the shade 0.45 in
  linear light; frames 0.78-0.89), now 0.84-0.95. Noon, overcast, twilight and night keep the full `D`: 23 of the 34
  reference shots byte-identical, the golden ones 0.3-1.4 % RMSE (Komarov's lit snow warm), fog 0.01-0.13 %.

- Night albedo (`night_albedo` in `night.wgsl`): textures times vertex luminance over the bake's open-ground light,
  clamped 0.3-1 at 0.25 (0.14 on 45_O to 0.39 on 10_O).
- Headlight beam (`beam` in `lamps.wgsl`, ours after the ECE patterns): dipped, cut-off 0.7 degrees down, reach about
  60 m; main (the default, as rally cars run stages), dipped plus an axis lobe of 5 times the hot spot, 1/e at 4
  degrees across and 2 up, reach about 130 m.
- The lamps' light in the air (`light_shafts.wgsl` `fs_beams`): quarter-size ray march at night and dusk (Kulla and
  Fajardo 2012): 1.36 ms at 1080p. The main pass keeps its depth whenever the post pass runs (else the beams marched
  120 m of fog over the cabin).
- The night's light is a moonlit night as the adapted eye sees it: clear, the frame's mean about 11 of 255 from
  40 m up on 44E, falling with cloud cover squared to 0.4 under heavy cloud (2026-10-23: 44E, 45O, 21O, 41M)
  (`night.wgsl`).
- Skid marks and the car's shadow decal are unlit records, so they take the moon and the lamps (`mark_light`).
- With the physical light the moon is the real one for the rally's date in 2004 (`relight::lunar`, decided
  2026-10-24): light by phase (half moon 0.09 of full), none below the horizon.
  2004's nights: arctic 0.35 of full, Mont Blanc 0.07, USA 0.78, British, Australia and Japan moonless. The disc shows
  its phase; the game-bright path keeps its full moon opposite the sun.
- With the physical light twilight and night are the day's path: the sky's tables run at every sun elevation, the moon
  (0.27 lux full) is the direct light from 1 degree down with 0.002 lux of airglow, and night from -18 only turns
  on the night-only effects. The lamps light in lux (dipped hot spot 20 000 cd), only in pipelines built with them
  (45O, 2026-10-23: -6.4 degrees 5.0 lux, -9.9 0.07, -13.3 0.006; frame mean 92 at 17:30 to 14 by 18:25, holding to
  23:00). Rod vision (`rod_vision`): moonlit ground turns blue with dim greens and reds kept, the headlit road stays
  neutral, by day at most 1/255; the meter bins luminance from -14 to 18 EV (in pre-exposed units twilight went
  black).
- With the day's light the lit draws outside the car shade as the stage (`day_lit` in `shader.wgsl`): linear albedo,
  Lambert under the day's sky, sun and ground maps, plus the sun's highlight; paint and glass reflect by Schlick's
  F0 0.04. The game's Ka, Kd, Fresnel limits and `LIGHT_SCALE` are off that path (they put the car at a fifth of the
  stage's light). The windows now show the interior. Frame time unchanged (2026-10-24: 176M, 41M, 45O chase).
- On that path paint and glass are a clear coat (`shader.wgsl` `COAT_ROUGHNESS` etc.), mirroring the 128-texel
  surroundings cube, still drawn beside the probes when the car is seen from outside (one face every 3 frames, 0.10
  ms). Fiesta R5 and GR Yaris on 103O at noon: 5 to 16% of pixels over 8 of 255; cockpit and additions off byte for
  byte (2026-10-26). Tried and dropped: bonnet SSR over the cube (little change); per-vertex car occlusion (needs
  the car in the prepass).
- Car look (2026-10-28, `car_look` in `shader.wgsl`; day's light, not in a cockpit's mirrors): the body's ambient
  and bounce light times an analytic occlusion in model space (sills, arches, underbody); a second sun lobe, pigment
  saturation 1.25; rubber by texel luminance, the rim a rough mirror; contact shadow 0.4 to 0.7 (0.4 in the cockpit
  view). gab-lit scene pass 3.01 to 3.06 ms; cockpits and additions off byte for byte.
  Round 2: the car's opaque reflecting draws are in the main view's depth prepass and shaded by the outside pipeline
  (`DrawPass::CarMain` in `renderer/pipelines.rs`), so the stage's contact shading sees the car; `Equal` depth
  there turned
  the Yaris's LED strips black. The coat mirrors half of a procedural far field (`horizon_mirrored`), so panels show
  a band over a dark one.
- Car look round 4 (`lens` in `shader.wgsl`): a lamp's lens is a reflecting lit draw with no dirt map (the M3's
  headlights are class-1 textured discs, no glass mesh): a sharp Fresnel mirror with the bright texels as chrome;
  the Yaris's LED strips are not affected. Tyre dirt falls off above 0.08..0.3 m over the ground, rubber only.
- Rain on the paint (`wet_paint`/`wet_base` in `shader.wgsl`; only with rain on wet roads, so damp, dry and
  additions off are byte for byte (dry overcast 3 pixels of 2M one level off, the codegen)): clear-coat roughness
  halved, base 15% darker and 40% more saturated (negative channels clamped: they made yellow decals lavender
  through `pow`); beads and streaks tilt the reflection's normal in the car's model space, faded out over 6 to 18
  m; the body takes grime low down (it has no ambient occlusion). Scene pass on the Luceram fly-by (1080p, ultra)
  1.73 to 1.74 ms (2026-10-28); the cockpit view's exterior paint past the A-pillar changes too, the cabin does
  not.
- Overcast is the section's cloud word, not `UseFog` alone (which also marks fog under a clear or broken sky: those
  now have their sun). Under it the tables hold the CIE overcast sky, giving up-facing ground a quarter of the clear
  sky's light (Kasten and Czeplak 1980); 45O wet mean 92 to 95; dusk to night smooth (2026-10-24).
- With the day's light the roads with a specular map reflect as a rough dielectric (`with_specular` in `shader.wgsl`,
  retail only): the game's `road_shine` stays beside it (the gravel's stone detail). Frame
  time ~0 (2026-10-24). On all ground it cost 0.5 ms for a few percent; on leaves the crown normals turned the canopy
  white.
- The clock's ambient share and tint scale the stage's display colours, so its light is them to the 2.2; day frames
  byte-identical (2026-10-20).
- The headlights' shadow map has its own layer of the sun's map (`sun::LAMP`), so the lamps cast at dusk too, beside the
  sun's cascades.

## Car

`car/` reads `Cars/Cars.ini`, the `.sgc`, `<car>.ini`, `<car>_shaders.ini` and textures.

- State per `SShaderMaterial::SetupRenderState`, order wheels, opaque, transparent (`car/shading.rs`): an opacity
  channel means blend, test `> 0`, no depth write; `CarPlastic` writes depth, two-sided; `CarIntGlass` two-sided,
  and opaque without one (the 911 GT3 RS (2010) RGT's inner door panels, the only installed pack with that, 2026-11-01).
- Not drawn (`car/`, `CHH_Car::Render`): `ghost_*`, `texture_*`, `Plane_*`, switched parts not in their state,
  gravel/snow/spare wheels, `k_` and `d_` parts; the interior `i_*` only in the cockpit, the bonnet view draws the
  exterior (user). `--hide-wheel` drops the interior parts the steering turns (one mesh on the 911 SC Grp4 and the
  i20 WRC 2020, dash and gauges kept).
- The steering wheel is the interior meshes whose node is `i_steeringwheel` (`MeshDamage::steering`), not the meshes
  centred near the node: rim and column in one mesh have their centre off the hub (Delta HF 4WD GrpA 0.17 m; the old
  0.12 m match missed it, the Escort Cosworth, Focus 2006, Impreza 95 and Polo R5 too; `tests/steering_data.rs`, 42
  installed community cars, all with the node). Delta frames: wheel static before, turning after; Escort, Clio R3 and M3
  cockpit shots byte-identical.
- Lighting per class from `<car>_shader_settings_clear.ini` with the stage's light (`car/shading.rs`); Schlick
  `F0 = 0.125` (`c91`). `c41`, `c67`, `c68`, `c90` unproven.
- Reflection: a 128-texel surroundings cube, as the game's; the load-time probes (`renderer/probes.rs`) for the
  car's ambient and sun occlusion, wet roads and water.
- Shadow decal (faithful, `car_shadow.rs`): `car_dpshad2.tga` on the `.col` triangles under the car, alpha by
  `Car_Deep_Shadow_Alpha`, pitch and height; none on an RSF-encrypted `.col`. `--enhance contact` replaces it.
- Playback: body and tarmac tyres from the recorded poses; steering is not recorded.
- Tyre place (2026-10-28, `rbr_recording::hub`, `WheelPose::centre`): a tyre goes at `anchor + HubOffset * axle`,
  the live process sending the offsets (HUB1; recordings carry none and keep the anchor). The 208 Rally4's and Clio
  Rally3's (-0.045 tarmac, -0.04 loose) sit 4.5 cm further in: centre 0.750 m against the model's 0.744 m, no longer
  10 cm outboard. Particles still take the anchor.
- Wheel rest places (2026-11-11, `car/wheels.rs`, `tests/wheel_data.rs`): 10 of 108 installed models stack their
  tarmac wheels at one node (RSF's Fiesta Rally2 and WRC 2019, DS3 WRC, i20 R5, Rally2, WRC 2017/2020/2021, Polo WRC
  2016, Lada VFTS), turned for one side (i20s right, the others left); the Porsche 911 GT3 RS swaps its front and
  back names. The loader moves each to the first class whose four nodes are apart, matched to `Point_e_wheel_<side>`
  by nearness (the i20 WRC 2020's gravel `lb`/`rf` are swapped), and turns it by pi when its normals face in there.
  Before: the car at rest (launcher preview) showed one wheel and three bare brake discs, and in a drive the stacked
  models' wheels of one side showed their backs. Wheels left where they are keep their node's turn.
- Brake discs (`e_brakedisc_<side>`, `_lit` too; under `e_wheelgrunk_<side>` in every model) move with their wheel,
  without the roll; drawn with the body they stood out of a rim the physics placed further in (i20 WRC 2020 front).
  The suspension links under the same node (`e_wheelhang/prop/stag`) stay with the body (the game's linkage is not
  ported). The exterior wheels are still not steered (the wheel's `axle` would give it).
- Not drawn: `pc_e_premove` (2026-10-28, `car/mod.rs` 0x1001e090): the classifier returns kind 10, which
  `FUN_1001e430` adds to no draw list (`cmp eax, 9; ja` at 0x1001e59d, `cmp eax, 8; ja` at 0x1001e61e).
  Community models give it a `Default Material` FaceBlock (40 of 583 installed `.ini`s; a 2.3 m white cube about the
  cabin), which showed as a pale panel behind the rear window in chase views (GC8 555, Clio Rally3, Fiat 131
  Grp4; Fiesta Rally2, M3 E30, Escort Mk II byte for byte).
- Stage and cloud textures get their mip chain completed to 1x1 at load (ours, 2026-11-04, the same
  `upscale::with_block_mips`, stored levels kept): retail stage textures hold 4 levels (a 512x512 one stops at
  64x64), so far ground shimmered without the upscaler, whose textures already have a full chain.
- Block-compressed car textures with no mip levels get a chain at load (ours, 2026-10-31, `upscale::with_block_mips`:
  top level kept, the rest box-filtered and re-encoded in the file's format). The Fiat 131 Abarth Grp4's 4096x4096
  livery, its 2048x2048 dynamic and its glass texture are DXT3 with one level and aliased in the chase view; its paint,
  dark green (0, 56, 8), reads grey under the overcast sky by the shared clear coat, its rear window's top strip is
  opaque green (0, 132, 51, alpha 255) and the beige dash panel is in its texture: the car's own art. Escort, GC8
  golden chase and the Escort cockpit byte-identical, Clio 0.001 % RMSE.
- Glass seen from outside with the day's light (ours, 2026-10-28; `GLASS_*` in `shader.wgsl`): the reflection takes
  its own weight, the pane absorbs at least half the view, so the crew dims and the sky and trees mirror (0.8 and
  0.6 read as milky fog). Only the window parts; the lamps' glass and the in-car view keep the old look. Dry day
  shots change on every car (glass only); scene pass unchanged (2.93 ms, rain fly-by). Tried and dropped: 1.7 px
  enlarged drops (aliasing speckle); lighting the cabin from outside (no visible change).
  The water film's cells (48x24, `wet_layer` in `glass.wgsl`) add a soft layer, wiper-swept areas clearer; outside
  views only, cockpit byte-identical.
- `CarIntGlass` in the in-car view with the day's light (ours, 2026-10-26; `shader.wgsl`): translucent texels darken
  what is seen through them, near-opaque ones (sun strips, visors) stay lit. A light texel (stage 52's `i_window_f`,
  grey 134 at alpha 85) greyed the view in overcast and wet. 12 community cars checked; additions off byte-identical.
  Coloured texels stay lit too (chroma 0.15 to 0.3, `INT_GLASS_PRINTED_CHROMA`, 2026-10-30): the M3 E30's yellow
  banner is drawn at alpha 0.47 and went dark against the sky; its clear glass is grey at 0.22.

## Lights

`lights.rs`. `CCarLightSystem` (`CCar+0x838`) emits `CSigCarLight` per lamp; `CHH_World::OnCarLight` 0x10068e30
swaps the `Predicate = on/off` parts and restarts the flare fade.

| Kind | Lamps | Switch box | On when |
| --- | --- | --- | --- |
| 1 | headlights | `light_lf/rf` | always (setter never called) |
| 3 | tail | `light_lb/rb` | always |
| 4 | brake | `brake_lb/rb/m` | brake to the drive `> 0.1` (`Set1e40` 0x4f1e40, pedal swap included) |
| 5 | reverse | `reverse_lb/rb` | gear index 0 (`Set1ed0` 0x4f1ed0) |

- Lit = kind switch, master word `+0xdc` and the lamp's works bit; the `lightglass_*` part and flare go with the glass
  bit. Live and sidecar: PEV1 kind 6. Otherwise brake from `CCar+0x4ec` past 0.1, no reverse or damage: the drive's
  gear index (`dri+0x54c`) and the lamp block live on the heap outside the recorded car block, and the payload's
  word 6 gear is the input's select action only (absent on auto-gear drives), so neither is in a recording; the lamp
  damage wants its own replay's kind-6 events (`particles::sidecar`, made with `rbr-standalone --events-out`).
- Flares (`InitLights` 0x10016fd0, `RenderLights` 0x100158b0; `lights.rs`): `<car>_settings.ini` `LightN`, key
  `kind * 4 + i`; additive quad 0.2 m toward the eye, alpha `facing * fade * 114.75`. Only the first record of a key
  with a `Switch = true` part switches. Ours: the headlight key (L) steps main beam, dipped, off (with the tail
  lights). Ours: no flare is drawn (user, 2026-10-31: the Clio Rally3's sat over and outboard of its lamps).
- Ours: the lamps show as their lit parts at their real luminance (2026-11-05; `Switch::glow`, `lamp_shown` in
  `shader.wgsl`): the kind's intensity (`CANDELA`: dipped hot spot 20000 cd, tail 8, brake 100, reverse 150) over the
  part's area along the car's axis (held within 0.002-0.1 m2), toward the eye by the nearest lit lamp's beam or signal
  pattern over the cosine (at least 0.25), at least 3% of the axis's, held under 30000 renderer units (half floats).
  It needs the glare (`hdr` below): the old 6% bloom spread it into a red fog. Was `LAMP_GLOW` 1.6 times the
  texture, a tail lamp's 0.3 of it by day (2026-10-31). The game's rear sprites read as blobs by day. The luminance
  goes into the draws' graded colour (`shining`, `ungraded`); its first day it went in linear, so the frame drew it
  to the power 2.2 (a tail lamp's 90 units at night at 20000, a capped headlight's past the half floats): 45O night
  chase, the tail lamps' red glare now faint about their burnt cores, by day no change (their glow under the
  texture).
- Ours: a lamp's lit part the game never draws (on the `light` or `brake` box, or `Predicate = NotUsed`), named
  `e_light_lf_lit`, `e_light_lb_lit`, `e_light_brake_lb_lit` and so on, follows its lamp by name, with its unlit pair
  `e_light_lb` when that one's box does not switch it (`lights::Switch::of_part`, 2026-10-31; user: "whatever is
  actually realistic"). Headlamps of 42 installed cars now glow from the front, by day too (M3 E30's dipped outer
  lamps, Fiesta Rally2's units; day Gabiria M3 and golden Luceram Fiesta fly-bys 1.9 and 1.6 % RMSE, night front
  views a white glare). Tail and brake ones glow in the records'
  red (`LAMP_RED`), not their texture's: the GC8's brake parts are white reflectors, the M3 E30's and 911 SC's lit
  textures light the indicator. 43 installed cars had dark tail lamps (night Sardian M3 chase); the GC8 brakes red
  (rain Sipirkakim chase 0.6 % RMSE), other reference shots byte-identical.
- Ours: a `_lit` part the game always draws (shown from the start, on no lamp's box: installed `Switch = false`, or a
  lights-on option's, `load_lit`) stays drawn and glows only while a lit lamp of its kind is within `LAMP_NEAR`
  (`Switch::of_drawn`, `steady`; the kind's number in `mode.glow.w` and `LampUniform.up.w`; 2026-11-05). Its lamp is
  its name's, else the `Point_light_*` nearest its centre within 0.3 m (pods, bars, bonnet lamps); names first, as a
  tail part's centre can lie nearest the reversing or brake point (Fiesta Rally2's and GC8's `e_light_lb_lit`). Of a
  part with lamp and body classes only the `CarLight`, `CarGlass` and `CarPlastic` faces glow (the quattro pod's
  `CarPaint` housing). This fixed the M3 E30's headlamps, dark at night since `load_lit`'s Lights_On option drew its
  lit parts always. Now glow: GC8 headlamps and tails (night Sipirkakim chase 3.6 % RMSE, fly-by 10.8 %, rain by day
  0.13 %; retail cars under 0.04 %; the GC8's tail part also covers the amber indicator band, which glows red), the M3
  E30's headlamps, the Fiesta Rally2's and M3's bars and bumper lamps. Still dark: the Fiesta Rally2's tails (the lit
  strip lies inside the opaque, always-drawn housing `e_light_lb`; the game shows that lamp by its flare), the quattro
  A2's pods (left out while its options tied; now picked, below, not yet seen), cars whose model has no
  `_lit` tail mesh (Focus WRC 2006, Delta HF 4WD, Lancer Evo II, Fiesta WRC 2014); MAZDA 323 not seen (not downloaded).
  The quattro's dark headlamps in Pengonpohja night shots past frame ~8000 are crash damage, not a regression (works
  bits 0xF0F to 0xC0C between frames 4000 and 8000).
- Ours: each kind's light at a luminous intensity (headlight hot spot 20000 cd, tail 8, brake 100, reverse 150); a lamp
  in its state since the drive's first frame is settled, not fading (2026-10-25). The rear lamps shine in a flat band
  after ECE R7.
- Ours: the headlights' light back off the road (`lamp_bounce` in `lamps.wgsl`): two Lambertian patches facing up, 15
  and 55 m ahead; scene +0.15 ms at night (45O chase and cockpit).
- Tried for the beam's white look (2026-10-25) and dropped: pulling the meter toward the 97th percentile, and local
  exposure from 1.5 stops (halos, day highlights down). The road under the beam at 1 to 5 lux against ~50x dimmer
  moonlight is physical.
- Ours: a stage the lamps light (sun below +6 degrees, `relight::LAMPS_FROM`, dusk included) loads the car with
  its car-parts light options on (`car::load_lit`, 2026-11-05, user: automatic, no picker UI): the headlights'
  on-option, then each extra-lights/pod on-option merged over the model's `.ini`/`_settings.ini` in memory
  before any mesh is built, so visibility, lamp switches and points all use the merged files
  (`rsf::night_options`, `rsf::merge_night_options`; `notes/formats.md` "RSF cars"). By day, and for the six
  installed lights-on already, the installed parts stay. Tied on variants take the first by folder name (2026-11-05,
  user: "just take whatever"; six cars kept their installed parts: the quattros, Volvo 242, Celica ST185, TA64, 208
  Rally4), not yet seen on them. An
  extra only renumbering the headlights' pods (Fiesta Rally3 `extralights_on`) is left out.
- Ours: a car with no kind 1 record that ever lights (a community car installed with headlights off: none, or all
  `Active = false` without their switch box, as 19 installed cars incl. the Escort Mk II, 2026-10-30) gets a beam per
  `Point_light_lf`/`rf` for the night light, in the colour of its "on" option's headlight record
  (`rsf::headlights_on`), else the retail 255, 255, 233. Points the car's `<car>.ini` lacks come from the "on"
  option's (`lights::with_headlight_points`, 2026-10-30): 9 of 108 installed cars had neither records nor points and lit
  nothing at night (GR Yaris Rally2, Fabia RS Rally2, Polo GTI R5, DS3 R5, Fiesta S2000 and R2, Grande Punto S2000,
  Kalina RC2, Mini WRC). The BMW M3 E36's option is named `Lights` (taken too; not rendered, its community pack is not
  installed here). The Fiesta Rally3 Evo has no `carparts/`: its `light_on/` beside the model is taken.
- Damage breaks the lamps through the damage pairs: the light block (handler `+0x3b24`) holds each kind's group at
  `+0x1c + 0x20 * (k - 1)`, lamp `i`'s works and glass at `+8i` and `+8i + 4`, so kind 1 (front) is the differential
  pair `+0x3b40` (`DmgCC30` 0x4ecc30), kind 3 (tail) the steering pair `+0x3b80` (`DmgCD30` 0x4ecd30), kind 4 (brake) the
  suspension pair `+0x3ba0` (`DmgCCB0` 0x4eccb0), kind 5 (reverse) the hub pair `+0x3bc0` (`DmgCDB0` 0x4ecdb0), kinds 2
  and 6 the driveshaft and brake-disc pairs (`DmgBB0` 0x4ecbb0, `DmgCE30` 0x4ece30); each clears its pair and sets its
  kind flag when its table fires (rbr-recomp `RBR_E35_DamageB.cpp`, `RBR_E36_CarLight.h`, `car-lights.md`). The master
  word (`+0xdc` = handler `+0x3c00`) falls in `EngineGate` 0x4ef680 and comes back in `CElectricalDamage::Vf04`
  (the service repair); no repair restores a pair, and `Reset` (`block_reset` 0x4f19b0) sets every word to 1. The six
  functors, the gate and the reset are ported (`rbr-physics` `car_body`, bound in `bound.csv`) and the hybrid check
  compares every heap byte, so live play already darkens broken lamps (lit part, beam and light off; `lightglass_*`
  with the glass bit; no flares by decision): night Tanner Corolla fly-by and cockpit with a sidecar breaking the
  front-right lamp (the 0xd0d words the standalone sent on tanner-damage) against the whole one.
- Open: `RenderLights`' second visibility test. Only the fade loop of 0x100158b0 is reconstructed (rbr-recomp
  `HH_CarRender.cpp`, 10%); the test's vectors are unknown, and no flare is drawn anyway (user, 2026-10-31).
- Ours: the headlights' shadow map (`sun::LAMP`, `lamps.wgsl`) shades the beams beyond the stage (2026-10-27): rain,
  snow, dust and mist (`lamps_at`), each step of the beams' march and the wet-road glints (`lamp_glint`). Only with
  `Enhance::lighting` and a drawn map (else a zero matrix reads lit): day and shadows-off frames byte-identical.
  41M start chase in fog cuts the air into shafts (mean diff 0.32, max 147/255); scene/particles/shafts within noise
  at 1440p (+0.06 ms at most). The retail skid marks keep their unshadowed lamp light.
- Foliage cards in the headlights' shadow keep half their light (`LAMP_THROUGH` in `night.wgsl`, as `sun_light` halves
  the sun's shadow on cut-outs; 2026-10-30): a tuft's opaque foot shadowed the next tuft's whole, a black bar under each
  tuft along the verges (41M 23:00 chase, Gabiria night chase). Night shots 0.7 to 2.4 % RMSE, day byte-identical.
- Rear lamps are capped by a soft knee on their summed light (2026-10-27): a brake lamp's 100 cd saturated red within
  a few metres under the night's exposure. Maps 450/369 fly-by 22:00-22:15 and 466 chase 23:00 show small lamps and
  a faint wash; headlights unchanged (map 126 night chase MAE 0.0004 to 0.0005).
- Wet-road lamp streaks at their real luminance (2026-11-05, `lamp_glint` in `night.wgsl`): the film's reflectance
  times the lamp's illuminance toward the road point (`beam`/`signal` of `CANDELA`) over the lobe's solid angle (pi
  times 0.2 rad in elevation and 0.2 times the view's sine, plus the lamp's 0.1 m over its distance, across), added in
  linear light; the rippled normal breaks it up. Replaced the fixed brightnesses: the 45O wet flyby's one clipped slab is
  two rippled streaks (clipped 6.6 to 5.4 % of the frame; road crop flicker mean 6.4 to 9.1, the ripples' edges), the
  chase's red haze under the tail lamps is gone (they shine little that steeply down); day wet and dry byte-identical.

## Damage visuals

`damage.rs`, `damage_parts.rs`, `dirt.rs`, `crack_decal.rs`. Inputs: `DMG1` live, `--damage`/`--crack` by hand.

- Deformation (`damage.rs`, bit for bit with the original, `tests/damage_data.rs`): per subtype-3 panel over the 64
  zone lists; normals kept; vertex alpha is the scrape amount. Interior panels without a morph follow their twin
  (ours).
- Cracks (`damage.rs` `Cracks`): code 3 the front window, code 4 a side window or (index 4) both rear halves; slot
  limits 2, 1, 1, 1, 1, 2. The texture is ours, a generated decal at `WS_Broken.dds`'s star positions.
- Detached parts: moved rigidly by the `DET1` body pose (ours; the game's motion after `OnDetachFromParent` is
  untraced). The side doors hang on their hinge while attached: the live process sends their hinge angles
  (`DOR1`, the hinged bodies' +0x3FC), and the viewer turns each door mesh about the exterior door's node
  translation by the angle over the rest angle (`coords::door_swing`, mirrored right, `MeshDamage::hinge`); at
  the rest angle an attached door draws as before, and a detached door leaves `DOR1` for its `DET1` body (whose
  first placement is the hung one, so there is no one-step pop). Hinge pivots sit at every retail car's door
  front-outer edge (`tests/car_data.rs`); the swing matches the body's world rotation for a hung door. No
  natural door detach found yet (the door functor tables want a ~9000-magnitude door-panel impact; the best
  observed crash gave 8814 on a front table), so a hung or detached door still wants a before/after capture.
- Dirt (`dirt.rs`): each tyre payload adds `mass * 1e-5`, capped 0.9; `CarPaint` lerps by the `Dirt` map. Ours: the
  colour, tyre dirt by `TireDirt` (never made in 1.02), side and rear glass grime (at most 55 %, the wet mud keeping
  40 % of the reflection).
- Dirt by surface (ours, `dirt::CarDirt::look`): the level from the mud and snow thrown, 0.3 of the water and 0.01 of
  the rest, so dry tarmac stays near clean (0.1 still filled a car to 0.79 in 98 s of clean tarmac, 2026-10-28);
  colour from the stage's dust (dry) to the game's mud tinted by it (wet, keeping 35% of the reflection) to packed
  snow by the snow's share; dirt maps' alpha raised to its square root; none on the cabin's interior set (with
  `cabin`). Cost within noise (9.06 vs 9.05 ms); additions off byte for byte (2026-10-27).
- Wheel and mud flap spatter (ours, `dirt::spatter`): parts named `e_wheel*` and `e_mudflap*` take the dirt colour as
  a film plus spots by the level. No brake dust: the rim is in the wheel's mesh.

## Particles

`particles/` throws the rally's `PS_*.ini` (`rbr_formats::pss`); sprites from `particles.dat`. `--no-particles`.

Each thrower follows its 1.02 source (`particles/*.rs` cite them; `E82F_BlkD0::Emit` 0x5ddba0 emits `rate * frame
dt` a payload, we use 1/60 s). The payload layout is in `rbr_recording::particles` (wheel, edges, velocity, rubber,
smoke, mass, depth, dirt, splash, surfaces, blend, contact). Ours: the step, pools 4x the file's, sparks (1.02
throws none), no 3D stones (never loaded in 1.02). Open: the wave's vertex alpha and scroll.
- The emitter's "camera speed modulation factor" (+0x180) scales the car's horizontal velocity, not the camera's:
  `CamHandler` 0x5d8600 on `CSigCarGeneralState` stores k (vx, vy, 0), `GenVel` 0x5d91fb adds it to each new particle.
  Only the weather's one-particle file emitters set it (-0.6, `g_rain*`, `g_snow2/3`); we don't run them, so nothing
  to port. `weather::Fall` (their fall height, forward distance, car velocity flag) is parsed but unused.

Wheel spray (`particles/wheel_spray/`, ours, `--enhance particles`): replaces the tyre handlers, fake smoke, splash
and wave with dust and water only (no stones, user decision 2026-10-20); dust, powder, mist, drops and splash by
surface, wetness, slip, speed and standing water. The splash payload +0x98 stayed 0 in every drive, so 1.02's own
splash never shows on a wet stage.
- Dust light and count (2026-10-27, `fs_soft` in `effects.wgsl`): by day the soft particles take the stage's light,
  not the game's ambient through their tone curve, which left dust darker than the road it came off (64_E heavy fog
  06:45: dust 61,49,35 against road 77,60,52); half the dust (60/s a wheel) looks the same and costs less (64_O fog
  fly-by 1080p particles 5.76 to 4.11 ms). Night unchanged; additions off byte for byte.
- Dust settles, mud does not mist (2026-10-27): dust no longer rises; shallow gravel and sprinkled tarmac throw
  less; wet gravel, dirt and grass throw a quarter of a hard road's mist (e.g. 450's `Dirt_Hard_Wet` corner cut
  beside dry tarmac threw a wet road's plume).
- Snow powder (2026-10-27): was alive but unseen (white on snow); now 70/s a wheel, 0.8 to 7 m, 3.5 s (45/s too
  thin). Particles pass +0.9 ms at 1080p (208 Sturec chase; Escort similar).
- Wet spray (2026-10-28): fully wet ground has pools of its own (the old `Mist` stays the damp haze, so dry
  weather's damp gravel is byte-identical). Effects pass 1080p: Arbroz chase 0.87 to 1.13 ms (Sipirkakim within
  noise).
- Wet spray sheet (2026-10-28): the road's mist was an opaque white wall; alphas and streaks thinned. 450's
  wet-surface corner in dry weather changes too.
- Mud plume (2026-10-28): `MudMist` lumpy and low, born at the contact patch. Effects pass Arbroz 1.09, gc8
  0.62 ms.
- Wet spray round 2 (2026-10-28; test frames stage 21 chase/fly-by `--at 5600`/`5700`): `MudDrops` are dark clods
  thrown back and out keeping most of the car's speed; `Splash` is a sheet with a white `SplashMist`; `WetMist`
  hangs. Dry, damp, dust and powder byte-identical. Effects pass 1080p: 41O chase 1.18 to 0.98 ms (52O within
  noise).
- Wet spray look (2026-10-28): the road's plume uses a holed sprite (`sprites::WISP`), fewer and denser, starting at
  the tyre. Effects pass 1080p: Luceram fly-by 2.28 to 1.63 ms (cockpit similar). Dry, dust, powder, snow and night
  byte-identical; damp shots change where wheels cross wet patches (spray keys on the surface's wetness, not the
  weather), by decision the same as in the rain (Gabiria damp chase: 129 of 2247 wheel frames on wet tarmac).
- Dust plume (2026-10-28, `sprites::DUST`, `fs_soft`): dry dust is its own sprite with a sunlit rim and a
  self-shadowed core, young dust dark tan thinning with age; rate 120 (was 60), damp ground an eighth of dry. New
  `Debris` pool: streaked dark stones off the tread. Rain, fog, snow shots and 580's chase RMSE 0; dry gravel shots
  change by design.
- Dust plume round 2 (2026-10-28): fewer, crisper puffs (`DUST_RATE` 120 to 40). Wet shots RMSE 0.
- Dust close up (2026-10-29; chase views read as opaque white cotton balls in shade): within 3 to 9 m of the eye a
  puff gets no outline of its own (radial falloff times the blurred sprite's lumps, peak 0.5), cut into wisps at the
  edge (golden Odry chase dust wholly in shade: luma 135-185 against the lit road's 100-120, now about 125-145).
  gab-lit RMSE 0.003, day Gabiria 0.0004; additions off byte-identical. The tan blocks in the fog Odry chase are the
  game's own stones, as on main.
- Dust sun shafts (2026-10-29, `dust_sun_reach` in `effects.wgsl`): a puff takes the sun's visibility averaged over
  its depth, so tree shadows print as soft shafts (fog 532 fly-by RMSE 0.022, fog Odry chase 0.028, Sipirkakim
  0.007, day Gabiria and 621 fly-by 0); other particles keep `sun_reach`. Effects pass in the 532 fly-by 2.6 to
  3.7 ms. Tried and dropped: peak alpha 0.9/0.55 to 0.7/0.4 (thinned gab-lit's and Odry's); `DUST_SUN` 2.2 to 1.6
  and a 0.75 far veil (dimmed gab-lit's far more than the 532 plume, so it stays a bright tan body).
- Dust shadow taps (2026-11-11, `dust_sun_reach`): 28 taps evenly along the view ray through the puff, each off it
  by a point of a 2.5-texel spiral disc, in the second cascade at the finest (2.5 to 5 cm texels; its own where
  that one does not hold the point). Before, seven places along the ray with four taps 2.5 texels apart, in the
  first cascade's 1 cm texels: a canopy's flecks printed on the dust as a grid of blocks (Odry chase, Comb I
  fly-by; particles drawn after the resolve, so the frame showed it as the shot did). Particles pass, 1080p ultra:
  Odry chase 0.33 to 0.28 ms, Comb I fly-by 1.98 to 1.77 ms. Tried and dropped: the four taps turned by a golden
  angle a place (speckle), the same in the second cascade (spots a tap's spacing apart, the shafts gone).
- No spray in the cabin (2026-11-01, `fs_soft`): in the in-car views the soft particles fade inside the cabin reach
  (2 to 2.5 m from the eye) as the rain does; the wheels' spray showed through the GC8's footwell gaps (52 rain
  cockpit `--at 4500` now matches `--no-particles` there). Rain chase byte-identical. The eye sphere is now the
  cabin mask (2026-11-05, below).
- Dust colour from the stage's road (2026-11-01, `wheel_spray::dust::of_stage`; was a colour by the `PS_*.ini`
  region name, ours, neutral grey on 29 of 52 readable community variants and all MontBlanc and Arctic ones): the
  area-weighted linear mean of the drawn ground (textures, texture 1's blend, vertex tint) within 3 m of the driveline
  on gravel, dirt or rough collision, green triangles left out (community verges marked loose under grass); luma kept within
  0.55-0.75 (stages 417's and 176's O textures carry their light, a third darker than M; wet textures half), no bluer
  than red (Gabiria's blue-grey verges); neutral under 5% loose road (tarmac, snow). Also tints the car's dirt.
  Retail measures pale cream in Australia (0.78,0.75,0.70; 1.02's own dust class 0.95,0.91,0.79, not our old red),
  grey-brown in Britain, grey in Japan, red-brown to tan in the USA. Scan over 86 retail and 52 community variants,
  checked against top-down renders of each road. Load +80 ms at most (621, 580). 34 shots: 12 change by 0.1% RMSE or
  more (dusty chases and fly-bys, day Gabiria fly-by 1.3%), snow, tarmac cockpits and night byte-identical or nearly.
- Spin dust live (2026-11-02, `particles::slip_of_mass`): the spray's slip took the payload's rubber, a slip speed in
  playback's payloads but 0 to 1 in the game's, so spinning and sliding threw no extra dust live or in replays
  (El Condor: rubber at most 0.001 while the mass reached 50 sliding). The game's payloads now take the slip from the
  mass flow (`SLIP_MASS` read backwards); the reference shots build their payloads from the poses and keep the rubber.
- Spray from the seat (2026-11-05). Diagnosis: the in-car views showed almost no own spray because none reached
  their frustum, not because a view rule erased it (cockpit frames byte-identical with the cabin fade, `DUST_CLOSE`
  and the wisp near fade off; bumper frames byte-identical with `--no-particles`, on gravel slides, hard braking,
  wet and snow). Spray leaves the wheels inside the body's box and drag takes it behind the eye within 0.3 s.
  Changes: `particles::cabin::CabinMask` (body box below the belt, eye − 0.25 m, from 0.5 m under the floor; room
  box above it from 0.8 m ahead of the eye; 0.15 m soft edge; model frame from `car.bounds()` and the cockpit eye)
  replaces the eye sphere in `fs_soft`, so spray outside the doors shows and the 52 GC8 footwell gaps stay clean.
  Front-wheel pools from the patch's sideways slide (`front::slide`, 0.8-4 m/s): `Kick` dust and `Fling` stones
  thrown out the slide's way, `Flurry` snow powder (also ploughed in snow); a plume from standing water (next
  bullet). The air about the car lags its speed by `front::WAKE_LAG` 0.8 s, so a braking car's dust, haze
  and powder roll past it (`front::carried`). The front pools draw from their own `Rng` (`Simulation::emit_drawing`)
  so the other pools stay byte-identical. Recorded drives seldom slide their fronts, so the side throws show mostly
  in live play. Additions off byte-identical; effects pass 1080p cockpit, wet and dusty, within noise (0.3-0.4 ms
  either way). Shots: snow Sturec 208 chase 0.027 and Peklo Yaris
  chase 0.016 (braking powder), dusty chases and fly-bys 0.001-0.009; 13 byte-identical. Mirrors draw the spray in
  the flat game pipeline (`particles.draw`), not the soft pass.
- Puddle plume and splats (2026-11-05; the first version's streaked `drop` sheet over the bonnet read as an old
  game's thin lines): a front tyre in standing water throws `Bow` clumps (`PUFF`, translucent muddy grey, 0.6 s,
  0.3 to 1.3 m) and `BowMist` (`WISP`, 0.9 s, 0.4 to 1.8 m), mostly out and up at the front corner, a quarter in
  over the bonnet, keeping 0.9-0.98 of the car's speed so the air takes them back past the wing. Gated on the
  standing water from 0.15 to 0.6 (`BOW_WATER`: the wet ruts' 0.08 film under `--enhance wet` throws none), and
  thrown at once on running in (`BOW_BURST` 80 by how much deeper it got: the puddles on 54O are crossed in about
  10 frames). Glass hook: `Thrown::bow`, splats a frame (6 a second at full depth and speed, 3 on running in), from
  `WheelSpray::feed` through `dirt::Spray` to `Glass::splat`: a 3-6 mm drop low on the glass with 3-7 raindrops
  round it within 4 cm, skidding and running and wiped as rain; the paint's grime ignores it. Effects pass 1080p
  cockpit at the 54O puddle (`--at 10800`, wet heavy): 0.46 to 0.70 ms; off the puddles within 0.05 ms. Captures
  `/tmp/cockpit-spray/wet2/`. Shots against the first version: rain Luceram GC8 cockpit RMSE 0.025 and fly-by
  0.023 (streaks gone, the plume at the corner), night Luceram 131 cockpit 0.014, fog Luceram GC8 chase 0.007, two
  under 0.001, 30 byte-identical; 52 rain cockpit `--at 4500` footwells clean. The wiper angle that differed
  between some builds in `--enhance all` cockpit frames is not the auto schedule (54O `--at 10803`: same frame,
  Continuous, 629 mud specks, phase 0.947 in both; adding a log line flipped it): build-dependent, not chased.
- Wet spray without streaks (2026-11-05): `Drops` and `Splash` are soft `PUFF` clumps of the water's grey (0.32 and
  0.25 alpha, growing 0.08 to 0.5 and 0.05 to 0.35 m), `MudDrops` small dark `STONE` clods without a trail; rates,
  lives, fall and drag as before, so the plumes keep their shape and every pool its draws. Effects pass 1080p within
  0.1 ms (54O wet chase 0.59 to 0.60, Luceram rain fly-by 1.51 to 1.58). Shots against the plume version: rain
  Luceram GC8 fly-by RMSE 0.010, fog Luceram chase 0.008, rain Sipirkakim chase 0.008, day Gabiria M3 fly-by 0.006
  (damp patches' clods), golden Gabiria chase 0.004, two under 0.001, 29 byte-identical. Captures
  `/tmp/cockpit-spray/wet3/`.
Soft-particle cost (2026-10-26, RX 9060 XT): the effects pass is fill-bound (Sipirkakim II `--at 15000`: 926
quads, ~400 screens of coverage, 35 ms at 1440p, nearly all `fs_soft`; GPU clocks bimodal). Fixes, frames within
2/255 of before (lamps' code out of the day's pipeline, fragments under 1/1000 alpha discarded, depth after the
erosion test). Medians of 3 interleaved runs, 1440p: dusty chase 59.7 to 16.1 ms (4K 137 to 39); wet heavy rain
cockpit 68 to 15, Falstone bumper 18.7 to 3.7. Rain streaks and mist cost under 1 ms of that.
`--frames` is steady state: particles built once, not per frame (the drawn-only frame's long 'encoding' is the CPU
waiting for the GPU).
Wet mist (2026-10-26): it had never shown (edge erosion ate the 0.28-alpha mist). Now a wisp (grey-white 0.32
alpha, 2.5 s, up to 3.4 m wide, 120/s a wheel at 28 m/s). Damp: a `Haze` pool at a third of the rate, 1.2 s, no
drops. Additions off unchanged; wet chase within noise of main.

Half-size particles (2026-11-05, `particle_target.rs`): on frames 1800 lines tall or more (4K), the rain's mist and
the soft billboards are drawn at half the width and height, over the depth reduced to its nearest of each 2x2, into
a target holding their light and the frame's transmittance (alpha `Zero, OneMinusSrcAlpha`), each laid over the
frame in its turn (mist, then the full-size streaks and waves, then the billboards, as before) by a depth-weighted
bilinear upsample (`DEPTH_TOLERANCE` 0.05); only the screen rectangle the billboards' corners cover is reduced, drawn
and laid, and the composite discards pixels no particle reached. Diagnosis (user's perf log, Loch Ard 127_N wet
chase, quattro, 4K, RX 9060 XT radv): the steady ~0.9 ms was mostly the fullscreen mist (1.1 ms at 4K, half of it
its lamps' light; the streaks 0.1 ms), the 5-9 ms spikes the puddle plume and spray puffs near the eye. 4K medians of
3 interleaved offscreen runs (`--frames 60`, steady frames), "particles, rain" mean before -> after: Loch Ard 127
light rain chase `--at 6750` 2.43 -> 1.24 ms, bench (Gabiria heavy rain 131 cockpit `--at 5821`) 2.25 -> 1.23,
dry dusty Odry 462 chase `--at 6053` 1.17 -> 0.51, Helette 580 dry chase 0.01 -> 0.03; scene unchanged. The user's
replay of that drive (`viewer --replay ... --video --perf-log`, game 0-50 s, 30 fps), mean / p99 / max: chase
1.65 / 2.96 / 3.43 -> 1.02 / 1.42 / 1.56 ms; cockpit 1.37 / 5.20 / 8.47 -> 0.93 / 1.92 / 2.84. Looks: 11 rain,
spray, dust, snow and grass shots at 4K RMSE 0 to 0.0062 (the mist at foliage edges against the sky); 8 consecutive
frames of the rain fly-by and the Odry dust chase without flicker (frame-to-frame RMSE the same as before); 1080p and
1440p unchanged (byte-identical shots). Tried and dropped: a full-size mist with only the billboards halved (0.55 ms
more on Loch Ard). In rain the mist's layer is drawn before the temporal resolve, which lays it over the frame it
writes and not over the history (`fs_taa_mist`, `particle_layer.wgsl`; 2026-11-06): its own pass over the whole frame
cost 0.45 ms at 4K. Loch Ard / bench "particles, rain" 1.23 / 1.24 -> 0.88 / 0.89 ms, taa +0.04 / +0.03 (the
layer's "nothing reached" test kept per tap: tested on the blended result instead, the billboards' composite cost
0.09 ms more); rain shots RMSE 0.0004-0.0006 (1/255 rounding noise on 7% of the pixels: the frame no longer passes
through a half float between the resolve and the mist).

## Windscreen and wipers

Cost (2026-11-06, 450 heavy rain cockpit, 4K, RX 9060 XT; by leaving kinds out): the film's cells about 1.8 ms (the
beads 0.94, their fine layer 0.5, a lens each costing nothing more; the blade's ridge 0.2, the wipe lookup 0.16; raster
and blend 0.15), the pane's cells 0.8, the drops and ribbons nothing. Shading the sheet's view only where it covers
anything: no gain. The beads' cost is their squares' lookups, not their lenses. The film and the panes' cells have
their own pipelines (`fs_film`, `fs_pane`, `gpu::PIPELINES`): RADV stats put the one shader at 60 VGPRs (24 waves), now
the film 60, the panes and the rest 48 (32 waves); no measurable time gain alone. Under `Detail::High` from 1800 rows
(4K) the film's cells seen from inside are drawn at half size (`HALF_FILM_FROM`, `Pass::draw_half`) and laid over
through their full-size quads, depth tested: 450 rain / 127 wet cockpits -0.88 / -0.61 ms, RMSE 0.026 / 0.014, beads
softer and the smallest fade, no flicker. User decision 2026-11-06: High only, Ultra keeps the film whole. Tried and
dropped: the fine beads left out at 4K (-0.43 / -0.29 ms; the glass reads much drier).

`glass/`, ours (`glass/model.rs` documents the chart, inputs and wiper schedule: 1.3823/1.3195 rad, 0.4 s each way,
`--wipers`, key I). Drops are lenses over the resolved frame; a wet film blurs it. Camera dirt on outside views is
not ported.
- The sound reads the glass after each frame (`crate::sound::glass`, 2026-11-05): the blades' phase and strokes, the
  film and drops under their sweep, and the splats (`Glass::splatted`, taken by `Drawn::take_splats`; nothing in the
  glass reads it). `notes/sound.md`, Exhaust and car effects.
- Look (ours): mud specks are opaque (the HDR sky through a 5 % gap read as grey); with `cabin`, seen from inside, a speck's thin rim lets
  3 to 45 % of the light behind through in the mud's hue, its core dark (2026-10-27; glass pass 0.14 ms before and
  after, off frames byte-identical). Tried and dropped: a dashboard reflection in the windscreen (user's call).
- Specks (ours, 2026-11-01; `splat` in `glass.wgsl`, `mud_color`, `hidden` in `glass/mod.rs`): mud specks take the
  stage's dust colour against the neutral dust, as the car's dirt does (snow specks keep theirs); radius 1 to 5 mm
  (was 2 to 7), twice as many a unit of spray, the auto wipers at 600; each a ragged blot thinning to its edge with up
  to 4 fine droplets thrown up the glass around it. Seen from inside, a speck behind a sun strip or printed band is
  hidden (what the glass lets through there, from 0.1 to 0.4; the printed bands of the Yaris, Fiesta and i20 hide them
  on damp Sipirkakim). By the stage's wetness (`glass::Mud`): the tint goes from the drying mud to the car's wet dirt
  colour, the size from 0.7 of the radius dry to 1.2 wet, and seen from inside wet mud lets 0.4 of the light through
  (`MUD_WET_THROUGH`). Dry tarmac and snow shots byte-identical.
- Water redone (ours, 2026-10-31; `lens`, `film_cell`, `ribbon` in `glass.wgsl`; `flow`, `merge` in `model.rs`): a drop, bead or
  ribbon is clear water, not a white cap: it lays 0.85 (0.55 for a ribbon's weight) of a lens over what is drawn behind it
  (replacing the pixel left drops darker than the hazy glass), the frame seen mirrored through its centre 5x its offset,
  a thin dark outline and a small highlight (the lens's up as of the wipers below). Pixel offsets
  come from the quad's screen axes (`Out::pixels`). Beads: two layers (laid out as of the 2026-11-02 bullet below), shown for
  `1 - exp(-2.5 film)`; under two pixels a bead has no outline. Film: the frame through a wavy sheet (noise slope, 2 px), 4 taps
  streaked along the airflow, patchy; no flat veil. Sim (as of the bullet on drops at rest below): drops that touch merge
  (volume kept, every 1/60 s, swept by x); drops from 2.2 mm lay a ribbon (kind 7, 2.5 s) every four radii from their own
  volume and a bead beside it (5 s), so they shrink and stop; a blade leaves 0.1 of the film and clears water within
  `BLADE_PAD` (8 mm) of the angles it swept. Wiper schedule and drizzle mapping unchanged.
  Cost at 1080p heavy rain (GC8 Luceram, `--profile --frames 120`): "particles, rain" 1.12 to 1.18 ms, drawn-only frame 7.88 to
  7.98 ms. Dry and snow cockpits and chases byte-identical. Open: heavy rain at a stop reads less speckled than before (fewer
  tiny white specks, by design); the 131 Abarth shows no windscreen water at night (its glass is not charted, as before);
  `--sequence` with `--live` now ends on `--at` (the car moves between frames; before it started there and stood still).
- The wipers on the water (ours, 2026-10-31; `last_wipe`, `bead_share`, `smear`, `ridge` in `glass.wgsl`,
  `model::Stroke`): the glass keeps the blades' last 4 strokes and each pixel finds when a blade last passed it. Behind
  a blade the film and its beads build again from nothing at the rain's rate, in a new layout each stroke; a smear of
  fine arcs dries in 0.5 s; the gathered water rides along the leading edge as a lumpy ridge, left lying where a stroke
  turns (0.1 s, then the drops the blade leaves). The glass steps by the step clock's time between frames, as the blades do (a payload period a frame
  ran it at half the blades' pace in a 6.9 ms sequence). The blades turn about the normal of the plane that best fits
  the glass over their sweep (`Windscreen::turning_axis`): about the chart's normal the GC8's blades sank 7 cm into it
  or rose 10 cm off it at the top of the sweep, 70 px off the line they wiped as seen from the seat; now within 2 cm.
  Lens: the sky's light (0.7 of `Frame::light`, at most 0.45) below an upturned line, a crescent in the lower rim, the
  outline darker on top, its up the screen's (a running drop's lens was upside down). The drops feel gravity less the
  car's acceleration (`felt_acceleration`: the velocity's change, smoothed over 0.25 s, at most 30 m/s²): braking runs
  them down the glass, a bend sideways (GC8 Luceram: 4 to 5 m/s² braking, 2 to 4 cornering). Dry M3 621 cockpit within
  0.2 of 255 on the glass. `--sequence` with `--live` showed the last frame each time (`Playback::step` is relative).
  Open: with `windscreen` the cabin's reflection, now over the water, lifted the dark view about 30 of 255, the
  heavy-rain look's main haze (now drawn at half, below); the blade meshes' frames stand 1 to 5 cm off the glass.
- Drops at rest and on the move (ours, 2026-11-01, user: "the drops move around too fast/aggressively"; `flow`, `leave`
  in `model.rs`): drops land 0.7 to 3.5 mm (cubed; 1400/m2/s still, 50 more per m/s), bigger ones only by merging. The
  pin goes as `16 (2 mm / r)^2` m/s² (the edge holds by its width, the pull moves the mass; was `8 (2 mm / r)`): on the
  GC8's glass (gravity 4.1 m/s² along it) drops run from about 4 mm, on a side window from 2.5 mm (no side factor now),
  small ones hold through the car's 3 to 6 m/s² of felt acceleration (p50, p90 on Luceram, as large as gravity along the
  glass). Grip 0.7 to 1.5 a drop, times 1 +- 0.6 over 4 mm noise patches; a running drop holds at 0.85 of its pin, so
  it stops on grippy patches until it grows. It follows its pull over 0.15 s (holds under 4 mm/s) at
  `0.006 (r / 2 mm)^2` m/s per m/s² beyond the pin, at most 0.2 m/s. The airflow's sideways share grows from the
  middle out. Where a blade turns or parks it leaves 0.05 of the water it swept, up to 24 drops of 0.6 to 2.5 mm (cubed),
  1 to 14 mm ahead of it (squared). Luceram heavy rain, 8 s in at three points: drops sliding 29-38 % to 6-10 %, their median speed 12-16 to
  2-3 cm/s (90th 60 cap to 7-12); over the 75 s drive 0-9 % slide. Frame cost within noise (drawn-only 20.4 to
  20.6 ms, glass pass 3.75 to 3.78 ms, 2000 to 2500 drops on the glass against 600 to 800). User: "looks pretty
  realistic now".
- Softer and less alike (ours, 2026-11-01, user: "the culmination effect from the wipers is a bit too harsh", "more
  natural and random", "real drops aren't that perfectly round", "the interior a bit less reflective"): the blade's
  ridge 2 to 7 mm wide (was 3 to 12), 0.6 of a lens, sheen 0.08 of the sky (0.35), left lying 0.1 s (0.25): where the
  blades turned it read as blocks of white foam. Each lens its own (`vary`): 0.3 to all of the sky share and of the
  crescent (now 0.2 from 0.3), its outline 0.75 to 1 as dark, its highlight shifted. Outlines (`outline`): ragged 0.12
  to 0.28 (was 0.04 to 0.2; beads 0.15, were round), up to 0.35 longer one way, each its own way. Beads patchy over
  4 cm (0.25 to 1.5 of their share); rain and snow in gusts, +-0.5 over about 2 s (`GUSTS`). The cabin's reflection
  in the windscreen at 0.5 (`CABIN_REFLECTED`). The pale shapes high in the GC8's windscreen at Luceram's hairpin are
  the stage (they stay with the pane and the film off). Cost within noise (GC8 Luceram heavy rain, drawn-only 20.38 to
  20.41 ms).
- Beads at random and drops on the move (ours, 2026-11-02, user: drops "on a grid", "many are just static, sticky";
  `bead_layer` in `glass.wgsl`; `flow`, `skid` in `model.rs`). Beads were one to a square of 5x5 and 2x2 a film cell,
  kept inside it (the big ones at its middle), nearly every square filled: a jittered grid, fixed until wiped. Now two
  lattices in the pane's metres (5.5 and 12 mm, turned 0.53 and -0.97 rad off the chart), a bead anywhere in its
  square with radius 0.8-2.5 and 1.5-5 mm (cubed), looked up among the 2x2 nearest squares; each lasts 3 to 9 s of
  standing heavy rain on the pane's landing clock (`Glass::landed`, faster at speed), then another lands in its square.
  A bead shows only where the pixel's last wipe is its square middle's (cut at a stroke's edge). Model: `AIR` 0.05
  (0.025 about evened gravity at rally speeds), a landing drop skids along the airflow at 0.002 of the car's speed,
  `MOBILITY` 0.004 (0.006), grip patches turned 0.6 rad, and past `MOST` the still drops go first (at speed the cap
  took drops under a second after landing, running or not). Sim (1.06 m2, 4.1 m/s2 along it, heavy rain, wipers off,
  8 to 12 s): sliding at 15, 25, 35 m/s 0, 0, 7 % to 27, 61, 100 % (skids included), median 1.5 cm/s at 25 and 4.3 at
  35 (p90 9); standing unchanged. Glass pass (in "screen reflections", GC8 Luceram heavy rain cockpit, 1080p) 3.90 to
  4.28 ms; the 36 shots: rain ones change, the rest byte-identical or within 1 of 255 on a few pane pixels.
- Glass CPU (ours, 2026-11-02, user: "the cpu latency it added"; `Drawn::draw`, `Glass::merge`, `pane_drops`). samply
  of `Drawn::draw`, GC8 Luceram heavy rain cockpit, live frames of 5 periods: one drive frame a frame drawn 2.2 to
  1.0-1.15 ms, twelve 6.0 to 2.75 ms. `MOST` and `TRAIL_MOST` are per square metre now (a side window held 3-4x the
  windscreen's density; four of them about 1800 drops each, was up to 6000); the panes' cells seen from inside made
  once, model space; the film's corners once each (were four times a cell); one instance buffer kept; the merge
  sweep over an (x, index) copy reaching the largest radius, not 2 cm (3x faster); the drive's frames stepped
  together up to 1/60 s, ending where the wipers turn and at the frame shown (a seek's 8 s warm-up in ~480 steps, was
  1142). Drop counts and the blades unchanged. Left: the upload (18k instances of 112 bytes, ~0.2 ms) and a step
  (~0.5 ms: merge, flow, wipe).
- Beads (ours, 2026-10-26): the film's cells draw the small drops too many to be drops of their own. The film builds to the rain's amount, less with the airflow, and the wipers clear
  the beads with it (drops alone covered 1.3-2.1% after 12 s; more drops cost ~5 ms a step at 100 km/h). Shows with
  additions off too (user-approved); cockpit heavy 13.1 ms before and after.
- Drizzle (ours, 2026-10-31; launcher "Drizzle" = `Precipitation::Light`, intensity 0.5): `Precipitation::shower()` is
  the intensity squared for rain (0.25 for drizzle, 1 for heavy, snow unchanged); the falling streaks' count and the
  glass's drop and film rates use it, so drizzle is a sparse faint shower with fine drops that build slowly. The wipers
  read the raw intensity: at 0.6 or below intermittent (`DRIZZLE_MOST`), heavier rain continuous. Heavy rain shots
  byte-identical.
- Side windows (ours): each side window its own glass (drops, trails, film, mud; no wipers, no bugs), drawn on its
  exterior pane; 0.3 of the windscreen's rain and 0.1 of its spray, landing low.
- The pane itself from inside (ours, 2026-10-30, with `windscreen`; kind 6, `pane` in `glass.wgsl`): a cell per film
  cell, drawn over the water (its inner face is nearer the eye; under it, drops hid the cabin's reflection): Schlick
  loss through both faces, a patchy dust film scattering the sun (forward and broad
  lobes, the stage's shade from sun cascade 0) and the sky, thinner in the wipers' sweep, specks, dried water spots, and
  arc scratches that glint where `dot(t, sun - view) = 0`; side windows half the dust. Dropped: dust in arcs along the
  blade (noise stretched along the arc read as even concentric rings). The cabin reflected in it (2026-10-30): the
  windscreen's mean plane is a mirror pass of its own (`set_windscreen_mirror`, the mirrors' last image layer, drawn as
  the centre mirror's, every frame), weighed by `F (1 + (1 - F)^2)` for the inner face and the outer seen through it; it
  shows over dark backgrounds and the tinted band, barely over the lit road. +1.7 ms at 1080p (208 cockpit: mirrors span
  1.2 to 2.9 ms), most of it the stage to 300 m behind the rear window. Tried and dropped: a screen-space march over the
  frame's depth (the dash under the glass, what the glass mostly reflects, is hidden from the eye behind the dash's
  crest). Normal: the chart's mean plane (the grid's stepped heights striped per cell). A cell is its four corners
  exactly (the low one and the others' offsets, `Drop::far`), each from its integer film corner: drawn as a centre and
  two axes the non-planar cells cracked into dark lines along every third cell edge (where the 48 x 24 film corners
  meet the 64 x 32 height grid's edges) and pinholes; a hair of overlap instead showed a bright grid (Clio Rally3
  Wythop, 2026-10-30). The film corners lie on the glass's triangles (off the glass at the nearest point's height;
  the grid's stepped heights cut the film in stairs where it meets the door trim, and the nearest triangle's plane
  threw cells off the glass), and each cell is cut at the glass's outline over a pixel by its corners' signed distance
  to it (`Windscreen::edge`, `Drop::edge` over `PANE_EDGE` 5 cm): kept or dropped by corner, the film ran up to half a
  2 cm cell past the glass in 50 px stairs (Clio Rally3 A-pillar fringe; the Escort's pillar edge). The outside's
  light on the pane (its dust, specks, spots and scratches) is weighed by what the
  window part's texture lets through, `1 - alpha` of its blended faces (`glass::Tints`, read at load from the
  interior set's window parts, `MeshDamage::window`), at 4 steps across each cell (`gpu::PANE_TINTS`, 5 x 5 samples
  of 4 bits a cell; at the corners alone the sky's light lay on the strip's lower 2 cm, a pale band over the black,
  Clio Rally3 (42, 43, 35) against the strip's (9, 17, 8), now the strip's): the Clio Rally3's printed sun strip glowed brown
  (0.37 against 0.05 with `windscreen` off) and now only shows the cabin reflected in it. Dust 0.4% (was 1%: the view
  went milky toward a low sun; dark trees +0.15 in display value, now +0.08). Against onboard footage and matched
  frames (2026-10-30; M3 E30 621 13:00 heavy cloud, Fiesta Rally2 Luceram 08:30): the glass hazes toward a low sun and
  reads clear away from it, with a few faint streaks and no visible dots or rings. Dust 0.6% with the broad lobe a
  tenth (was 0.4% and 0.3); scratches a third as strong, half as many, each a run of its arc (`SCRATCH_RUN`, about
  0.3 of it: whole rings read as a regular pattern of arcs); specks a quarter as many and at most 0.4 mm (orange flecks
  on the side windows); dried spots only as the glass has been wet (`Renderer::wetness` or the precipitation, so none on
  a dry stage), 0.8 to 2.5 mm with a soft wavering rim (drawn rings). The dust's sky light a third (`DUST_SKY`
  0.001, was 0.0032: an even milky veil away from the sun, the golden Clio's view's deviation 34 against 46 without
  it, now 40), and the cabin reflected in the outer face weighed by the texture twice (the strip keeps only the inner
  face's: the Clio Rally3's switch panel over its strip 56 to 35 of 255). Left: the display reflected at the strip's
  edge (lit texels at the Fresnel of 60 degrees, physical; a viewing cone on the backlight changed nothing, its
  display is not backlit at 18:30).
- Dials in the reflection at night (2026-10-30, `backlit` in `shader.wgsl`): the windscreen sees the dash some 75
  degrees off its faces, and the flat dials of the meshes showed whole and at their full backlight (Audi quattro A2,
  Helette 22:30: the gauge cluster mirrored over the road). In a mirror's pass a backlit texel shows what a recessed
  face does past its bezel, `1 - DIAL_RECESS tan` of the angle off its face (0.25: 7 % at 75 degrees); the main view
  is unchanged (the A2's dash byte-identical; a recess there hollowed the orange dome lamp that stands out of the
  dash). The dials are 25 cd/m2 (`DIAL_GLOW` over `NIGHT_PRE_EXPOSURE`), their 8 % reflection as bright as the lit
  road, and local exposure takes the dials themselves down: the pair read alike. Left: the dome's reflection (a lit
  lamp), the Clio Rally3's display at the top of the glass (seen 45 degrees off from above, physical). The bulkhead (`glass::bulkhead`, with
  `windscreen`): a dark strip hanging 0.6 m from the glass's foot in the interior set, drawn as its first opaque lit
  face, added after the cabin light's bake (seeing no sky) and drawn only in the in-car mirrors' passes, casting no
  shadow (`Renderer::set_reflected_only`). The sets leave gaps between the dash and the glass that the driver never
  sees past; the reflection showed the road under the car and the sky through them (Clio Rally3: a ragged white patch
  over the dash; with the stage left out of the pass it turned to sky). Drawn in the main view it hid the Escort Mk
  II's footwell (upright glass), and baked it took the cabin's sky share from 0.280 to 0.267; now the Escort is
  unchanged and the Clio changes only in the gap; a few dim lines stay at the dash's edge. Tried and dropped: sloping
  it back under the dash at 45 degrees (lit grey facets, a larger gap). Outside views byte-identical.
  Cars without `i_window_f` (the M3 E30) have no glass at all.
- The panes' cells are placed corner by corner (2026-11-13, `glass::drawn::placed_cell`): each corner is a stage point
  of its own and the others' offsets from the low one are taken there. Placed by offsets turned on their own, a
  neighbour's corner landed up to a float's step beside this one's (0.25 mm some km from the stage's origin), the cells
  crossing or parting by part of a pixel: lines of dots along the cells' edges, plain over a bright view (the door
  mirror through the side window; M3 E36 on stage 44, with and without `--mirrors`, single pixels without TAA).

## Skid marks

`skidmarks.rs` (pure, documents the emission, rings and fade), `skidmarks_pass.rs`. Records from the `PS_*.ini`
"Skidmarks" section and its aliases; `_low` files have none.

Measured (ours): the payload edges are 0.20 m apart; records scale them by 2.0-2.2 and the strips fill ~85 %, so 1.02
marks show ~37 cm, laid along the axle. The payload's velocity +0x50 is not the patch's; rubber is 0-1 in the game's
payloads, a slip speed in m/s in playback's from poses. Snow drives run mostly on `Snowwall_Bottom` (0x58).

`--enhance marks` (ours) replaces them: width 0.21 m loose/snow, 0.235 m tarmac, swept across the travel. Gravel/dirt:
rut with tread print, light berms; tarmac only when sliding; snow groove; grass flattened; wet darker, a sheen at
grazing views. Sections every 1 m, 800 m a wheel, the last 250 m fading; marks fade 90-220 m from the eye. A seek
replays only the 4 s warm-up, so captures show short trails.

## Ghost

`ghost.rs` (documents the mesh, look, pose and placement), `rbr_gameplay::ghost`, `rbr_formats::ghost`. Ours:
`--best-ghost`. RSF ghosts (`.gch`) are not supported (user decision).

## Characters

`characters/` (documents the spawn, triggers, visibility, crew and scripted events): `.lbs` `(2,14)` items in
`(2,15)` random groups, Lua 4.0 scripts, `.sga` clips, CPU skinning. Generators: the exe's LCG and msvcr71 `rand()`,
`--seed`. In-car crew: chase views only in the game, as the High renderer. Ours (2026-11-10, `characters::incar`,
`CarView::crew`): the cockpit draws the driver's arms (the faces whose vertices each follow a node from the forearm
down most: the eye is in or behind his head, and his helmet, HANS and body filled the view on the GC8, the quattro,
the Clio Rally3 and the GR Yaris) and the co-driver whole, in the cabin's light (the interior set's marks, 0.12 of the
sky seen up through the windscreen, not baked: `--debug-view cabin-light` has the arms at the dash's level; at 0.4
they glowed). The arms' sleeves are one dark colour there (`SLEEVE`; the team's 128 by 64 suit texture read as a blue
smear from the upper arm down), the gloves keep their texture; the arms off by default, the co-driver and the car's wheel on, each chosen in `[camera]` (`driver`,
`codriver`, `wheel`; the pause menu's Camera page, the launcher's Graphics page). A model's own crew in its interior
set (`i_drivers`: the RSF Fiesta Rally2, i20 Rally2, i20 WRC 2020 and 2021) is not drawn (`car::BAKED_CREW`); behind
`Cam_internal` the cockpit eye saw the driver's helmet from the back (the Fiesta without its `_personal.ini`). Tests: `tests/characters_data.rs`,
`scripted_data.rs`, `crew_data.rs`.

## Fences and dials

`fences/` (documents the cloth, spans, posts and textures): `.fnc` lines as `CFenceHandler` cloth, pushed by the
car's `c_e_body`. With the own light (ours) the lines carry their normals; fence meshes skip `on_foliage` and
`with_plants` (the 45_O net was a "plant", glowing in patches). Without the own light they stay facing up;
additions off equal on 64_O, 45_O, 61_M but 1 to 2 pixels by 1 (shader codegen). Cost: +0.17 ms scene at 1440p with
a net filling the view. A torn line's rewrite has occlusion 0 (no bake), as before. `dials/` (ours): a community car's
needles and displays from RBRHUD's gauges, drawn into the dial textures from the HUD records.

## Enhancements

`renderer::Enhance`, `--enhance a,b` or `all`; each off (but for the always-on HDR and TAA) leaves the frame as the
game draws it. The launcher's Quality section (`graphics_page::Graphics`, user decisions 2026-10-29) sets them by
preset (Low, Medium, High, Ultra by default) or one by one: `anisotropy`, `decals` and `water` always on (free), `wet` with `--reflections` as "Wet roads" (off; low: no screen-space rays;
medium; high), `grass` as "Dense grass" on High and Ultra (user decision 2026-10-30: a half-density, shorter
field cost more frame time than the full one at 4K, so no middle setting), `windscreen` as "Windscreen" on High and
Ultra (user decision 2026-10-30: one switch for the pane and its reflection). Removed: `lens`; `ao`, `msaa`, `taa`,
`wind` (user decision 2026-10-19); `hdr` (2026-10-26: float targets, bloom, exposure and the tone curve are always on); `asphalt`, `gravel`, `snow`, `ground` (2026-11-02, below). Older settings and names are
not read (user decision). The launcher's render scale is 100 % (native) by default, else 87, 75, 67 or 50 %, which the
TAA upscales (user, 2026-11-01: at least 50 %; 75 kept, older settings hold it). The launcher offers no
anti-aliasing choice (user, 2026-11-01): the TAA either resolves or upscales; a saved `anti_aliasing` is not read, and
`viewer --anti-aliasing off` stays for the byte-identical checks. The default frame was
byte-identical before and after (chase, cockpit, heavy rain, 1920x1080).

| Name | What |
| --- | --- |
| `anisotropy` | 16x anisotropic stage samplers |
| `cabin` | cockpit lit by its baked sky share, sun only through glass, interior set clear of the stage fog; with the day's light the interior set takes it as the lit draws outside do (45O Xsara and the Evo IX, dry, wet, fog; scene pass within noise; additions off byte-identical, 2026-10-25); the car's glass drawn from both sides, so the far window seen through the near one tints too (the game culls it) |
| `contact` | soft contact shadow in place of the decal |
| `decals` | soft ground decals blended after the stage instead of cut at 60/255 (`decals.rs` documents the rule: stages 129, 369, 370 skid marks and 129 sand patches; no retail draw; the factory's blend switch untraced). Upright soft cut-outs keep the cut: blended, the wire mesh vanishes |
| `lighting` | our lighting in place of the game's look (2026-10-27: one flag for the former `sky`, `sun`, `occlusion` and `clouds`; the half-on looks, the 2D cloud layer and its cloud-face lighting went): the physically based sky with aerial perspective, height fog, light shafts and volumetric clouds with their shadows; sun shadow cascades and sunlit foliage in place of the baked light maps; ambient occlusion and bounced light computed at load; reflection probes; the day's exposure. Details below |
| ~~`hdr`~~ (always on) | float targets, glare (bloom until 2026-11-05), histogram exposure, Gran Turismo 7's tone curve (`post.rs`; AgX until 2026-11-05, below); the exposure's limits follow the clock's light, heavy rain or snow a fifth darker, heavy cloud 15% more saturated. Local exposure: where the bloom blur is over 2 stops above middle grey, 0.6 off, so the night beam keeps the road's texture (45O cockpit 23:00: 24% clipped, now none); post pass 2.91-2.94 ms; additions off byte-identical (2026-10-25). Tried and dropped: a 97th-percentile cap (blacked out everything around the beam). Daytime exposure under our sun lighting (`Enhance::lighting`, 2026-10-27; `post::SUN_EXPOSURE` 1.5, `DayLight::sun_share`): 41M chase noon lit grass (188 207 141) to (166 184 124), road 190 to 137; 45O noon grass green 165 to 121, road 178 to 115. 1.5 keeps sunlit grass above a photo's 120-140 green; 2 crushes the shadows (31 to 24). AgX contrast followed the adapting light (Stevens effect): day p95/p99 up ~10 levels, p5 down ~6; post pass 3.34 to 3.35 ms; additions off byte-identical. GT7 in place of AgX (user's pick on the lighting sheet, 2026-11-05; Polyphony Digital's SIGGRAPH 2025 operator, MIT, `notes/references`): its curve per channel blended with its intensity in ICtCp, chroma fading only near white; the exposure's middle grey (`KEY`) where GT7 shows it at AgX's sunny display grey (0.415). No contrast by the adapting light and no 1.1 saturation (AgX's). Sheet, 7 views x 6 hours x dry/wet/fog, AgX against GT7: mean luma within 1-3 levels, p95 up 3-16 (day dry 155 to 164, dusk dry 162 to 175), clipped pixels 0 to 0.3-2.8% (the low sun, the night beam), chroma 5-20% lower, nights about a level darker. Post pass 0.27 to 0.34 ms at 1080p, 0.84 to 1.09 ms at 4K (45O chase, RX 9060 XT radv). It reaches white about 3.3 stops over middle grey; the same operator drives HDR displays by its peak alone. HDR output (`viewer --hdr`, the launcher's HDR, off by default; 2026-11-05): `Rgba16Float` in extended-range sRGB where the surface has it (`present::surface_colour`: Metal's EDR; DX12 has no such space, Vulkan per driver and compositor; else SDR, logged); the paper white stays at the display's SDR white (1) and the curve peaks at the paper white times the display's live headroom (`tone_map_headroom`, read each frame; `HDR` override in `post.wgsl`), so the HUD, egui and the loading screen draw unchanged and screenshots and videos stay SDR. SDR byte-identical on the 11 light-level scenes; offscreen `Rgba16Float` on 44E's lake: headroom 1 within 0.6/255 of SDR, headroom 4 leaves coded values under 0.7 unchanged and takes 5.3% of the pixels (glints, sky) past white up to srgb(4). Seen on a 14" MacBook Pro's XDR display (potential headroom 16): looked good (user, 2026-11-05). Glare in place of the bloom, with the light sources at their real luminances (2026-11-05, after GT7's course; `post.wgsl` `glare_weight`): the Airy pattern of a round aperture over the chain's six levels (least squares, 27% rms in log), the F-number a camera at 1/125 s and ISO 100 takes for the adapting EV100 (f/1.4 to f/22): 5.4% of the light spread in sun, 0.5% at dusk and night; the bloom spread 6% always. The car's lamps (Lights) and, with the sky's tables, the sun's disc at its real size (0.27 degrees, pi times the day's sun over its solid angle) are drawn at their luminance, held under 30000; the local exposure reads the frame's light at most 8 times the display's white (uncapped, it darkened the car's rear about the tail lamps; at white, the night beam burnt out: 45O chase 23:00 3.7% clipped, 2.4% at 8). Tried and dropped: GT7's glare of the light past white alone (did not show, even 16 times as strong), Karis averaging of the first level (no flicker gain, weaker glare). Sheet against GT7 with the bloom: mean change 0.5-1.1 levels, more than 8 levels on 0.5-2.8% of the pixels, most at night (tail lamps, beam); post pass 0.33 to 0.36 ms at 1080p, 1.09 to 1.17 ms at 4K. A near-primary light past white whitens from its own hue before the curve (`whitened` in `post.wgsl`, 2026-11-05): per channel, a red lamp's core went orange (Rec.2020 red's green, 0.07 of it, clips before its blue, 0.02); now its least channel rises to its middle one from the curve's linear part to 4 peaks, so red holds to about 4 times white, pink by 16, white by 32; a hue whose second channel is over 0.4 of its first (amber) keeps the curve's. Night chase tails of the retail Impreza and 206 within 0.0004 RMSE |
| `marks` | our tyre marks in place of the skid marks (Skid marks; launcher default off) |
| `particles` | GPU rain and snow, rain rings, lit soft particles, chimney smoke, wheel spray. The rain or snow is decided once (`Scene::precipitation` in the viewer; `Rain::of_weather`, `Precipitation::of`): `--precipitation`, else with `particles` ours by the road, else the game's by the preset. Rings only in standing water, gone past 60 m over the view's cosine (2026-10-25). Rain looks (2026-10-26): 196000 faint streaks at full rain in place of 56000 bright needles, plus a fullscreen rain mist (a fifth of the lamps' light at night); no drop falls inside the cabin; zero without rain or `particles`, so additions off stay byte-identical. Streak look (2026-10-28): `--flyby`/`--camera` eyes take zero eye velocity (they took the car's ~25 m/s, slanting streaks ~40 degrees); `SHUTTER` 1/30 to 1/60 s; dry and damp shots byte-identical. About +0.5 to 1 ms on 45O chase wet, the denser drops another +1 ms on heavy; 30 chase frames without flicker. Drizzle (2026-10-31): `Precipitation::shower()`, the intensity squared, sets the streak count and the glass rates (light rain 0.25, heavy 1). In-car rain (2026-10-31, `IN_CAR_CLEAR`): none within 6 m of the eye, fading in by 10 m (the cabin alone left long bright streaks across the windscreen; clearing 3 m took a sixth of them, 6 m half, GC8 Luceram heavy); the far rain and the rain in the headlights stay; snow keeps the cabin's 2 m; outside views and the game's own rain unchanged |
| `grass` | not in `all` (launcher "Dense grass", default off; the reference shots take it): crossed cards of drawn grass blades over the green and straw ground within 20 m of the driveline (`grass.rs`), streamed by 8 m tile about the eye into 550 fixed tile meshes in 9 pools by card count (a tile takes the smallest free one holding its cards, bare ground none), drawn within 118 m (`Lod::Close`), full within 50 m then halving every 25 m, flattened into the ground from 100 to 110 m; each tile holds 1.5x its density (by its nearest point) on 24 steps and the vertex shader shrinks each card by its rank against the density where it stands (`GRASS_THIN`), so the field glides instead of popping; swayed by the wind in the vertex shader (`grass_sway`). Tiles are made on a worker thread (the near 24 m on the frame itself, uploads capped; tiles ahead of the motion built early). Lit as a dense canopy (`GRASS_*` in `shader.wgsl`): tighter wrap, sky and sun shut out down the blades, a low sun more; the ground's computed sky occlusion, sky direction and bounce read back once at load (`Renderer::grass_light`). 580 chase 1080p scene 5.05 to 5.14 ms (+39k triangles), Gabiria fly-by 3.76 to 3.62 ms (-77k triangles); ~280 MB video memory (2026-10-30; ~540 MB since the reach below). Look (2026-10-30): cards 0.07 to 0.14 m (were 0.1 to 0.2), their foot lighter (`GRASS_BASE` 0.6, `GRASS_SUN_BASE` 0.4 to 0.6), so the field reads as a low meadow, not dark fur; the retail tufts toned to sit with it (`TUFT_TRANSMIT` 0.35). Snow shots unchanged (RMSE under 0.005). Retail tufts lit as our grass (2026-10-30): the stage's tufts and ferns take our grass's light (`GRASS_*`: wrap 0.3, sun and sky shut out down them, no warm tips), keep 0.4 of the sky's colour (`TUFT_SKY_TINT`: their grey-green textures read bluish), no dark-leaf lift, green leaves at most 0.09 linear (`TUFT_GREEN_MOST`, was 0.15; 580's lime ferns read as lit card). Their luma over our grass's in the same 48-pixel tiles: 580 Evo chase 2.4-2.5 to 1.2-1.3, 580 shot 3.7 to 1.5, 532 1.34 to 0.89, Gabiria's pale straw 2.1 to 1.9 (its texels sit in the snow test's range: a straw cap missed them, and moving the range changed Pian's snow tufts). Snow shots RMSE under 0.0035, night Sardian identical. The tile meshes not yet given a tile (no bounds, zeroed cards: 306 of 560 at the Luceram shot) drew their whole capacity in the main view and the mirrors until skipped (2026-10-31): scene 0.5-0.7 ms less (580 chase 5.09 to 4.59, Luceram rain fly-by 3.83 to 3.18), mirrors 0.45-1.0 ms less in the cockpit, the 34 shots byte-identical. Their zeroed vertices no longer go through the load's vertex prep (2026-10-31): all in one place, they made `foliage::with_plants`'s tree grouping quadratic; 580 load 28 to 22 s, the 34 shots 594 to 441 s, byte-identical. Tried and dropped: lighting our blades as upright on banks (no visible change; 580's dark bank is the hill's own shadow). Thatch (2026-10-30): with a field (`GRASS`) the ground under the grass is darker (0.82) and fuller in colour (1.1) by the grass's own test, slope and density by the eye's distance (`thatch` in `shader.wgsl`; not the 20 m band, so the near ground past it is thatched too), so its pale and pink texels no longer show between the blades; the blades brighter (`SHADE` 0.62 to 0.9), less saturated (`SATURATION` 1.15 to 0.85, `LUSH_SHARE` 0.25 to 0.15: lit, 580's came out at blue over green 0.26 against the ground's 0.63, now 0.37 against 0.57) and fuller (240, 180, 180, 140 blades a picture, were 170, 130, 130, 100; the same cards). Blades over the ground between them (median of 32 px tiles): 580 Evo chase 0.51 to 1.12 and 0.31 to 0.86, Gabiria fly-by 0.79 to 1.29, 532 0.48 to 0.90; the field's mean brightness 580 Evo +0.5% and -14%, 580 shot +8%, Gabiria +4%, 532 -8%. Snow shots unchanged; scene pass 580 19.8 ms and 532 11.8 ms both before and after. Reach (2026-10-31): full to 50 m (was 14), halving every 25 m (was 10), to 110 m (was 100); the tile meshes sized by their cards (a full 6400-card mesh for every tile in reach would be ~1.2 GB; sized on 580, 621, 532, 450, 417, 176 and 52, a few of the farthest at Odry 462's densest spot go without). Meshes never filled drew their whole capacity of zero-area cards (no bounds: never culled), about 0.5 to 2.4 M triangles a frame before: now they draw nothing until filled. Full density to 100 m measured 580 chase 29.4 to 39.0 ms a frame, 621 cockpit 12.5 to 29.4 ms (with the empty meshes still drawn); this reach, after the fix: scene pass 580 chase 20.1 to 21.0 ms, 621 cockpit 7.8 to 8.4, Odry chase 10.8 to 8.7, Gabiria fly-by 11.4 both |
| `water` | water flowing by its scroll (`water.wgsl`), fading into banks |
| `windscreen` | the windscreen seen from inside as glass: the pane (Fresnel loss, dust, specks, wiper scratches, water spots) and the cabin reflected in it, a mirror pass of its own every 2nd frame whatever `--mirrors` says (held with the car between) ("Windscreen and wipers" above). Off: the frame as before |
| `wet` | the wet road, in wet and damp weather: a film and puddles over the ground whose `.col` surface takes them (tarmac 1, gravel and dirt 0.5), retail and community alike (user decision 2026-10-25; `puddles.rs`). Screen-space reflections on the wet roads: +12 to 17 ms at 1080p; film alone +0.9 to 1.7 ms; load +0.2 s. Wet film (2026-10-28): the rays missed the car, so the film halves back to any crossing and `reach` judges it (`film` in `trace`); the far road takes the sky, the near road stays dark. 41O chase screen reflections 1.32 to 1.50 ms at 1080p. Loose roads (2026-10-28): the half-film road's film was a pale even sheet; its reflection now falls with the film's share, so the stones show through and puddles stand out (tarmac and dry byte-identical; 52O/21O chase screen reflections 1.46/1.67 to 1.14/1.37 ms). The rings were never on the film: they were in the puddles. Damp is a drying road: no puddles, rings or screen-space reflections, the film 0.4 of the wet one's, the ground soaked by half (41M dry 101, damp 68, wet 59 at 13:00; 460M 147 to 131; 45O 140 to 118). 41M +0.9 ms, 45O within noise; wet and additions off unchanged byte for byte (2026-10-26). Wet road edge (2026-10-28): the reflection and glint fade over the outer 0.1 to 0.8 m, frayed by noise (puddles and the road's darkness stay); mean distance 1.2 m on 41M, 0.3 m on Gabiria; damp and dry byte-identical. By texture (2026-10-28): a film triangle takes the share most film triangles of its texture have (Bois de Bie tags stretches of tarmac as gravel: 46000 `Gravel_Fine_Shallow_Wet` against 139000 `Tarmac_Coarse_Clean_Wet`); retail Fraizer Wells byte for byte, damp Gabiria RMSE 0.0003. Wet road look (2026-10-28, tarmac first): the film alone is darker and its reflection takes the asphalt's grain, its normal tilted by microrelief and rain's impacts, fading to the mean between 4 and 25 m. Tried and dropped: a 0.12 tilt (white sky blobs over the near road). Gravel nearly unchanged; dry, snow, night byte-identical. Flyby 1080p scene 1.72 to 1.80 ms, screen reflections 1.58 to 1.85 ms; cockpit within noise. Tarmac's even film (2026-10-28, user decision): where the film share is 1 there are no random puddles and only the hollows hold water, so the bumper and plate read in the road near the car. Rain rings on tarmac only in the hollows' water; the film shows fine splash specks instead. Round 2 roughens the normal at the grain in place of the 5 and 2 cm waves. The near road's olive cast is the green bank's reflection, kept. Gravel and dirt keep the film and puddles (52O chase RMSE 0.0000022); dry, damp, snow and night-dry byte-identical; Luceram fly-by within compiler noise (RMSE 0.00015). Puddles at the edge (2026-10-28): puddles now end 0.05 to 0.2 m from the edge, frayed as the film is; 52O chase RMSE 0.017, dry byte-identical. Wheel tracks (2026-10-28, loose roads only): the water lies in two tracks 1.5 m in from the road's edge, rims frayed by 7 cm stone noise; damp keeps its old spray. 52O chase scene 2.54 to 2.65 ms, screen reflections 1.69 to 1.74 ms; dry, damp, snow, night byte-identical, Luceram within compiler noise (RMSE 0.00015). Cost at 4K (2026-11-06, wet 127 chase): `wet_plus` 1.05 ms of the scene, `puddled` 0.9 of it; the edge's fray noise once a pixel (was three times) and the zero-share shapes skipped (stray puddles on tarmac, tracks and stones off loose roads): scene 6.70 to 6.51, screen reflections 2.90 to 2.81; B 5.82 to 5.64 and 1.82 to 1.70; exact (1080p shots RMSE 0 but a pixel) |

Motion blur (ours, experimental, 2026-11-11): `--enhance blur` (`Enhance::motion_blur`, not in `all`; the launcher's
"Motion blur", `motion_blur`, off by default and left alone by the presets), only with the TAA: the resolve writes into the blur's target (the output's size) and `motion_blur.wgsl`
gathers it along each pixel's blur vector into the post pass's scene. The velocity is the motion target's where z >
0.5 else the point from the reversed depth through the TAA's `view_to_prev` (as `previous_uv` finds it), times
`SHUTTER` 1/120 s over the frame time (0 past 0.1 s or without a previous frame; the shader takes 0 from the resolve's
`params.z` on a cut), centred and capped at `MOST` 0.05 of the output height; 11 taps with interleaved gradient noise,
each but the centre weighted by its own vector's length over the centre's, so the car and the cabin stay sharp and
bleed nothing. The frame's time step is worked out on its first view, so a triple screen's side views blur as the
centre does. Cost (Gabiria chase): 0.09 ms at 1080p, 0.38 ms at 4K. Off byte-identical (chase, cockpit RMSE 0).

Dense grass in a screenshot (2026-11-11, `Renderer::wait_for_grass`): `viewer --screenshot` waits for the worker's
tiles before its frame. One frame held only the tiles built on it (within `SYNC_DISTANCE`, 24 m), so every
reference shot showed a bare hillside past that where a drive has the field to 110 m (580 chase, the bank to the
right). A repeat render is byte-identical; the window and the videos stream as before.

Dense grass placement and size (2026-11-10):

- Placement uses unblurred blended textures before vertex tint, with grassiness at least 0.5 at the root and eight
  points around it, at least 12 cm away and enough for the cards, lean and wind. Samples across a triangle edge
  take the neighbouring ground through the existing tile lookup (`Field::vegetation`, `supported`; tests cover
  tinted pavement, narrow green cracks and grass/pavement seams). Tint and blurred colour still colour the blades.
- A candidate triangle's centre must be within 10 cm of collision ground where available, excluding elevated
  props (`grass::add`). Collision tarmac alone cannot exclude grass: Helette 580's grassy `Route_O.dds` shoulders
  also lie on tarmac-labelled collision (installed-stage diagnostic, dry/new).
- Final caps after patch and kind scaling: ordinary grass 14 cm, seed heads 20 cm, card width 20 cm (`MAX_HEIGHT`,
  `MAX_WIDTH`; grassy road-shoulder fixture). Previously the 7–14 cm base reached 39.9 cm tall and 63.84 cm wide.
- Golden-hour chase captures at 1080p: Helette 580, Gabiria 450 and Wythop 532 keep their verges with shorter
  tufts, RMSE 0.0135/0.0225/0.0335 against the prior placement (`scripts/render-shots.py`, same GPU and build inputs).

Scanned surfaces removed (user decision 2026-11-02): `asphalt`, `gravel`, `snow` and `ground` (Poly Haven scans over
the roads, snow and off-road ground; launcher "Surfaces") did not look right on every stage. The road and ground marks
stay for the grass field (`Stage::mark_surfaces`).

Lighting, in pieces (all with `lighting`; each measured on its own before the merge):

- Sky: Hillaire sky, aerial perspective, height fog, light shafts; the tables are stored in display colours once per sky
  change (same frame within 1/255, 2026-10-20); under a clear sky the ground haze hides 70% no nearer than 7.7, 4.6 or
  2.3 km (Koschmieder visibility 25, 15, 7.5 km; 2026-10-27), the day's mist 95% at 33 km.
- Probes, occlusion, bounce, contact: reflection probes (`renderer/probes.rs`) along the driveline replace both
  per-frame cubes (headless 117 to 256 probes in 0.44 to 1.43 s, up to 67 MB). 41M 1080p drawn only 21.4 to 20.3 ms
  wet, 10.3 to 8.2 ms dry; additions off unchanged byte for byte (2026-10-25). Ambient occlusion from the stage's
  geometry at load (`sky_occlusion.rs`; 0.1 s on 41M, 0.5 s on Semetin, 2026-10-23): unlike the bake's `sky_share`,
  the computed one holds no sun shadows. With it the stage's light is ours (`own_light`): the sky 0.56 of it times
  the sky seen, the sun 0.79 by its facing past the cascades, so a shadow is the sky's light alone; the bake gives
  only the hue. Every stage within about 10% of the bake-driven frame's brightness; the weak fits and the unbaked
  gain the sun's modelling, dusk at -3 degrees the twilight sky's light (45O mean 15 to 24 of 255). The light off the
  stage (`bounce.rs`): drawn once at load (no running time of day, 2026-10-25); 0.1 to 0.4 s (Semetin), the bake
  0.1-0.5 to 0.2-0.9 s. Below the horizon, then also above (2026-10-26): Valstagna's shaded cliff +20% (41.6 to 49.9
  of 255). Tried and dropped: light through the leaves (nothing visible at 3x sun). Partly shaded places brighter
  (42O chase +38% mean, 460M +19%, 176M +13%), frame time within noise. With additions off the frame moved by up to
  14 of 255 on 88 pixels of 41M's two characters, unexplained. Contact shading: half size, on the sky's and bounce's
  light only; +0.4 to 0.8 ms over the mask alone (2026-10-25; full size +2 to 3 ms). `Detail::Ultra` reflections
  (2026-10-27): the surroundings cube at 256 texels, a face every frame, the probes baked at 128 texels, and the wet
  screen-space reflections traced at full size (split above 1440 rows, 2026-11-06). 41M 1440p: dry +0.05 ms, wet screen reflections 1.75 to 3.4 ms; High
  byte for byte (6 scenes). Sky domes (`sky_dome.rs`): a world sky face over the stage's middle casts into neither
  the sky's occlusion nor the cascades (39 of 659 readable community variants, e.g. 519, 580); as a caster it left 519 and
  580 black. Under `lighting` the dome is not drawn at all (user decision; 2026-10-27).
  Probe transitions (2026-11-10, `shader.wgsl::surrounding`): bilinear between the 32 m grid's cell centres, each
  naming its nearest probe, clamped at the grid's edge. The former cell seed plus its two route neighbours switched
  between disjoint pairs on folded tracks. Nordschleife's 256 probes, 81.5 m apart over 20.8 km: fixed 16:58,
  light cloud, Ultra captures across 20 cm at the start/finish and near 12.20 and 12.22 km reduce reflection-sample
  jumps from 76, 25 and 28/255 to 0, 0 and 1/255. Cube count and bake unchanged; four cube reads instead of two.
- Sun: cascades and a far layer to 1 km in place of the baked light maps, leaf translucency (`sun.rs`,
  `foliage.rs`); the characters cast into the cascades, and the car's body and the characters take them (the car
  stays out, its own map shadows it). A plant taller than grass is a crown (`trees`: the retail trees are three wings
  about 120 degrees apart); its cards fade out edge-on and take their sun from the crown (Beer-Lambert out of its
  ellipsoid). 41M 1080p drawn only 3.93 to 4.11 ms. Tried and dropped (2026-10-26): per-tree tint (snow firs turned
  brown on 10O), leaf-gradient normals, per-pixel crown normals (slight), turned card copies (square tops, darker),
  interior transmission (lost the backlit glow). Vegetation look (2026-10-28; `tuft_*` in `shader.wgsl`, `foliage::with_plants`):
  grass, low bushes and shrub crowns get bent normals, wrapped sun with transmission, foot occlusion and a straw tint
  on pale textures (retail tufts read bluish under the blue sky); gab-lit 1.9% RMSE, additions off identical. Each piece's cut-out test runs once a piece over a summed-area
  table: Semetin's load 7.4 to 2.3 s, 41M 1.08 to 0.67 s, frames identical (2026-10-25). Structures in the community
  stages' own atlases (2026-11-01, `foliage::Marks`): a piece up to 9 m whose part of the texture, read at 1024 texels,
  has few gaps and is not green, or straight edges, is not a plant; people, signs, fences, poles, buildings and
  vehicles share the atlases with the trees, and names alone (`STRUCTURES`) caught 31% of them (4% by use). A tree
  fitted to 1391 hand-labelled community pieces (768 drawn by use): with the names 95% caught, 1% of plants taken (0.3% by
  use), cross-validated by texture 93% and 3%; 192 pieces of unseen textures: 90% of those it calls structures are,
  98% of those it calls plants are. Misses: chain-link and wire (mostly named), bare twigs and dry stalks. At 256
  texels the gaps do not show (9% of plants taken). Retail atlases (tiny dry plants, cacti) failed it: only a stage's
  own textures take it, retail stages unchanged. 394's plant pass +1 s of one thread. 34 shots: 7
  byte-identical (retail 52 among them), the rest within 0.8% RMSE. The main view's sun shadow is worked out once per
  pixel before the stage is shaded (PCSS, 3x3 blocker search). Semetin 5.39 to 6.06 ms at 1920x1080 (41M similar,
  2026-10-25). The prepass's normals go to an `Rgb10a2Unorm` target of their own: the prepass 1.0 to 0.52 ms at 1440p
  (4K similar); with TAA, frames differ on 0.3-1% of pixels, mostly by 1/255, dusk and night byte-identical. The first
  cascade's depth range per 4x4 texels (`ShadowRange::sun`, 0.035 ms per frame) lets the mask skip where every texel
  is before or behind the point: frames byte-identical, the mask up to 4x faster (dusk 1.7 to 0.4 ms; 25O, 41M
  similar, 45O unchanged). The main view draws the lit opaque draws before the other stage draws: frames
  byte-identical on 45O and 41M, scene 1440p ~0.2 ms faster (45O chase 2.23 to 2.02 ms; 2026-10-26). `Detail::Ultra`
  (with `lighting`): the maps at 4096 texels instead of 2048 (400 MB of depth instead of 100), so foliage and trunk
  shadows on the road are cleaner; the far layer's texel 0.5 m. 1440p 25O, cascades 0.55-0.6 to 1.4-2.5 ms (+0.9 to +1.9;
  3072: +0.5), scene within the GPU's noise; `High` byte-identical to before (25O, 41M, 45O dusk and night). Tried and dropped: no alpha sample in the prepass for opaque draws and no `world_lit` where the
  cascades reach in full (2026-10-26; the whole sun term, `own_sun_seen` apart from the car, under 0.03 ms); the same reordering for every opaque
  non-prepassed world draw (no gain); single shading terms (each 0.1 ms or less, 48-72 VGPRs, no spills); far layer
  to 2.5 km (2026-10-27; +0.1 ms, nothing more seen); contact shading at full size (+0.1 to 0.2 ms, edge pixels only).
  Fewer casters (2026-10-30, `--profile` times each layer): the far layer is drawn again only after 50 m of travel
  or a change of light or size (`sun::FarDrawn`); the grass's tiles (`Lod::Close`) go into no layer and no cube; the
  backdrop's meshes are culled by each map's sides. 1080p Ultra, 580 chase: sun maps 7.36 to 3.92 ms, surroundings
  cube 0.82 to 0.52, CPU drawn-only 15.4 to 12.2; Gabiria fly-by 3.83 to 1.64. What is left is cut-out fill at 4096
  (cascades 0.5/1.5/1.7 ms; 0.2/0.3/0.3 without cut-outs). Frames differ only in the paint's cube (no grass) and the
  asphalt. Tried and dropped: alpha from a coarser mip in `fs_sun` (-0.5 ms on cascade 3, changes coverage), cut-out
  casters sorted front to back (no gain). The mirrors draw only the rain's near layer (the far one's drops are under a
  pixel there): 4K Luceram rain cockpit mirrors 0.91 to 0.72 ms, the mirror images unchanged to the eye (2026-10-30).
- Cascades in turn (2026-11-06, `sun::CascadesDrawn`, `sun::TURNS`): the first every frame, the second every 2nd, the
  third every 4th between the second's; between turns a cascade is read through the matrix it was drawn with while its
  slice's 12 points (corners, near ones also at the near distance) lie 4 texels inside it, else drawn again (cuts, light
  or size changes). Driving at 60 fps (`--sequence 40 --sequence-step 12`, 127 and 580 chase) every kept frame held.
  4K offscreen Ultra, RX 9060 XT, median of 3: cascades and lamp shadow 127 chase 2.46 to 1.18 ms, 127 cockpit 2.50 to
  1.21, 450 rain cockpit 2.13 to 1.04, 580 sunny chase 1.68 to 0.84, 450 sunny cockpit 3.04 to 1.47; CPU span 0.5-1.4 to
  0.2-0.6, the wait for them 0.8-1.6 to 0.45-1.15. Shots byte-identical (a screenshot draws its cascades fresh); the
  driving sequences against before RMSE 0.0001-0.0006 per frame (leaf flecks at 16x, no seams). Offscreen frame change
  580 chase 0.41 to 0.58 ms mean (the turns' uneven cost). Tried and dropped: the first cascade every 2nd frame (held
  the view, but the chase car came out darker: parts of it cast into that cascade and shaded it a frame late). The first
  cascade's depth ranges (`ShadowRange::sun`) cost 0.21 ms a frame at 4096 (reading the layer); caster culling by the
  view's slice cannot be exact: the surroundings and reflection cubes and the mirrors read the cascades all round.
- Clouds: the main view's clouds are volumetric (`renderer/cloud_volume.rs`, `cloud_volume.wgsl`; by night lit by the
  moon) and the game's cloud faces are not drawn (2026-10-25). The layer's kind by the cover: clear cumulus 1.2 km up
  under cirrus, partly 2.2 km thick, light stratocumulus 1.1 km up, overcast a closed deck 0.9 km up. A sunlit layer's
  shape is cut at most at 0.65 of its weather, the weather's threshold lowered to keep the cover (cut at the full
  weather they stood as flat-topped columns, 2026-10-27). Each section has its own field start and drift. About 0.7
  to 0.8 ms on a 1080p open sky; additions off and all but the clouds unchanged byte for byte. With the volume shown
  the ground's shadows come from the volume itself (a 512² R8 map over 8 km about the eye, redrawn each frame;
  `cloud_volume::shadow`, 2026-10-27). Overcast, night, and additions off byte-identical; 1440p scene medians within
  noise of before. The shadow march passes the height within the layer, 0.1 to 0.9 at its five samples, to
  `volume_density` (2026-11-10): adding the layer base before normalizing had put every clear and light-cloud sample
  above 1, so those presets cast no shadows. Nordschleife at 13:00, light cloud: open ground 0, cloud shade 253 to
  254/255 in the raw shadow diagnostic.
  First frames (2026-11-11, `volume_now`, `volume_blend`): a direction the history does not hold (the first frame, a
  cut, the sky a turn brings in) is marched four times, the starts a quarter step apart, and a history of n frames
  takes the new one as 1 / (n + 1) until that is under `VOLUME_BLEND`. One march blended at 0.12 left its dither as
  a diagonal weave at every cloud edge of a screenshot (one or two frames) and for a few dozen frames after a cut;
  30 frames in, the image is as before. The steady frame marches once, as before.
  The 36 reference shots with the probe blend held fixed (2026-11-10): cloud shadows alone leave 24 pixel-identical,
  change seven by more than 1/255 RMS and five by less. Largest: Falstone 27.90, Pengonpohja 27.69, Sipirkakim
  26.88/255; direct warm light and sun shafts fall in cloud shade, with exposure sometimes lifting the sky. All four
  night views pixel-identical. The gallery is regenerated with both lighting fixes.
  Reference-shot choices (2026-11-10): Pengonpohja's quattro fly-by uses Partly cloudy at 18:00 with wet roads and
  no rain; Comb I's Yaris fly-by uses Clear at 05:45, sun 5.8 degrees up, for warm side light and backlit dust.
  Partly's 0.35 is a weather-noise threshold input, not measured sky coverage; choosing a preset also changes the
  cloud field's seed and layer shape. Pengonpohja's approved view has cloud opacity above 0.5 on 97.5% of sampled
  visible sky pixels (raw transmittance diagnostic, top 500 rows, 2026-11-10), median opacity 1; all those pixels
  clip near white in the normal render. That is the view, not the preset: see the full-sky survey below.
  Full-sky cloud survey (2026-11-10, `cargo run --release -p rbr-render --example cloud_survey -- OUT [preset] [WIDTH]
  [SECONDS]`): opacity > 0.5 over the whole upper hemisphere, equal solid angle, before exposure and terrain, 4 fields
  x 3 eyes x drift 0/900/1800 s. Clear 12-14% (FEW, 1-2 oktas), Partly 47-51% (SCT; NWS partly cloudy 3/8-5/8),
  Heavy 95-97% (OVC). Light at 0.5 gave 45-57%, no more than Partly; at 0.72 it gives 70-81% (BKN, 5-7 oktas), so
  `Clouds::Light` is 0.72. The `us_clouded` face keeps 0.5 (stage defaults unchanged). Light's shots against the RSF
  install: Luceram 0.56/255 RMS, Falstone 11.7/255 (less direct sun).
- Merge (2026-10-27): the four names are no longer accepted (`--enhance sky` is an unknown name); `LIGHTING` is the one
  pipeline override (was `SKY_LUT`, `CONTACT`, `PROBES`; `SUN_MASK` stays apart, it needs the pass). The 2D cloud layer
  is gone (`clouded`, `cloud_shape`, `cloud_face_lit`, `clouds.wgsl`): the cloud field's noise, `cloud_turn` and
  `cloud_shadow` live in `cloud_volume.wgsl`. `--enhance all` is byte-identical to before
  in 13 scenes. The sky in the baked reflection probes, wet roads, water and the car's paint no longer holds the 2D
  layer (up to 1/255 on a few thousand pixels, 3/255 at Valstagna's sea). `--no-relight` with lighting on stays a debug state.

Albedo and shadow colour (2026-10-27, `lighting`; `examples/albedo_survey.rs`): terrain mean linear albedo (Y,
saturation): 41M/45O grass 0.12-0.13 (0.73), 61M dirt 0.18 (0.66), 10O snow 0.77, grey road/rock 0.33-0.36 (0.41 on
32M's limestone). Commonly quoted ranges (from memory; not checked): grass visible 0.08-0.15 (sat 0.6-0.7), dry soil
0.15-0.25, fresh snow 0.8-0.9, worn asphalt 0.05-0.2, concrete 0.3-0.5. So grass, dirt and snow are within range; only
neutral mid-bright surfaces run 2-3x high and grass is a little over-saturated. `albedo_remap` (`shader.wgsl`):
neutral surfaces with Y 0.2-0.6 lose up to 40% (snow over 0.7 untouched), saturation over 0.5 eased by up to 15%.
Shadows are not a light fault (41M 15:30 tree shadow on grass: lit sat 0.71, shadow sat 0.56, cooler and greyer as
expected); the shadowed grass reads more saturated on screen because AgX desaturated the bright lit grass (the curve before
GT7), so the shared tone curve was left alone. Haze: `CLEAR_REACH` 15, 9, 4.5 to 7.7, 4.6, 2.3 km (crisp, plain, hazy) and
`DAY_HAZE` 18 to 33 km. 41M 2560x1440 scene 2.49 to 2.51 ms; additions off byte-identical on nine scenes.

Shadow lookups skip only taps that cannot change the pixel (2026-10-20: cockpit scene pass 13.9 to 11.5 ms at
2560x1440, frames equal). Tried and dropped: a min/max depth map of the cascades, corner taps first, a 1024 car map.

Cabin light: only the car's lit pipelines carry `CABIN`, and the open sky is a branch, not a `select` (2026-10-20:
cockpit scene pass 13.6 to 11.5 ms at 2560x1440, frames equal but 2 pixels by 1). Tried and dropped: an RG11B10
bloom chain. Shade about the car (2026-10-30, `cabin_shade`): the probes read a forest overhead about as bright as
the open sky (1.1 to 1.27 of it on 450, 523, 532) and their sun reach stays 1 under most trees, so the cabin kept its
light in shade. The first cascade at 5 x 5 points about the cabin now dims the window light to 0.7 where all is shade
(weighed by where each face looks out) and scales the sun bounced inside; per vertex (the sun map is visible to the
vertex stage), +0.05 ms at 4K (per pixel it was +0.6). Wythop 532 cockpit frames in shade darker, open and
overcast frames unchanged.
Window colour (2026-10-30, `cabin_window`): what lies outside the seen way is taken against the overhead per channel,
as `open_sky_light` against the mean; against the overhead's luma the sky's blue came in twice (in `day_sky` and in the
cube), and the Clio Rally3's white cage on Wythop 532 lit blue (blue minus red 78 to 33 of 255, the dash 40 to 18).
Day Gabiria Escort, night Sardian Clio, fog Alpes GC8 cockpits unchanged (20 pixels past 8 of 255).

Cabin bounce by the day's light (ours, 2026-10-30, `cabin_day_lit`): the constant inside floor and the sun's back share
are gone. A closed room's interreflection gives the bounce: E = (1 - sky) x rho / (1 - rho) x the mean light the
interior takes in, the sky's through the cabin's mean seen direction and the sun's mean n.l (`SunIn`). rho is the
area-weighted interior albedo (texel at each face's uv centroid at 64 px or less, times the vertex colour, per channel,
at most 0.9; the bounce per channel, so a tan cabin's is warm), over the faces that see the sky at most 0.6 to 0.9 of
the time (`cabin::OUTSIDE`): the interior set holds outside body panels, and the M3 E30's green livery (13.7 of its
28.7 m^2) lit its headliner teal (albedo 0.099 / 0.183 / 0.146, now 0.095 / 0.112 / 0.106; roof 19 / 44 / 34 of 255,
now 23 / 29 / 28). The light in (sky share, seen direction, the sun's n.l) is weighed so too (2026-10-31, with the
cabin's own exposure below): the body seen through the glass takes in light the cabin does not, and counted it made
the bounce 1.5 to 2.7 stops too bright (before, under one exposure, it took the M3's roof to 7 of 255 and was left
out). The day's window light and sky inside take `IBL_TINT` as the outside's ambient does (no visible change).
Measured (luma, the viewer's `cabin:` line) sky share (was) / albedo / bounce: Escort 0.090 (0.280) / 0.104 / 0.116,
GC8 0.068 (0.343) / 0.056 / 0.060, Clio Rally3 0.055 (0.361) / 0.084 / 0.092, 131 0.087 (0.222) / 0.152 / 0.179, 208
0.057 (0.296) / 0.087 / 0.095, Fiesta 0.059 (0.236) / 0.043 / 0.045, Yaris 0.059 (0.198) / 0.467 / 0.877, M3 E30 0.092
(0.380) / 0.108 / 0.122. Dark cabins fall off to the roof and
footwells, the white Yaris shell is lighter; the window term is untinted (`CABIN_DAY_RANGE` 0.03 to 4). The glass: the
sky's share through two clear faces, `(1 - F)^2` over the cosine-weighed hemisphere, 0.848 (`CABIN_SKY_THROUGH`; the
former 0.85 was this); the sun's from the blended panes facing it, weighed by area toward it, per measured direction
(`SunIn::through`; 208 at 19:30 about 0.8). Left heuristic: `CABIN_SHADE`, `CABIN_GAUGE_LEAST`.

Exposure in the cockpit (user's call, 2026-10-31; was one exposure, the cabin metered at 1/2 of the outside, before that
2/5): the outside and the cabin (nearer than `post::CABIN_REACH`) are metered apart (`cs_meter`, 128 bins each), as the
eye adapts to the dash when it looks there. The outside's meter counts the cabin at 1/5 (`IN_CAR_WEIGHT`): at 1/2 the
dark cabin held the outside 1.5 to 2 stops over (Escort road 168 of 255; metered alone 91). The cabin takes its own
exposure, eased as the outside's, at least the outside's and at most 4 stops over it (`CABIN_STOPS`), its darkest
values lifted by a toe (`CABIN_TOE`, 1 stop at black, a sixty-fifth at middle grey; both changed 2026-11-01, below), in a composite pipeline of its
own (`CABIN`). The glow and the light shafts keep the outside's exposure (at the cabin's they lit the pillars with a
green haze); a pixel takes the cabin's only when its 3 x 3 are all cabin (a softer blend drew a dark band inside the
pillars, or a bright halo outside them). Chosen against more cabin light (the light is right, above), a shadow-side
local exposure and the curve's toe alone, then from cabin weights 0, 1/5, 1/2 by 3, 4, 5 stops. Eight day cockpit
shots, means: outside 149 to 125, cabin 35 to 62, pixels at 8 or below 24.9 to 14.0 %, cabin detail (mean |luma - its
5 px blur|) 4.3 to 7.5; the two nights within a few levels. Post pass 0.29 to 0.36 ms (Escort cockpit, 1080p); the 24
outside shots byte-identical; 4 s of driving (Escort, Clio) hold the cabin within 2 levels, no pumping. Left: glass
that writes depth (the Clio Rally3's `CarPlastic` side windows) and the rear-view mirror count as cabin, so what is
seen in them takes the cabin's exposure (about 1.5 stops over on the Clio); the forest shots' door cards stay dark (5
to 7 stops under the road, which their light is).

Cabin depth (user's pick of five looks, 2026-11-01, `cabin_day_lit`): the sky share came out near zero on most faces,
so the bounce, the same on every face, lit nearly all of the cabin and it read flat. Three causes, three changes. The
bake took the exterior set's own low-detail cabin (Escort Mk II `e_internal`, i20 WRC 2020 `e_tablero`, `e_jaula`;
never drawn in the in-car view) as solid over the interior's dash: now only the exterior's painted shell
(`car::cabin::shell`, `Kf` over 0) casts with the interior set (mean sky share Escort 0.070 to 0.076, i20 0.090 to
0.104). The window light counts twice and the bounce half (`CABIN_WINDOW_LIGHT`, `CABIN_BOUNCE_SHARE`). What the
cabin's own faces close off nearby takes window light and half as much bounce (`CABIN_NEAR_BOUNCE`; in full it took
the Fiat 131's tan dash under its hood to black): per vertex, the rays that meet a face within 0.3 m
(`car::cabin::NEAR`, in `Vertex::occlusion`), and per pixel the contact shading at 0.2 m and strength 2.5 over the
cabin's depth, which the in-car view now lays in the prepass (`pipelines::prepassed`), read over 3 x 3 half-size texels
(one texel left a fixed speckle). The cabin's exposure is held to 2.5 stops over the outside's and the toe is gone (the
look keeps its blacks; alone the change was barely visible), dial faces at least half the open sky's light
(`CABIN_GAUGE_LEAST`, was a quarter). All 10 cockpit shots change (RMSE 1.7 % nights to 11.6 % fog Alpes GC8): dash tops
under the glass lit, binnacles, footwells and cage roofs dark, the GC8's blue cage no longer glowing. 1080p cockpit
scene pass (RX 9060 XT) 3.35 to 2.98 ms on 621 i20, 2.67 to 2.64 on Alpes GC8: the cabin's depth hides the stage
behind the dash. Tried and dropped: the near occlusion at twice its weight on the bounce too (recessed dash faces
black), the original textures in place of the upscaler's (grainier, no less flat).

Interior sheen (2026-10-31, `cabin_sheen`): every cabin face but paint open to the sky (the bonnet through the glass,
`CABIN_OUT`) reflects as a low-gloss plastic, `environment_reflectance` at roughness 0.8 (2 to 3 %) of the cube's
level 3, where the reflected ray leaves the car (`cabin_open`). A class with a Fresnel kept the paint's reflection: the
Clio Rally3's dash is `CarPaintInt` with `Kf` 1 from `FresnelMin` 0 (9 installed cars; 70 have 0.21 / 0.44) and read
as glass; the rest took a smooth F0 0.04, a third of the sky at a grazing angle. Mlynky 135 Clio cockpit 19:30: dash
top 108 to 85 of 255 (no sheen 86). Cockpit reference shots 1 to 2.6 % RMSE by day (Fiesta's grey dash 6.5 %), nights
0.02 to 0.04 %, outside shots byte-identical. The dash still reads blue-grey: near-black texels under the cabin's own
exposure and the sky through the windscreen.

Cabin albedo and colour (ours, 2026-11-12, `cabin_day_lit`; picked by the user 2026-11-12): the cabin textures' darks
decode at 2.2 to an albedo of 0.003 to 0.02, so dark dashes, wheels and door cards had no form and the cabin's
exposure burnt the labels out. Now gamma 1.6 for them (`CABIN_GAMMA`; the dials keep 2.2), a soft floor of 0.012
where a face's reflection stays in the car (`CABIN_INWARD`), and 0.7 of the window's and bounce light's colour taken
out, the sheen's too (`CABIN_NEUTRAL`; the sun keeps its own): the Fiesta Rally2's dash at Luceram 08:30 53 / 67 / 91
to 102 / 108 / 120 of 255. Ten cockpit shots at 2560x1440: days 5.6 to 13.1 % RMSE (i20 WRC most), the nights 0.16 %
(the sheen's colour), outside shots byte-identical; the brighter cabin takes the road a little darker (the outside's
meter counts the cabin at 1/5). The bounce's mean albedo (`frame.cabin`) is still taken at 2.2. Tried and dropped: a
normal from the texel's brightness (nothing at slope 3, wet blotches at 8), a procedural grain bump (not seen under
TAA), the sheen at roughness 0.55 (reads wet), a glass reflection over the dials (a grey veil), the bounce at 0.75 to
1 (slight), the floor alone at 0.02 or as a rough dielectric's share of the light (the i20's flocked dash a flat grey,
its texture gone), a floor in proportion to the texel (the cabin's exposure takes it back), the upscaler off (grainier,
as harsh). Per-car art (branch `render/car-look-sidecar`: a material sidecar, remodelled parts, a repainted Fiat 131
texture) gave less than this for one car each.

Cockpit debug view (viewer `--debug-view cabin-sky|cabin-near|cabin-contact|cabin-light`, ours; `renderer::DebugView`):
the interior set draws one term of its light (`cabin_debug`: the sky share, the near openness, the contact shading, the
irradiance over the open sky's, half of it mid-grey) as a display grey, past the exposure and the tone curve in
`post.wgsl` (`DEBUG_VIEW`), every other pixel as normal; the frame without it byte-identical.

The car's sun shadow (2026-10-30, `car_shadow`): a penumbra by the sun's disc and the casters' mean distance (3x3
search, PCSS as the cascades), taps a texel apart, at most 9 a side and the range's reach. The fixed 4x4 taps 4 cm
apart over 3 mm texels drew a 1 cm grid of steps where the sun came through the glass (208 binnacle, 19:30). 580 208
chase RMSE under 1 %; cost not measured.

The stage's shadow in the cabin (2026-11-11, `cabin_stage_lit`): a box of the first cascade's texels `CABIN_PENUMBRA`
(6 cm) to each side, 1 to 4 texels by the map's detail, in `texel_filter`'s taps for any width (k + 1 bilinear taps a
side for 2k + 1 texels, no gaps). The one filtered compare of before drew a tree's shadow on the dash and the door as
stair-stepped blocks (Pirttikulma 15:00, quattro door, Fiesta dash); `soft_filter`'s search with taps 2 texels apart
(2026-11-10) left a dot grid that the sheen below showed. Scene pass, Fiesta cockpit there at 1080p ultra: 2.70 ms
before and 2.69 after.

The sun's sheen in the cabin (2026-11-11, `CABIN_HIGHLIGHT`): the cabin's faces in the sun take a rough plastic's
highlight (`sun_lobe`, m 0.5), at most 0.035 (uncapped, the lobe's grazing Fresnel lit the dash white, which had the
highlight left out; at 0.2 and 0.08 the patches still clipped under the cabin's exposure). Without it a dark cabin's
sun did not show: the Fiesta's dash (albedo 0.04) moved 41 to 52 of 255 between shade and sun while its irradiance
moved 2.3x, and neither `CABIN_SHADE` 0.35 nor a fixed cabin exposure changed that visibly. Not on the dial faces.
The eight cockpit reference shots rendered (Gabiria 131, 621 i20, Luceram Fiesta, Alpes GC8, 532 Clio, Pirttikulma
quattro, Peklo GC8, Sardian Clio) are byte-identical: none has the sun on a cabin face.

## Mirrors

Ours: `mirrors/` (detection, views), `renderer/mirrors.rs` (passes), `mirrors.wgsl`; `viewer --mirrors off|1|2|3`,
launcher "Mirrors" (default every frame). Off: no passes, the frame byte-identical (6 views, 2026-10-19).

- Found at load from the cockpit eye (`Mounts`, `mirrors/mod.rs`): the `*mirrorglass*` parts (else the `*mirror*`
  housings' faces) and the opaque glass-class faces (retail cars), in panes of welded, coplanar faces. A pane is a
  mirror when the eye's line reflects back along the car, it is 3-45 cm, 0.2-1.5 m ahead, at most 45 cm below the
  eye, flat to 10 % of its size, its faces fill 55 % of their box, and the eye sees it past opaque non-glass faces;
  the largest by its faces' area, one per side and one centre. No named centre mirror (community cars): the largest
  opaque pane hanging there. Not flat: the cube as before.
- Set for the driver (2026-11-13, `Glass::aim`, user's call: the mirrors of the M3 E36 and the 911 GT3 RS 2007 and 2010
  showed only the car): the image is reflected across the plane through the glass's centre that sends the cockpit
  eye's line back along the car, not across the glass as modelled. A door mirror's line goes outward by 0.75 of the
  glass's half width as the eye sees it and down by 0.3 of its half height (the flank at the inner edge, the horizon
  above the middle); the centre one straight back, 2 degrees down. Set at load and again when the seat is nudged
  (`camera_prefs::Session::apply`). The glass samples the image where it lies on screen (the view before the
  reflection), and the image starts 3 cm plus the glass's reach before the plane (its housing left out).
  As modelled (99 added packs, 50 RSF and retail, 235 door mirrors): 52 showed only the car, 98 looked over 15 degrees
  outward, lines from 45 degrees inward to 57 outward and 51 down to 14 up; set, all 1 to 5 degrees outward, median
  9 % of the glass the car's own, one over 60 % (Celica ST185, right).
- Ranked by box area the rim about the glass won over it on the 911 GT3 RS 2010 (54 cm2 in a 23 x 16 cm box against
  the glass's 275; the glass kept the cube), and the M3 E36's and 911 GT3 RS 2007's mirrors were a larger pane than
  their glass. At 4 % the Peugeot 208 R2's centre glass (9 mm off over 22 cm) lost to its rim and the Fiesta Rally3,
  Rally4 and R2 2019 had no centre mirror; the 911 GT3 RS 2007's centre mirror was a lone triangle and the Yaris WRC
  2018's four faces 39 cm wide turned 45 degrees (both dropped). 303 mirrors over the 149, 301 before; 10 cars with
  none, as before.
- Survey (`examples/mirror_survey.rs`, also by a model's `.sgc` path): the retail cars 2 door mirrors each, Lancer Evo
  VII 1 door + centre, Impreza 2000 2 + centre; each with where its line goes and the car's share of its glass.
- View: `crop * VP * R`, R the reflection across the glass, crop the glass's screen box with x flipped, z row for an
  oblique near plane 1 cm past the glass. Image about 1:1 with the glass, 32 px steps, at most 1024 x 512. Rates take
  turns between mirrors.
- Under TAA each image is drawn with its own sub-texel jitter (Halton by the layer's draws), which the glass samples it
  by, so the main view's resolve gathers its edges (2026-10-31, no cost). The windscreen's curve shears its image low on
  the pane: an edge along a texel row showed as fins about 10 texels long, halved by a 2048 image (tried for Ultra,
  +1 ms) and only softened by a blur or FXAA; the jitter with TAA's glass-over-the-car rule removes them at 1024.
- Passes draw the stage to 300 m, sky, fog, the car, ghosts, the game's particles, rain and lamps, with the frame's
  sun cascades and exposure; no prepass, light shafts, cubes or bloom. Only the glass's pipelines have `MIRRORS`: on
  in the shared shader it cost 1.1 ms a frame at 1440p (M-series).
- Cost over Off, Evo X R4 cockpit, min of 5 (2026-10-19): every frame +0.66-0.82 ms at 1080p, +0.71-0.75 at 1440p,
  +0.71-1.13 at 4K; every 2nd +0.29-0.45; every 3rd +0.18-0.34.
- `--profile` times each layer (`mirror 1` to `4`, `windscreen reflection`). RX 9060 XT, 1080p ultra cockpits
  (2026-10-31): the windscreen's 1.2-1.5 ms every frame, the mirrors' 0.4-1.0 each; the stage in the windscreen's
  only 0.3 (the cabin hides it, a reach of 10 m costs as 300), the rain in it 0.07. Since drawn every 2nd frame (user's
  call, 2026-10-31): 532 Clio cockpit GPU frame 12.02 to 11.08 ms, mirrors 2.01 to 1.34.
- At 4K (2026-11-06, visual trade, for everyone): images at most as dense as on a 1440-pixel-high screen
  (`mirrors::DENSEST`; door mirrors were 416x416 and 800x288 at 1:1, the windscreen's 1024x352 stays at `MOST`).
  Median of 3, 4K: mirrors 127 cockpit 1.68 to 1.21 ms, 450 rain cockpit 0.96 to 0.84. 1080p and 1440p unchanged; 4K
  cockpit shots RMSE 0.001-0.004, the mirror slightly softer. Tried and dropped: a one-tap sun shadow in the passes (no
  gain), reach 150 m (no gain), the windscreen's image at a quarter (-0.18 ms of its 1.0 in rain), the grass thinning
  and ending at 55 m (-0.04 ms, less grass).
- The passes draw the lit opaque draws (the cabin, the flank) first, as the main view does (2026-11-06): the stage
  behind them fails the depth test. The windscreen's pass was the stage 0.57 ms, the car 0.3 (450 rain, 4K); now 0.99
  to 0.60 ms, 127 cockpit 0.44 to 0.35; 4K cockpit shots byte-identical.

### Cost on an M3 Pro (2026-10-29)

GPU frame time, `viewer --screenshot --frames 60` mid-drive (`render-shots.py`'s placement), 1080p, M3 Pro 14-core
GPU; each choice off from the launcher's High. Seven shots: 580 day chase, 52 gravel, 450 cockpit, 417 heavy rain
chase and cockpit, 126 night, 413 snow. Repeats within 0.5 %.

| Choice | Cost |
| --- | --- |
| `lighting` | 3.6-13.9 ms (18-53 %; 580 day most); load +1.1 to 6.2 s (occlusion, bounce, probes) |
| `particles` | 4.7-6.2 ms heavy rain (rain pass ~4.8, more in the mirrors), 1.5 snow, 0.2 dry |
| `wet` | 3.1-5.6 ms in rain, 0 dry; `--reflections` high to off 2.5-3.5 rain, 1.2 dry 580 (water); medium 1.0-1.6, low 1.6-2.2 |
| `--detail ultra` | +3.6-6.1 ms (+14-38 %; cascades 6.1 to 11.8 ms on 450) |
| `grass` | +3.5-4.2 ms; most in the cascades (+2.3) and the surroundings cube (+0.8), waiting profile |
| `--mirrors 1` | 0.1-0.3 ms dry cockpit, 2.9 heavy rain (the rain drawn in them ~2.2); every 2nd/3rd by the rate |
| `cabin` | 1.0-1.4 ms in the cockpit |
| `contact` | 0.1-0.8 ms |
| `--anti-aliasing off` | -0.5 to -0.75 ms |
| `anisotropy`, `decals`, `water`, `marks` | within noise |

Presets 1080p (native 3024x1964 on 580, 450, 417 x2): Low 5.7-8.8 ms (8.0-13.3), Medium 7.2-13.6 (13.1-32.1, measured
with low reflections), High 11.4-26.2 (23.7-47.6), Ultra 15.9-33.9 (33.1-63.7). Native at render scale 67 %: 15.9-27.5.
Lighting on with the rest cut saves only 1.5-2 ms dry. Tried: sun maps at 1024 saved 0.4-1.9 ms (the cascades are
geometry-bound), so no cheaper lighting tier by size.

### Performance log (2026-10-26)

Launcher Graphics page "Performance log" (`perf_log = on`, default off) passes `viewer --perf-log <files folder>/logs`
(`perf_log.rs`, `profile.rs`, `bin/viewer/perf.rs`; both document the files, spans and timestamp modes). Two files
per session, named by the start in UTC: `perf-<YYYYMMDD-HHMMSS>.log` and `.trace.json` (last 600 frames, written at
exit, so a killed viewer leaves only the `.log`).
- `viewer --frames N --perf-log DIR` logs the offscreen frames too (header `present mode offscreen`). Frames are
  byte-identical with it on (1080p `--enhance all` screenshot `cmp`). Overhead median +0.08 ms a frame (45O, 7 runs,
  640x360 and 1440p). Untested without a display: the window path (`App::frame` spans, `Renderer::end_frame`
  after the present's submit).
- `viewer --replay FILE --video OUT --perf-log DIR` logs each rendered frame of the replay offline (header `present
  mode video`): a live drive's spikes at any size without the window (each frame is read back, so the GPU clocks
  lower than live: compare runs with each other only).
- Live, the summary counts from the race's first go (`perf_log::mark`, a `t=…s go` line): the loading, the start
  orbit and the countdown stay in the lines a second only. The summary has p99.9 beside p99.

Scene pass at 4K (2026-11-05, Loch Ard 127 wet chase `--at 6750`, 6.0 ms steady): pixel-bound (2.3 ms at 1080p:
about 0.8 ms geometry, 0.72 ms a megapixel). Skipping draws' shading after the prepass: alpha-tested (foliage,
grass) 2.1 ms, opaque 1.3, blended under 0.1, the clouds' march 0.2; the rest is the prepass, the sun's mask and the
sky. Dense grass costs 0.9 ms on 580, 0.3 on 127. The stage's pixel shaders hold 72-84 VGPRs (radv
`RADV_DEBUG=shaderstats`: 18-20 waves a SIMD). Tried and dropped: the prepass sorted opaque first and front to back
(no change).

Cuts at 4K (2026-11-06, each byte-identical; RX 9060 XT radv, medians of 3 interleaved offscreen runs): a crown's
pixel works out only the crown's sun share and any other pixel only the shadows' (both were computed, each a 9-tap
filter: 580 scene -0.15 ms); inside the cascades' reach the world layer's filter is skipped (`mix` at 1: Odry scene
-0.09, 580 -0.06); the post composite's per-frame terms (the rods' white, the glare's summed weights) come from
`cs_meter` (-0.03). The lamps' beams in the air take the rear lamps in 4 steps (`REAR_STEPS`, the headlights keep 12;
visual trade, for everyone): Loch Ard at dusk light shafts 0.61 -> 0.40 ms, a night chase in heavy fog within 3/255.
Measured and dropped: the baked light map read only when used, the march's shade-only steps
skipped past `GLOW_REACH`, GT7's second ICtCp intensity alone (all no change: the compiler or the scene already had
them), the glare chain's taps (pass overhead and the 4K read dominate). Where the time is, 580 grass chase: terrain
detail 0.24 ms, the stage's own light 0.17, fog 0.11, the sun share 0.07; post 0.95 ms (tone curve 0.27, glare chain
0.33, sharpening 0.07); TAA 0.8 (bandwidth: two half-float writes a pixel); light shafts' march 0.52 (the shade-only
steps 0.13).

CPU frame (2026-11-06; RX 9060 XT radv, 32 threads). Measure the live CPU with a replay video (`viewer --replay FILE
--video OUT.mp4 --video-span 20,28 --perf-log DIR`): it runs the window's scene update and render each frame, and its
spans match a live log (finish scene commands 0.9 against 0.82 live). Offscreen `--frames` queues frames without a
wait and shows 2.2-2.4 ms. Where the live CPU went (user's 4K Loch Ard 127_N wet cockpit drive), mean ms:
- Scene update 2.84: the glass's drops 1.1 (sim 0.54, layout 0.40, upload 0.18), the car's moving parts 0.7 and the
  characters' skinning 0.6 (about half each the upload into staging memory), particles 0.43.
- Render 4.13: grass streaming 1.0 (copying 5-8 MB of rebuilt tiles a frame into staging memory while driving, at the
  bus's rate: copying in parallel did not help), the wait for the shadow thread 0.8 (it started only after the frame's
  preparation, and the mirrors waited for it).
- The input and live-state spikes (p99 4.2, p99.9 6.7): the frame thread locked the sound mixer's mutex (the
  H-shifter's grind every frame, each sound step), and the audio callback holds it for a whole block's mix.

Kept, each exact (4K and 1440p shots byte-identical). Replay-video medians of 3 interleaved runs, render mean / p99:
- The sun's maps are recorded from the start of `render()` and submitted with the first mirror. Loch Ard cockpit:
  1440p 4.57 / 6.18 -> 3.92 / 5.43, 4K 5.12 / 7.35 -> 4.48 / 6.18; the wait 0.65 -> 0.04. The dry chase (no mirrors)
  is unchanged.
- The grass worker stages its tiles in buffers of their own (`grass::Filled::staged`), so the frame only records the
  copy. Loch Ard: 1440p 3.96 -> 3.24, 4K 4.67 -> 3.58; dry 21 chase: 1440p 3.59 -> 2.90, 4K 3.92 -> 3.10.
- The mixer's commands go through a queue that the callback takes before each block (`rbr-sound` `posted`). The
  samples are identical (`posted_commands_play_as_applied`). Not measured live yet.

Measured and dropped:
- The characters, the car's parts and the glass's panes over rayon: means -0.1 to -0.3, but scene-update p99 went from
  2 to 6-14 ms.
- Skipping redundant pipeline and bind group sets in the stage passes: no change (wgpu's cost is per draw).
- A lookup for the grass ring's held tiles: under noise.

### Benchmark (2026-11-02)

`scripts/bench.py`: the autopilot drives the worst case live, fullscreen, every option at its most, frame rate
uncapped, force feedback off; the perf log times 90 s from the go (`viewer --quit-after go+90`); median of 3 runs,
`--compare OLD_DIR`. The scene was chosen on cachy (RX 9060 XT, radv, 4K): offscreen `--frames 60` cockpit mid-drive at
1440p, Gabiria (450) in heavy rain 11.97 ms drawn, Helette (580) rain 11.43, 580 day 9.82, 417 night rain 9.23, 621
rain 7.57, 417 rain 7.25, 417 day 6.24, Peklo (133) snow 5.82. Live, one run each: 450 rain 54.7 fps (1% low 42.2),
580 rain 56.2 (44.1); 580 costs more CPU (scene update 8.1 against 6.1 ms), 450 more GPU (screen reflections 6.3 ms).
Cars on 450 rain at 4K: Fiat 131 19.37 ms, Clio Rally3 18.08, GC8 18.14, Fiesta Rally2 17.77, Yaris 17.72, i20 WRC
16.94. So: 450, heavy rain, 10:00, Fiat 131 Abarth, cockpit. Baseline (cachy, 3840x2160, Mailbox: radv on niri offers
no Immediate): 54.7 fps, 1% low 42.2, 0.1% low 35.5, p50 18.26 ms, GPU 17.93 ms (GPU-bound), 1535 m driven.
It needs a display output on (2026-11-08, sway, monitor asleep: `swaymsg -t get_outputs` empty): fullscreen then
draws 1920x1080 to no screen, a `--size` window comes out 1x1 (bench.py fails the run), and a window whose output went
away mid-run stopped drawing and never quit (bench.py ends a run after 420 s plus `--seconds`).
Pacing and latency (2026-11-02): the perf log's `frame change` row (each frame's time against the one before) and
`RBR_LATENCY=frames`' "any input -> on screen" (from every ms of the drive from the go until the first frame that read
input after it was on screen, by Wayland's presentation feedback). SwayFX 0.6 (sway 1.12), same machine and scene, one
90 s run each:

| Present | fps | 1% low | pace ms | latency p50 / p95 ms | shown / discarded | present -> on screen p50 |
| --- | --- | --- | --- | --- | --- | --- |
| mailbox | 54.7 | 35.5 | 11.05 | 81.6 / 102.3 | 2735 / 2739 | 51.0 |
| fifo | 54.6 | 48.1 | 0.64 | 63.9 / 74.5 | 5465 / 0 | 36.7 |
| immediate | 54.6 | 35.5 | 11.24 | 64.7 / 85.7 | 2769 / 2701 | 34.0 |

Under sway, Mailbox and Immediate alternate 10 and 23 ms frames (`vkQueuePresent` blocks every second frame) and the
compositor shows every second one (about 27 fps on screen); FIFO shows all, evenly, with the lowest latency. Off
(30 s runs): VRR and SwayFX's blur and corners changed nothing. Under niri, Mailbox paced evenly (2.1 ms; the wait in
`acquire` each frame), latency not measured yet. FIFO's 37 ms from present to screen is the two frames in flight
(`desired_maximum_frame_latency` 2) of a GPU-bound loop: what a just-in-time frame start would take out.

Frame pacing (`bin/viewer/pacer.rs`, `--pacing on|off`, on): frames start no faster than the GPU finishes them, the
period moved each frame by a tenth of the gap between the GPU finishing the last frame and this one's submit missing
0.5 ms (`Queue::on_submitted_work_done`, a thread polling every 0.25 ms); a CPU-bound loop is left unpaced. Same scene
and sway, one 90 s run each, paced:

| Present | fps | 1% low | pace ms | latency p50 / p95 ms | discarded | present -> on screen p50 |
| --- | --- | --- | --- | --- | --- | --- |
| mailbox | 54.2 | 48.1 | 0.14 | 37.9 / 47.5 (81.6 / 102.3 unpaced) | 17 | 18.2 |
| fifo | 54.1 | 48.1 | 0.14 | 38.0 / 49.4 (63.9 / 74.5) | 0 | 18.2 |
| immediate | 54.2 | 48.1 | 0.14 | 36.7 / 47.0 (64.7 / 85.7) | 4 | 17.9 |

The rest is the frame: half a period's wait for the next input read (~9 ms), input read to submit ~10 ms (the live
answers, scene update, encoding), the GPU's 18 ms. Not measured yet: niri, Windows.

macOS (2026-11-02, M3 Pro, 120 Hz ProMotion, fullscreen, fifo, one 15 s run each): the bench scene, GPU-bound, 18.5 fps
unpaced, 18.1 paced; present -> on screen p50 101 to 56 ms, any input -> on screen 133 to 91. Refresh-bound (a light
drive: `--render-scale 0.25 --weather dry`, reflections, mirrors and AA off; GPU ~14 ms, 57 fps): the pacer leaves it
(the GPU idles) and frames queue: acquire wait 12.7 ms, present -> on screen 29.6, any input -> on screen 42.6;
`--max-fps 60` takes the queue out (acquire 0, 18.8, 32.0, judder p95 8.2 to 5.5 ms). Fullscreen ProMotion adapts its
refresh: unpaced frames land on the 120 Hz grid, capped ones on a 4.17 ms one (12.5 and 20.8 ms gaps), so pacing to a
fixed refresh grid does not hold there (the refresh pacing below got 37-42 ms): it is off on macOS.

Refresh pacing (`Refresh` in `pacer.rs`; Linux, fifo, Wayland's presentation feedback, while the GPU's period is 0):
frame starts locked to the refreshes they go on screen at, whole refreshes from the newest frame on screen, each
started so its CPU and GPU time (p95) ends 0.5 ms and a learned `extra` before the refresh it aims at (sway settles at
6-10 ms, its compositing deadline). A late frame behind the last skips a refresh, one late alone adds a quarter refresh
to `extra`. It stands aside while a frame's CPU or GPU p95 does not fit in a refresh (locked to every other refresh the
GPU would idle and its period never take over), and for good on a variable refresh: two frames on screen over 1 ms
from whole refreshes apart (sway's adaptive sync up to 8 ms; a fixed refresh 0.4 at most). Sway, 60 Hz, VRR off, a
light drive (`--present fifo --render-scale 0.5 --reflections off --mirrors off --anti-aliasing off --detail high
--weather dry --no-particles`, GPU ~7 ms), 3 runs of 90 s, median:

| | fps | 0.1% low | latency p50 / p95 ms | present -> on screen p50 | judder p95 / max | acquire wait |
| --- | --- | --- | --- | --- | --- | --- |
| unpaced | 59.7 | 59.7 | 58.5 / 66.0 | 33.6 | 1.40 / 2.0 | 0.03 |
| paced, before refresh pacing | 59.7 | 59.7 | 58.5 / 66.0 | 33.6 | 1.40 / 1.9 | 0.03 |
| refresh pacing | 59.6 | 57.2 | 27.8 / 35.7 | 14.5 | 1.38 / 17.9 | 0.03 |
| unpaced, `--max-fps 60` | 59.2 | 59.7 | 29.7 / 41.4 | 15.7 | 1.40 / 18.0 | 0.03 |

No frames discarded. The cost: a frame repeated every minute or two (a GPU or CPU spike past the margin, which the
unpaced queue hides). The bench scene (GPU-bound, fifo, paced, 3 x 90 s): 54.7 fps, 1% low 48.1, latency 43.9 / 58.1
ms, as before (54.8, 46.1, 44.2 / 58.2); the handovers to the GPU's period need the frame's own CPU and GPU time, not
its start to GPU done (which counts the wait behind the last: a 48-82 ms frame at each). VRR on (30 s): stands down, as
before (58.2 / 65.8 ms); without the stand-down 58.0 fps, 1% low 54.8 against 59.7 (the frames chase their own grid),
latency 21.7. Not measured: niri, a display over 60 Hz.

Judder (`RBR_LATENCY`'s "judder": how much the shown step's age on screen changes from one shown frame to the next): an
answer comes every 7 ms of game time, so drawn at its answers the car moved on by 0 to 7 ms more or less than the
screen did (up to 19 cm at 100 km/h); the pacer changed none of it (p50 2.8 ms on and off). Live play draws the car at
the physics' newest step (`playback/lead.rs`): the answer's pose moved on by its velocity and turned at its last turn
rate, at most 15 ms; the body, wheels, shadow, glass, mounted views and the chase's shift take it, the particles, tyre
marks, fences and sound keep the answers. Same scene, sway, FIFO, paced, 30 s: judder p50 / p95 / max 2.73 / 5.23 /
7.65 to 0.59 / 1.65 / 4.80 ms, the shown step 30.1 to 26.9 ms old on screen (p50).

`--frames N --profile` prints per-pass medians. Texture packs (`bin/texture-pack.rs`, never committed):
`viewer --textures <dir>/pack` reads them, the car's after its dials are made (dial faces keep theirs). The compact
`realesr-animevideov3` beats `realesrgan-x4plus` on retail liveries and signs (cleaner flat edges, 1024 livery 4.0 vs
20.7 s; RX 9060 XT, 2026-10-26); both garble non-Latin sign text. Cap 4096 (the default): Fiesta R5 28 s, 257 MB
against 79 s, 833 MB at 8192, its livery text as sharp but the smallest line.

Load-time upscaling (`upscale.rs`, ours): `viewer --upscale-textures DIR` (the launcher's "Textures: upscaled",
`upscale_textures`; DIR `<player folder>/Upscaler`, filled by the launcher's "Download upscaler…") upscales the
car's and the stage's textures with Real-ESRGAN General WDN v3 4x (halved for 2048 sources, 4096 at most),
after the car's dials are made, cached by the source file's SHA-256. Model choice (10 tried): WDN v3 (~4.4 s for a
1024 livery) crisper on lettering than UltraSharp and Nomos8kSC (both ~14 s) with the grain kept; animevideov3 and
digital-art flatten grain, digital-art and x4plus invent letters. ACCENT 7 textures 17.2 s first, 0.2 s cached;
stage objects ~25 s first; the ground's atlases 8 to 14 s more. Cached, a stage loads in under 1 s more. The
backdrop and the course tape showed no difference. Missing textures go through one run of the binary over a folder:
track-41_M with ACCENT 73.6 s before, 27.0 s (2026-10-26). Tried and dropped: shrinking 2048 sources to 1024
before the 4x (4.9 s; smears them).
Dial textures (the cockpit of a car with an RBRHUD gauge pack, `Image::Dial`) are upscaled too (user's call,
2026-11-01), from the car's own texture's size: RBRHUD's 13 bases larger than their car's (the Clio Rally3's 4096
against 2048) are smooth resizes of it, their finest octave 0.45 to 0.71 of the car's detail. The needles and
displays keep the pack's texels, scaled to the target's. Clio 4K cockpit: edges crisper, the dash's grain smoothed,
the small switch labels garbled. Reference cockpits 0.35 to 2.86 % RMSE, upscale off byte-identical; the target
gets the base's mip chain (+21 MB at 4096, the dials redrawn into every level: a Clio redraw 0.92 ms GPU against
0.60, `--profile` with the dials redrawn each frame).
A pack's base without mip levels (139 of the 150 community packs) gets a chain at load, as the car's own textures do: one level
aliased over the whole cockpit where the upscaler did not replace it (2026-11-01).
Past a face's first draw, the levels its largest region is under 8 texels in keep its last whole draw (`SMALLEST`); a
face's textures with one file and one base share a target: the Fiat 131's redraw 26 passes to 8 (dials span 0.081 to
0.024 ms, 2026-11-02). A Fiat 131 cockpit `--sequence 4` (redraws past the first) byte-identical; 6 reference shots
as before (the M3 E30 cockpit, rain and snow ones vary run to run with either build).
In a frame at least 1800 high (`--size` times the render scale) the in-car view's textures go to 8192, not 4096
(`upscale::INTERIOR_MAX`, user's call 2026-11-01): at 4K the Fiat 131 Grp4's 4096 dash is about 0.8 texels a pixel,
at 8192 its gauges and dash fabric are sharp; its 4096 pack base is upscaled 2x (4x, halved). Cached in a folder per
largest side; about 90 MB of disk per texture, a gauge cockpit's `RGBA8` target about 350 MB of GPU memory. The Fiat's
first 4K load 128 s (five 8192 interior textures).
The stage's backdrop textures (those only the sky layer draws, not the clouds) are upscaled too, to 8192 at any frame
size (`upscale::BACKDROP_MAX`, user's call 2026-11-01): the panorama strips span a whole turn (462 Odry's 4096x512,
337 Torre Vecchia's `land_mo` 4096x256), so a 4K view of them was magnified about 6x. Textures shared by the backdrop
and the ground take the ground's 4096.
Not packaged: the download takes Real-ESRGAN v0.2.5.0's release zip (macOS universal, Linux and Windows x86_64; none for ARM Linux) and the
model's ncnn files from upscayl/custom-models at 500e6a0, each checked by SHA-256.

## Lighting sheet

`scripts/light-sheet.py --a VIEWER [--b VIEWER] --out DIR [--quick]` (documents its views, hours, weathers and
summary) renders seven fixed views at 1920x1080 with `--enhance all` and writes one sheet per view and `summary.txt`
(mean, maximum and share over 8 and 32 of 255 of the a/b difference). The same binary as a and b differs on under
0.01% of pixels (a few up to 50 of 255: shadow and particle noise); judge by the mean and the shares, not the
maximum. Quick with two binaries: about 2.5 to 4 minutes.

## Open

- Nordschleife (2026-11-10): broad dry-terrain lighting/shadow changes matching the reported symptom are not yet
  isolated. The probe transitions and missing cloud shadows are fixed above; dry diffuse terrain does not read the
  probes, and the sun stays fixed during a drive (`relight::Clock::atmosphere`).
- Sky dome and cloud transforms and inputs (`SkyDome.vso` `c41`-`c66`); stage in-scatter not reproduced.
- Cut-out backdrop strips with black transparent texels (Odry's panoramas) show a dark rim along the ridge: the
  alpha cut falls where the filtered colour is half black (462 `HOR_1`, `HOR_2`: transparent texels (0, 0, 0)). A fix
  bleeds the colour into them at load (the textures upload as BCn, so a decode and re-encode) or reads such textures
  as premultiplied; either changes every cut-out with black transparent texels, trees included.
  Cut-out textures (2026-11-11, `examples/cutout_survey.rs`, by triangles drawn): most are DXT3 or DXT5 with a
  smooth alpha (7 to 17% of texels neither 0 nor 255); the clear texels are black on 0 to 2% of 621's and 532's,
  10 to 19% of 450's and 580's, 25% of Semetin's, 38 to 53% of Odry's, 25 to 40% of retail 41's and 52's DXT3.
  The stages' DXT1 cut-outs have no clear texels (retail's 15 to 26%, all black).
- Stage `c90`; kind-byte blend bits; reflection items `(1,7)`; water texture.
- Wheel set shown; `Plane_*` shadows; detached node motion; `pc_e_window_*` panels.
