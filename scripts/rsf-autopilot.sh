#!/bin/bash
# One driverless RSF drive (notes/verification.md "RSF sessions"): RSF car PACK in slot 5, the stage started by the
# harness (rsf-session.sh AUTO, IRBRGame::StartGame), driven by PLAYBACK's controls (an FRM1; the game quits 2 s after
# they end), recorded as session NAME and fetched to $RBR_RECORDINGS/rsf/NAME.
#
#   scripts/rsf-autopilot.sh NAME PACK MAP WEATHER TYRE DAMAGE PLAYBACK
#
# PACK: a folder of $RBR_RSF/rsfdata/cars, or - for the car slot 5 holds. WEATHER 0 good, 2 bad; TYRE 0 dry tarmac,
# 2 wet tarmac, 3 dry gravel, 4 intermediate gravel, 6 snow; DAMAGE 0 off .. 3 realistic, - for the game's. MAP a 1.02
# stage: RSF's own stages do not start this way (the main loop hangs). The install's slot 5 (Cars.ini [Car05] and
# Physics/s_i2003, where RSF copies the chosen car) is backed up and restored after the run. Needs the plugin installed
# (scripts/rsf-session.sh install) and ~/.local/bin/rbr (or RBR_LAUNCH) starting the game: `$RBR_LAUNCH game`.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/dev-env.sh"
[ $# = 7 ] || { sed -n '2,13p' "$0" >&2; exit 1; }
name=$1 pack=$2 map=$3 weather=$4 tyre=$5 damage=$6 playback=$7
root=$(cd "$(dirname "$0")/.." && pwd)
game=${RBR_RSF:?set RBR_RSF (dev.env)}
runs=${RBR_RSF_RUNS:?set RBR_RSF_RUNS (dev.env)}
launch=${RBR_LAUNCH:-$HOME/.local/bin/rbr}
[[ "$pack" =~ ^([A-Za-z0-9_.-]+|-)$ && "$damage" =~ ^([0-3]|-)$ ]] || { echo "bad PACK or DAMAGE" >&2; exit 1; }
[ "$pack" = - ] || [ -d "$game/rsfdata/cars/$pack" ] || { echo "no RSF car $pack" >&2; exit 1; }

ini=$game/Cars/Cars.ini slot=$game/Physics/s_i2003
[ ! -e "$ini.autopilot" ] && [ ! -e "$slot.autopilot" ] ||
  { echo "an earlier run's backup is left: $ini.autopilot, $slot.autopilot" >&2; exit 1; }
cp -p "$ini" "$ini.autopilot"
cp -a "$slot" "$slot.autopilot"
trap 'cp -p "$ini.autopilot" "$ini" && rm "$ini.autopilot" && rm -rf "$slot" && mv "$slot.autopilot" "$slot"' EXIT
if [ "$pack" != - ]; then
  rm -rf "$slot" && cp -a "$game/rsfdata/cars/$pack" "$slot"
  # [Car05] as RSF writes it: the car's model from RSF's car and model lists
  python3 -I - "$game" "$pack" <<'EOF'
import json, re, sys
game, pack = sys.argv[1:]
load = lambda n: json.load(open(f"{game}/rsfdata/cache/{n}.json", encoding="utf-8-sig"))
car = next(c for c in load("cars") if c["path"] == pack)
model = next(m for m in load("carmodels") if m["id"] == car["carmodel_id"])
d = "Cars\\" + model["path"] + "\\" + model["filename"]
block = (f"[Car05]\r\nCarName = {car['name']}\r\nFileName = \"{d}.sgc\"\r\nIniFile = \"{d}.ini\"\r\n"
         f"RSFCarID = {car['id']}\r\nRSFCarPhysics = \"rsfdata\\cars\\{pack}\"\r\nShaderFile = \"{d}_shaders.ini\"\r\n"
         f"ShaderSettings = \"{d}_shader_settings\"\r\nTexturePath = \"Cars\\{model['path']}\\Textures\\\"\r\n\r\n")
path = f"{game}/Cars/Cars.ini"
text = open(path, encoding="latin-1", newline="").read()
text, n = re.subn(r"\[Car05\].*?(?=\[Car06\])", lambda _: block, text, flags=re.S)
if n != 1:
    sys.exit("Cars.ini: no [Car05] before [Car06]")
open(path, "w", encoding="latin-1", newline="").write(text)
print(f"slot 5: {car['name']} (RSF car {car['id']})")
EOF
fi
auto="$map 5 $weather $tyre"
[ "$damage" = - ] || auto+=" $damage"
AUTO=$auto PLAYBACK=$playback "$root/scripts/rsf-session.sh" start "$name"
setsid "$launch" game >"$runs/$name.game.log" 2>&1 </dev/null &
until [ -e "$runs/$name/.done" ]; do sleep 5; done  # the session's watcher ends it once the game quits
cat "$runs/$name/.done"
"$root/scripts/rsf-session.sh" fetch "$name"
