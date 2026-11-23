# Force feedback from the tyres

By default ReRBR sends the wheel NGP 7's force. NGP adds up the torque the car's suspension and tyres put on the
steering rack, smooths it, and puts it through a curve that flattens it towards full force. The launcher's Settings >
Experimental "Force feedback: From the tyres" switch builds the force the way Assetto Corsa EVO does. It changes only
what the wheel feels. The car drives the same, and records, ghosts and championship results are kept.

You can also switch while driving: the pause menu's Force feedback page has Force (NGP 7 or From the tyres) and, with
From the tyres, Steering levers and Road texture. The change starts when you resume, so you can compare them on the
same corner, and it is kept for the next drive.

## What it does

Each front tyre on the ground pushes sideways on the road. That push acts a little behind the centre of the tyre, at a
distance called the pneumatic trail. The steering geometry adds a fixed 25 mm on top. The force on the wheel is the
sideways push times that lever, added up over both front tyres and sent in a straight line. Twice the torque gives
twice the force, until the wheel reaches full force.

The pneumatic trail is longest while the tyre grips and shrinks as it starts to slide. So the wheel gets heavier as
you corner harder, peaks a little before the front tyres run out of grip, then goes light as the front washes out. I
find that lightening the most useful part, because it tells you about understeer before the car does.

## Compared with NGP's force

| | NGP's force | From the tyres |
|---|---|---|
| Near the front's grip limit | keeps building on some cars, the Alpine A110 RGT up to 10° of slip | peaks at 4-6° of slip, then goes light |
| Torque steer and the pull under braking | felt | absent |
| Bumps through the suspension | felt | felt only when they change the tyres' grip, or with Road texture |
| Mapping | NGP's curve | a straight line |
| At a standstill | a small centring force | none |

The same sensitivity gives each car a different weight. On a 40 m circle the Fiesta Rally2 was 0.73 times as heavy as
with NGP's force, and the A110 RGT 1.15 times. Set the car's sensitivity again after switching, on the Controls page or
in the pause menu while driving. Both forces use the same sensitivity setting, so a change applies to both.

## Car's own steering levers (a trial)

The "Force feedback: Car's own steering levers" switch replaces the fixed 25 mm with each front wheel's own lever, the
caster trail NGP's steering geometry gives it at that moment. It changes with the suspension's travel and the steering
angle, and differs from car to car: from 15 mm (Trabant) to 43 mm (Punto S2000), 24 mm for the median car. Near
straight ahead most cars feel as with 25 mm. The Rally2, S2000 and kit cars, at about 40 mm, are about a quarter
heavier (set the sensitivity again). Every car goes lighter as you turn the wheel further: at half lock about 0.7-0.9
of the fixed lever's force, at full lock in a hairpin about half. The front's lightening at the grip limit stays. It
does nothing unless "From the tyres" is on.

## Road texture (a trial)

The "Force feedback: Road texture" switch adds the bumps the front wheels ride over to the force from the tyres. They
come from the physics: NGP's own steering torque has them, in the wheels' braking, drive and vertical forces about the
steering axis, which the force from the tyres leaves out. Only the 5-30 Hz part is added, so there is no steady pull
and no fine buzz, and it is capped, so hitting a rock kicks the wheel without slamming it to full force.

Gravel and grass are about three times as rough as tarmac and snow, so the texture also tells you when a front wheel
leaves the tarmac or drops onto the verge, where it has about half the grip. It does nothing unless "From the tyres" is
on. If the wheel's smoothing setting is high, it smooths the texture away too.

## Why 25 mm

I recorded Assetto Corsa EVO's force from its telemetry in a GT4, an MX-5 and an A110 S. It matches the front tyres'
aligning moment plus their sideways force times 36-45 mm, sent in a straight line. ReRBR's tyres have a shorter
pneumatic trail than AC EVO's, so 40 mm here leaves too little lightening. With 25 mm the torque peaks at 4-6° of front
slip and keeps 0.69-0.86 of its peak at 10°, close to AC EVO's. It is also the typical NGP car's own: over 102
cars, NGP's steering geometry gives a median of 24 mm, from 15 mm (Trabant) to 43 mm (Punto S2000); the Rally2 cars
have 38-43 mm.

When the front wheels lock under braking, their sideways force collapses and the wheel goes light: on tarmac to about
half within 0.2 s, on gravel nearly to nothing.

The measurements are in [notes/ngp.md](../notes/ngp.md), section "Force feedback". The code is in
`crates/rbr-standalone/src/tyre_ffb.rs`.
