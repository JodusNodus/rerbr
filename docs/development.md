# Development

How to build ReRBR, run it, find your way around the code and check a change. Each area has a note in
[`notes/`](../notes/) with the detail, and [status.md](../notes/status.md) lists the open work.

## What ReRBR is

A Rust program that plays Richard Burns Rally with NGP 7.4.778 physics on the player's own game files: the retail
stages and cars, and community cars and stages, read in place. No original code runs. ReRBR reads the game
data, the 1.02 exe's data sections and the tables of the player's `NGP.dll` as files.

The physics and the gameplay (race flow, timing, weather, damage, service, season, saves) must give the same results
as the original game, bit for bit. What the player sees and hears is new: the renderer, sound, HUD, cameras, and the
launcher in place of the game's menus. Scope and milestones: [rust-plan.md](../notes/rust-plan.md).

## Set up

You need Rust through rustup (`rust-toolchain.toml` pins the version CI uses), SDL3 and `pkg-config`, plus the ALSA
and udev development packages on Linux ([README](../README.md#build-from-source) names them). On Windows 10 or 11
(x86_64), use rustup's MSVC toolchain with the Visual Studio Build Tools ("Desktop development with C++") and cmake
in place of SDL3 and `pkg-config`, and add `--features rbr-render/sdl-static` to every cargo build, test and clippy.
That builds SDL3 from source, as CI does.

```sh
git clone https://github.com/JodusNodus/rerbr.git && cd rerbr
cargo test --workspace
python3 -m venv .venv && .venv/bin/pip install unicorn numpy   # only for the harness tools
```

The tests need nothing else. The first run builds everything (the tests at `opt-level = 3`). A test that needs game
data or recordings prints "skipping" and passes when its variable is unset.

### Test data

Copy `dev.env.example` to `dev.env` (worktrees read the main checkout's) and fill in what you have.
`scripts/rust-check.sh` names the variables that are unset. `RBR_DATA`, `RBR_PATCH` and `RBR_RECORDINGS` cover most
tests.

| Variable | What | Where from |
| --- | --- | --- |
| `RBR_DATA` | the retail 1.0 install, one folder per installer group (`Root/`, `Cars/`, `Maps/`, ...) | your disc (below): `OUT/retail-install` |
| `RBR_PATCH` | the official 1.02 patch's `Main_group` (tests, the hybrid check and `scripts/gen-exe-data.py`; the program does not need it) | your disc and patch (below): `OUT/Main_group` |
| `RBR_RECORDINGS` | drives recorded in the original game (`drive-*.bin`) | `scripts/fetch-test-data.sh` (below) |
| `RBR_VECTOR_STORE` | per-call physics records of the original | the same script |
| `RBR_NGP_PHYSICS` | an NGP 7.4.778 `physics.rbz` with NGP cars in slots 2, 4, 5 and 6 | below |
| `RBR_NGP_DLL` | NGP 7.4.778's `Plugins/NGP.dll`, in place of the install's | the NGP package; the reader checks its SHA-256 |
| `RBR_NGP` | a folder with `7.4.778/physics.rbz` and NGP car `.lsp` files under `cars-rsf/` | the NGP package and the car packs |
| `RBR_RSF` | the game folder in use when set (an unsaved developer override pairing the game data with sample community packs) | your own install |
| `RBR_SAVES` | the `SavedGames` files 1.02 wrote: `pf<name>.rbr`, `.acm`, `pfresult.rst` | play 1.02 once (a new profile, no records) and copy the folder |

**From your disc.** `DISC` is the retail disc's `.iso` or folder, `PATCH` the official 1.02 patch (`RBR_v102.zip` or
the patch `.exe`). `OUT` must be new or empty and takes 2.8 GB.

```sh
cargo run --release -p rbr-launcher -- --extract-test-data DISC PATCH OUT
export RBR_DATA=OUT/retail-install RBR_PATCH=OUT/Main_group
```

`cd OUT/retail-install && shasum -a 256 -c <repo>/original/retail-1.0/install.SHA256SUMS` checks the install.

**NGP.** Only the 7.4.778 `NGP.dll` is supported (`ngp-compat` checks another one against it:
[ngp.md](../notes/ngp.md#adding-an-ngp-version)). Take `physics.rbz` from the NGP package (`RBR_NGP-7.4.778.507.7z`)
and put four NGP cars in it, each from its gvrc.de physics pack (`<Car> - NGP
Physics.zip`, unzipped; an install with community packs has the same folders under `rsfdata/cars/`):

```sh
cp <NGP package>/physics.rbz ngp-physics.rbz
python3 -I scripts/ngp-install-car.py ngp-physics.rbz mg_zr    "BMW_M3_E30_GrpA_ngp6"                         # slot 2
python3 -I scripts/ngp-install-car.py ngp-physics.rbz p_206    "Ford_Fiesta_Rally4_ngp6"                      # slot 4
python3 -I scripts/ngp-install-car.py ngp-physics.rbz s_i2003  "Ford_Fiesta_Rally3_ngp6"                      # slot 5
python3 -I scripts/ngp-install-car.py ngp-physics.rbz t_coroll "Toyota_Celica_2000GT-FOUR_RC_ST185_GrpA_ngp6" # slot 6
export RBR_NGP_PHYSICS=$PWD/ngp-physics.rbz
```

**Recordings and vectors.** They are release assets; `data/test-data.SHA256SUMS` names the release and checks the
download (about 345 MB, 5.5 GB unpacked):

```sh
eval "$(scripts/fetch-test-data.sh ~/rerbr-test-data)"   # sets RBR_RECORDINGS, RBR_VECTOR_STORE
```

## Running it

`cargo build --release && target/release/rerbr` builds the three programs a player needs and starts the launcher:
`rerbr` (the launcher), `viewer` (the driving window) and `rbr-standalone` (the physics). The launcher starts the
other two as child processes. `cargo build --workspace` builds every tool as well. Each program has `--help`.

`scripts/launcher.sh` builds them and starts the launcher with the overrides below, taken from the main checkout's
`original/` folder where it has them. Without overrides the launcher reads the game folder chosen on its Settings
page.

| Override | Replaces |
| --- | --- |
| `RBR_DATA`, or `rerbr --data DIR` | the game data (the extracted retail install works too) |
| `RBR_PATCH` | the 1.02 files |
| Settings field `ngp_dll` (unsaved: `RBR_NGP_DLL`) | NGP's DLL. Empty, it follows the game folder's `Plugins/NGP.dll`; a missing or unsupported DLL blocks Start. With this and `ngp_physics` empty, a downloaded copy in the player's folder (`NGP 7.4.778/`) is used |
| Settings field `ngp_physics` (unsaved: `RBR_NGP_PHYSICS`) | NGP's archive |
| `RBR_RSF` (unsaved, never saved) | the game folder in use when set, for pairing the game data with sample community packs |

On Windows the launcher and the viewer open no console window. Started from Explorer or `cmd`, they write stderr to
`rerbr.log` in the player's files folder, and stdout shows only when redirected (`rerbr.exe --list > list.txt`).

## Code map

One Cargo workspace, each crate in `crates/<name>/`.

| Crate | What | Note |
| --- | --- | --- |
| [`rbr-launcher`](../crates/rbr-launcher) | the launcher `rerbr` (egui): game folder, stage and car pickers, settings, championship, replays | [cameras-hud-launcher.md](../notes/cameras-hud-launcher.md) |
| [`rbr-render`](../crates/rbr-render) | the wgpu renderer and the driving window `viewer`: stage, car, sky, weather, HUD, cameras, devices, force feedback | [renderer.md](../notes/renderer.md), [cameras-hud-launcher.md](../notes/cameras-hud-launcher.md) |
| [`rbr-sound`](../crates/rbr-sound) | gameplay sound | [sound.md](../notes/sound.md) |
| [`rbr-gameplay`](../crates/rbr-gameplay) | the gameplay loop: input to car control, frame clock, stage states, timing, race flow, championship, service park, saves | [gameplay.md](../notes/gameplay.md) |
| [`rbr-formats`](../crates/rbr-formats) | readers for the game's files and the community layouts, one module per format | [formats.md](../notes/formats.md) |
| [`rbr-btb`](../crates/rbr-btb) | BTB stages (experimental): their folders, meshes, the stage files built from them, and their scene | [btb.md](../notes/btb.md) |
| [`rbr-standalone`](../crates/rbr-standalone) | the physics process: `--live` steps the car for the viewer, `--boot` checks a recording | [physics.md](../notes/physics.md) |
| [`rbr-physics`](../crates/rbr-physics) | NGP 7.4.778's physics; `ports.csv` lists the ported functions | [physics.md](../notes/physics.md), [ngp.md](../notes/ngp.md) |
| [`rbr-hybrid`](../crates/rbr-hybrid) | the pure step, and the bindings that run the ports inside the original's step (`bound.csv`) | [verification.md](../notes/verification.md) |
| [`rbr-recording`](../crates/rbr-recording) | readers for the recordings, live records and replays | [verification.md](../notes/verification.md) |
| [`rbr-counters`](../crates/rbr-counters) | instruction and allocation counts per step and frame | [verification.md](../notes/verification.md#measuring-cost) |

| Path | What |
| --- | --- |
| `scripts/` | `rust-check.sh`, `fetch-test-data.sh`, `launcher.sh`, `render-shots.py`, `bench.py`, `package.sh`, generators of Rust tables (`gen-*.py`), NGP tools |
| `harness/` | records and replays the original game for the physics checks ([README](../harness/README.md)) |
| `data/` | test data hashes and file lists, the reference shot list, generator inputs |
| `docs/` | this guide, the install guide, the reference shots and clips |
| `original/` | original binaries and game data, gitignored apart from their hashes |
| `build/`, `target/`, `dist/` | local caches, the build, the packages; gitignored |

## Rules

- Physics and gameplay stay bit-exact against the original's recordings. Where the original and a nicer formula
  disagree, the original wins.
- Presentation (rendering, sound, HUD, launcher) never changes the physics. It reads the physics memory after a step,
  or copies a payload at an existing host sink, without writing to it.
- One module or crate per system. Shared files (`renderer.rs`, `viewer.rs`, both `live.rs`, `lib.rs`, `Cargo.toml`,
  the README tables) get small hooks only.
- File readers go in `rbr-formats`, one module per format, with a test over every retail file they cover.
- Physics code names physical quantities; addresses and the original's names stay in `layout` code, the bindings and
  the CSVs ([rbr-physics README](../crates/rbr-physics/README.md#code-style)).
- Never commit game files: no binaries, game data, NGP or community content, or anything extracted from them.
- Portable code (CI builds on macOS, Linux and Windows); English only.

## Checks

Before a merge, from the repository root:

```sh
scripts/rust-check.sh
```

It runs `cargo fmt --check`, clippy with `-D warnings`, the tests and the physics vectors for the crates the change
touches and the crates that depend on them, then prints PASS or FAIL. `RUST_CHECK_ALL=1` checks every crate.
`RUST_CHECK_VECTORS=none` skips the vectors, as does an unset `RBR_VECTOR_STORE`.

CI (`.github/workflows/ci.yml`) runs fmt, clippy and the tests on macOS, Ubuntu and Windows, and on macOS (arm64) the
published recordings and vectors as well. The retail files are not published, so CI skips the tests that need them;
run those locally. The vector targets hold on an arm64 host only ([physics.md](../notes/physics.md), "Default NaN
sign").

Some changes need more:

| Change | Also |
| --- | --- |
| renderer, HUD, cameras, launcher | before and after captures (`viewer --screenshot`, `rerbr --screenshot DIR`); for the renderer the [reference shots](#reference-shots) |
| sound | a short audio render |
| a file format | a test that parses every retail file it covers |
| `rbr-standalone` or `rbr-hybrid` | `.venv/bin/python -I -B harness/rsf_compare.py $RBR_RECORDINGS/rsf/drive2 --attempt 5 --sections physics --out DIR` gives physics PASS 84084/84084 and exit status 0 (release `rbr-standalone` and `rsf-settings`; `RBR_NGP_PHYSICS` and `RBR_RSF` set) |
| physics | the hybrid check and vectors ([Physics changes](#physics-changes)) |

## Physics changes

The original game is the oracle. A plugin in the original (`harness/plugin/`) records every physics step of a drive
and dumps the game's memory at chosen steps. Three checks compare the Rust physics with those records:

- `rbr-standalone --boot` loads a recorded drive's stage and car from the game files, runs the pure Rust step and
  compares the car and wheels after every step;
- the hybrid check (`harness/rust_step.py`) runs one step of the original from a snapshot in an emulator, once as is
  and once with the Rust ports bound in, and every "run B vs run A" line must read equal;
- the vectors replay fuzzed per-call records of branches no drive reaches, one test per module.

A short list of documented differences is allowed (the default NaN sign, stale stack words the original never reads,
heap addresses, frame-loop words). [verification.md](../notes/verification.md) has the commands and the masks,
[physics.md](../notes/physics.md) the float rules.

The hybrid check reads the snapshots and the x87 cache, about 155 MB more (9.2 GB unpacked):

```sh
eval "$(scripts/fetch-test-data.sh ~/rerbr-test-data snapshots)"   # sets RBR_SNAPS, RBR_REPLAY_FPU_CACHE
```

The x87 cache holds the CPU's results for the transcendentals (`fsin`, `fcos`, ...), measured on the machine the
recordings ran on. A run that needs a case the cache lacks reports those steps UNVERIFIED; name them in the pull
request and the maintainers measure and publish them.

The maintainers make new recordings and snapshots, running the original under Wine
([verification.md](../notes/verification.md#dev-setup)), and pack a release set with `scripts/pack-test-data.sh`.
NGP's snapshots hold NGP's code and are not published.

The C++98 reconstruction of the 1.02 binaries, used for the ports, lives in the frozen rbr-recomp repository with
every note as of the split under the same paths. Paths under `src/`, `symbols/` and `types/` in comments refer to it.

## Reference shots

`scripts/render-shots.py` renders the 36 shots of `data/shots.json` at 1080p with every enhancement on. They are the
README pictures and the reference for graphics changes. The script needs release builds of `rerbr`, `viewer` and
`rbr-standalone`, an install with NGP and the community stages and cars the shots name, and the upscaler for the
same textures. Its docstring lists the options.

For a graphics change:

1. Build from main and render a baseline: `scripts/render-shots.py --out BASE`.
2. Make the change, rebuild and render again: `--out NEW` (`--only NAME...` for a subset).
3. `--out NEW --compare BASE --diff DIFFS` prints each shot's RMSE and writes a diff image for each changed one.
4. When the change is wanted, update the committed JPEGs: `--out NEW --no-render --previews docs/shots --gallery
   docs/shots/README.md`.

Compare renders made on one machine. GPUs and C libraries round differently: a Mac and an AMD desktop differ by 2-3
of 255 on average, at foliage edges and dust. The PNGs are game content and stay out of git; `data/shots.json` and
the JPEGs in [`docs/shots/`](shots/README.md) are committed.

A README clip comes from a replay: `viewer --replay FILE --video clip.webp --video-span 290,297 --video-fps 25 --size
1280x720 --no-hud --view chase --enhance all,grass --detail ultra`. A `.webp` path writes an animated WebP, and
`--video-span FROM,TO` encodes only those game seconds, without sound.

## Benchmark

`scripts/bench.py` measures the frame rate of the worst case we know: the autopilot drives Gabiria in heavy rain from
the cockpit of the Fiat 131, fullscreen, every graphics option at its most and the frame rate uncapped, and the
viewer's performance log times 90 s from the go. It prints average fps, the 1% and 0.1% lows, frame time percentiles,
hitches and the costliest GPU passes and CPU spans, the median of 3 runs. The same release builds and install as the
reference shots, and a display; the window takes the screen for a few minutes. `--out NEW --compare BASE` compares two
results. The scene's choice and a baseline are in [renderer.md](../notes/renderer.md#benchmark-2026-11-02).

## Packaging

`scripts/package.sh` builds the players' download into `dist/` with `cargo build --profile play`. The version is the
launcher's (`crates/rbr-launcher`). SDL3 is built from source and linked in (`rbr-render` feature `sdl-static`, needs
cmake), so players install nothing else. `.github/workflows/release.yml` runs it on a pushed `v*` tag and attaches
the packages to a draft release; nightly on main and by hand (Actions, "Run workflow") it keeps them as the run's
artifacts only.

- macOS: `ReRBR-<version>-macos.dmg` with `ReRBR.app`, for Apple silicon and macOS 11 or later. It is ad-hoc signed,
  not notarised.
- Linux: `ReRBR-<version>-<arch>.AppImage` (needs the ALSA and udev headers). Build it on an old distribution to keep
  the glibc floor low, for example:

  ```sh
  docker run --rm -v "$PWD":/src -w /src -e CARGO_TARGET_DIR=/target rust:1-bookworm bash -c \
    'apt-get update && apt-get install -y cmake libasound2-dev libudev-dev && scripts/package.sh'
  ```
- Windows: `ReRBR-<version>-windows-x86_64.zip`. Run the script in Git Bash with the MSVC toolchain and cmake. The C
  runtime is linked in, so players need no Visual C++ redistributable. Microsoft's DXC (`dxcompiler.dll`, pinned
  release, its licences beside it) goes in next to the programs: wgpu compiles the shaders with it in place of FXC
  ([renderer.md](../notes/renderer.md#decisions)). The zip is unsigned; the README says how to get past SmartScreen.

The icons are in `scripts/packaging/`. The player's files go to the platform's data folder
(`crates/rbr-launcher/src/home.rs`, [README](../README.md#your-files)), since an app bundle is read-only.
