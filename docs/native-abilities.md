# Native Idris B

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
current ARAL validation layout has its own extents. Its tests use points
legal on that layout and keep the historical vectors as codec fixtures.

Learning index 1 on an authenticated, registered native control seat
spends an available skill point before confirming. Repeated learning in
the same simulation tick receives no additional grant. Point casting uses
the simulator's target, range, resource and cooldown checks. Only an
authoritative release emits 0x0416 to the originating connection.
Canceled windups and rejected inputs do not emit it. The capture does not
establish other heroes' ability-slot mappings or the exact native stamina,
collision or damage rules.
