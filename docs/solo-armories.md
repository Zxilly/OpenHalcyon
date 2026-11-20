# Solo armories

The Android 147219 Solo opening declares template 297 (VainNode) in six slots. In slot order 0 to 5 the X/Z positions are (-70,-11), (-63,0), (-70,11), (70,-11), (63,0), (70,11). Wire actors 2000 through 2005 retain their identities after rebuilding. The first three defend the home crystal. The delivered unit has 4350 HP, 100 armor, 100 shield, zero weapon damage, range, attack speed and movement speed. The simulation treats each as a stationary objective.

A living damaged armory returns to full HP 180 seconds after its last effective hit. A destroyed armory rebuilds at full health after 270 seconds. Destruction increments the attacking team's armory count and sets its first-armory timestamp once. The native results writer reports that timestamp in seconds. The host assigns no armory bounty or XP. [Update 3.7, regeneration](https://www.vainglorygame.com/news/update-3-7-hero-item-balance-changes/) · [Update 3.5, rebuilding](https://www.vainglorygame.com/news/update-3-5-hero-item-balance-changes/)

The official map ties the missile count per barrage to standing armories. Update 3.7 changed the firing pattern. Update 4.6 specifies 4% of target maximum health per shot and 60% slow. These sources leave the timing and target allocation within a multi-shot barrage unresolved. The host retains its single-target attack cadence while any allied armory stands and suppresses these attacks when all three are down. Rebuilding restores the attack. Crystal mitigation stays independent of the armories following Update 3.9. [Official map](https://en.vainglorygame.com/5v5-map/) · [Update 3.9](https://www.vainglorygame.com/news/update-3-9-magnus-arcane-prince/) · [Update 4.6](https://www.vainglorygame.com/news/update-46-hero-item-balance-changes/)

The shipped tutorial line `SIDEBAR_ARDAN_TUTORIAL2A_DIALOG_LINE_ARMORY`
requires destruction of the turret in front of the armory before it can take
damage. In `level_layouts.json`, `TutorialLevel2_Joystick` and
`TestingGrounds_5v5` use the same F002 map and place all six named lane
VainNodes behind their side's matching base turrets. The Android Solo opening
declares the same positions as templates 297 and 341. In top/middle/bottom
order the home node actors 2000–2002 are protected by 2051/2065/2079, and
away actors 2003–2005 by 2093/2107/2121. The host checks the appropriate
living turret when damage lands. Protected hits leave HP, recovery timing
and damage credit untouched. A rebuilt node can take damage if its turret
has already fallen.

The map identifies a lane-minion empowerment upon destroying an enemy armory.
The exact wave enhancement remains unresolved.
