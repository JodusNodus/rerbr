# Screens: ultrawide, curved and triple

ReRBR can draw the stage at its true size for the screen in front of you. Tell the launcher how big the screen is and
how far your eyes are from it, and it works out the field of view that makes a corner 50 m ahead look 50 m ahead. The
settings are on the Graphics & sound page, under Display > Screens, which offers Single, Curved and Triple.

Nobody has tried Curved on a real curved monitor yet, or Triple on a real triple rig. The geometry is tested in code
and in screenshots. If you have one of these setups, please [report](https://github.com/JodusNodus/rerbr/issues/new/choose)
how it looks.

## Field of view

The field of view sliders (Graphics & sound > Camera, and the pause menu's Camera page) set the horizontal angle the
image covers. The default is 50° for the in-car views and 65° for the chase view.

The true field of view is the angle your screen covers from where you sit. At that setting the world on screen is
the size it would be through a window. A wider setting shows more but makes everything smaller and further away. A
narrower one zooms in. Each Screens mode shows the true field of view for what you entered, and "Use for in-car views"
copies it to the in-car slider. The chase view keeps its own slider, because a camera behind the car has no true size
to match.

## Single screen

This covers any one flat monitor, ultrawides included. Enter its diagonal and your distance from it. The readout is
the true field of view across the whole width. Nothing else changes: the default stays 50°.

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
