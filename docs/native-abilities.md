# Native ability requests, grants and releases

## Idris B

The Android catalog identifies Idris as template 273. In the played ARAL
archive for that seat, four 0x0436 requests with index 1 each received a
0x0436 echo and 0x043A grant with actor ID and operand 3. The stock room
can select Idris when its configured game data includes that name and
template. The default validation roster uses Ringo.

Four 0x0412 requests from the same seat used selector 3. Every request
contained a three-float vector and one zero padding byte. The full vector
and selector reappeared in a 0x0416 response from the same actor after
39–99 ms. The signed-zero middle component is preserved.

| Vector X, Y, Z | Nearest reported actor X, Z | Approximate planar distance |
| --- | --- | ---: |
| (-40.8939, 0, 1.4944) | (-38.417, 0.785) | 2.58 |
| (-58.2402, -0, 14.9417) | (-65.077, 12.327) | 7.32 |
| (-44.1787, -0, 3.5794) | (-46.192, 8.335) | 5.16 |
| (-34.1510, 0, -0.7185) | (-36.317, 5.817) | 6.88 |

The positions cluster around the caster in world space. The vectors
themselves have lengths 34–60 and do not represent unit directions.
OpenHalcyon uses X/Z as the authored simulation point for Idris B. The
server models the measured ARAL lane within explicit bounds. Tests use
points inside those bounds and keep the historical vectors as codec fixtures.

Learning index 1 on an authenticated, registered native control seat
spends an available skill point before confirming. Repeated learning in
the same simulation tick receives no additional grant. Point casting uses
the simulator's target, range, resource and cooldown checks. Only an
authoritative release emits 0x0416 to the originating connection.
Canceled windups and rejected inputs do not emit it. The capture does not
establish other heroes' ability-slot mappings or the exact native stamina,
collision or damage rules.

## Lorelai

Template 409 used learning indices 0, 1 and 2. In seven successful grants the
0x043A operand equaled the requested index (two A, four B, one C). Six A and
five B point requests had matching 0x0416 releases after 35–142 ms. Their
distances from the latest reported caster position ranged from 5.27 to 8.00
and 4.71 to 10.00 units respectively. The C request named another hero on
the caster's team. A 0x0415 carried caster, identical target and selector 2
55 ms later. The native input handler supports that allied-hero form.

## Witnessed grants and releases

Across seven complete ARAL recordings, the actor in the client's 0x0471
identifies its controlled seat. Initial full hero declarations supply the
other seats and their teams. Counts below include repeated actions.

| Server message | Controlled hero | Other allied hero | Opposing hero | Outside six hero IDs |
| --- | ---: | ---: | ---: | ---: |
| 0x0415 targeted release | 21 | 1,607 | 2,135 | 5,343 |
| 0x0416 point release | 15 | 215 | 654 | 2 |
| 0x043A learn grant | 24 | 104 | 189 | 0 |

An allied Idris accounts for 34 witnessed selector-3 point releases and five
operand-3 grants. Lorelai appears only on the controlled seat. The room
applies the shared release format to mapped skills from successful human and
bot simulation events. A team's current world view must contain the caster
and, for a targeted release, the target. A live authorized observer receives
the full view. The requesting control generation receives one confirmation
with the original point bytes. A replacement socket for that seat receives
no confirmation of the former connection's input. Skill-point expenditure
produces the public grant. Only the requesting seat receives its input echo.
