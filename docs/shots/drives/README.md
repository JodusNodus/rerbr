# Shots from drives

Frames from replays of real drives (an Audi quattro A2 Group B at 05:30, the drive's own sky), not from the
autopilot list of [`data/shots.json`](../../../data/shots.json), so `scripts/render-shots.py` does not make them. Each
replay was rendered as a 1080p video at 30 frames a second with every enhancement on and no HUD
(`viewer --replay FILE --video OUT --video-fps 30 --no-sound --no-hud --enhance all,grass --detail ultra
--anti-aliasing taa --reflections high`, plus `--view chase`, `--view cockpit --fov 65`, or `--view chase --fov 45
--flyby 9,2.5,0.7` for the low camera), and the frames picked by hand (the video encoded at x264 CRF 12, 4:4:4, through an `ffmpeg` wrapper on `PATH`, rather than the viewer's CRF 18, 4:2:0). The replays stay out of the repository.

## El Condor (RSF 564)

### condor-barrier-chase

chase view

![condor-barrier-chase](condor-barrier-chase.jpg)

### condor-boulders-chase

chase view

![condor-boulders-chase](condor-boulders-chase.jpg)

### condor-boulders-flyby

low chase (`--flyby`) view

![condor-boulders-flyby](condor-boulders-flyby.jpg)

### condor-crowd-chase

chase view

![condor-crowd-chase](condor-crowd-chase.jpg)

### condor-helicopter-chase

chase view

![condor-helicopter-chase](condor-helicopter-chase.jpg)

### condor-helicopter-cockpit

cockpit view

![condor-helicopter-cockpit](condor-helicopter-cockpit.jpg)

### condor-pampas-flyby

low chase (`--flyby`) view

![condor-pampas-flyby](condor-pampas-flyby.jpg)

### condor-sideways-flyby

low chase (`--flyby`) view

![condor-sideways-flyby](condor-sideways-flyby.jpg)

### condor-start-chase

chase view

![condor-start-chase](condor-start-chase.jpg)

### condor-start-cockpit

cockpit view

![condor-start-cockpit](condor-start-cockpit.jpg)

### condor-tape-flyby

low chase (`--flyby`) view

![condor-tape-flyby](condor-tape-flyby.jpg)

## Pengonpohja (RSF 607)

### pengonpohja-fans-flyby

low chase (`--flyby`) view

![pengonpohja-fans-flyby](pengonpohja-fans-flyby.jpg)

### pengonpohja-fields-flyby

low chase (`--flyby`) view

![pengonpohja-fields-flyby](pengonpohja-fields-flyby.jpg)

### pengonpohja-junction-cockpit

cockpit view

![pengonpohja-junction-cockpit](pengonpohja-junction-cockpit.jpg)

### pengonpohja-junction-flyby

low chase (`--flyby`) view

![pengonpohja-junction-flyby](pengonpohja-junction-flyby.jpg)

### pengonpohja-lupins-flyby

low chase (`--flyby`) view

![pengonpohja-lupins-flyby](pengonpohja-lupins-flyby.jpg)

### pengonpohja-slide-flyby

low chase (`--flyby`) view

![pengonpohja-slide-flyby](pengonpohja-slide-flyby.jpg)

## Pirttikulma (RSF 590)

### pirttikulma-barn-cockpit

cockpit view

![pirttikulma-barn-cockpit](pirttikulma-barn-cockpit.jpg)

### pirttikulma-farmhouse-flyby

low chase (`--flyby`) view

![pirttikulma-farmhouse-flyby](pirttikulma-farmhouse-flyby.jpg)

### pirttikulma-slide-flyby

low chase (`--flyby`) view

![pirttikulma-slide-flyby](pirttikulma-slide-flyby.jpg)

### pirttikulma-spectators-flyby

low chase (`--flyby`) view

![pirttikulma-spectators-flyby](pirttikulma-spectators-flyby.jpg)

