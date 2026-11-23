# Screens: ultrawide, curved and triple

ReRBR can draw the stage at its true size for the screen in front of you. Tell the launcher how big the screen is and
how far your eyes are from it, and it works out the field of view that makes a corner 50 m ahead look 50 m ahead. The
settings are in the Screen section of Settings > Graphics & sound:

1. Pick Single, Curved or Triple.
2. Enter the screen's size and your distance to it: the curve's radius for Curved, the bezels and side angle for
   Triple.
3. Leave Field of view on "Match my screen".

Measure the distance sitting as you drive, from your eyes to the middle of the screen (the centre screen for Triple).
A tape measure from the bridge of your nose is close enough. Sit with the middle of the screen at eye level if you can:
the view assumes it, as iRacing's and Le Mans Ultimate's setups do.

Nobody has tried Curved on a real curved monitor yet, or Triple on a real triple rig. The geometry is tested in code
and in screenshots. If you have one of these setups, please [report](https://github.com/JodusNodus/rerbr/issues/new/choose)
how it looks.

## Field of view

The true field of view is the angle your screen covers from where you sit. At that setting the world on screen is
the size it would be through a window. A wider setting shows more but makes everything smaller and further away. A
narrower one zooms in.

Field of view in the Screen section has two settings:

- **Match my screen.** The in-car views use the true field of view, worked out again whenever you change the size,
  radius or distance. The Camera section's slider shows the value greyed out.
- **Manual.** The Camera section's slider sets it, 50° by default. The Screen section still shows your screen's true
  field of view next to it.

Picking Curved or Triple turns on Match my screen. Single starts on Manual, so nothing changes for a player who never
opens the section. Changing the field of view on the pause menu's Camera page while driving switches to Manual and
keeps the value you chose.

The chase view keeps its own slider, 65° by default, because a camera behind the car has no true size to match.

## Single screen

This covers any one flat monitor, ultrawides included. Choose Match my screen, then enter its diagonal and your
distance from it. The field of view covers the whole width.

A flat ultrawide is where this matters most. At 50° a 49" 32:9 panel at 70 cm looks zoomed in, because the panel
covers about 81° of your view. Some examples at 70 cm:

| Screen | True field of view |
| --- | --- |
| 27" 16:9 | 46° |
| 34" 21:9 (3440x1440) | 59° |
| 49" 32:9 | 81° |

## Curved screen

Choose Curved and enter three numbers:

- the diagonal, in inches;
- the curve's radius in millimetres. Makers write it as a number and R, so 1800R is 1800 mm;
- the distance from your eyes to the middle of the panel, in centimetres.

A flat image on a curved panel bends straight lines and stretches the edges. ReRBR draws the panel as three flat
segments that follow its curve, each with its own view from your eye. The road's edge stays straight within each
segment, and the segments meet without a gap.

In this mode the field of view sliders and the Camera page set the angle of the whole panel, edge to edge. The true
field of view is wider than the same panel flat, because the curve brings the edges towards you:

| Screen at 70 cm | Flat | Curved |
| --- | --- | --- |
| 34" 21:9, 1800R | 59° | 62° |
| 49" 32:9, 1800R | 81° | 89° |
| 49" 32:9, 1000R | 81° | 94° |

The HUD, the menus and the loading screen sit in the middle of the panel, at most 16:9 wide.

Three flat segments are an approximation of the curve. At 1800R the picture is about 0.3° off at the middle of each
segment and exact where they meet. Each segment is also about 0.2 % too short (0.7 % at 1000R). Neither has been
checked on a real panel.

From the command line the viewer takes `--curved DIAGONAL,RADIUS,DISTANCE`, for example `--curved 49,1800,70`.

## Triple screens

Choose Triple for three like monitors side by side, the outer ones turned towards you. Enter:

- each screen's diagonal;
- the bezel, the frame around each screen's image in millimetres. Two bezels sit between neighbouring images, and
  the picture continues behind them as it would behind a window frame;
- the distance from your eyes to the centre screen;
- the side angle, how far the outer screens are turned in. Auto turns them to face your eyes.

Then pick how the screens are connected. "One wide display" is a single spanned desktop, such as NVIDIA Surround or AMD
Eyefinity: ReRBR splits one window into thirds. "Three monitors" opens a borderless fullscreen window on each monitor
you name, and needs fullscreen on.

In triple mode the field of view sliders set the centre screen's angle, and the side screens follow from the angle
and bezels. The HUD and menus go on the centre screen. From the command line: `--triple DIAGONAL,BEZEL,DISTANCE[,ANGLE]`
and `--triple-monitors LEFT,CENTRE,RIGHT`.

If a settings file turns on both Curved and Triple, Triple wins.

## Limits

- Some effects are worked out for each view on its own: the glare around bright light, and the local exposure that
  follows it. Where two views meet they can differ a little. In test shots the step was 1 to 4 levels of 255 on
  average, larger at the edge of a bright windscreen, plus a line two pixels wide about 2 levels darker. With bezels
  between the screens this hides behind the frames; on a curved panel it can show.
- The exposure follows the centre view.
- Three views cost more than one, mostly on the CPU. On a Radeon RX 9060 XT at 5120x1440, in the benchmark's worst
  case (heavy rain on Gabiria from the cockpit), Curved averaged 44 frames a second against 58 for a single view,
  with 1 % lows of 21 against 52.
- Replays exported as video stay a single view.
