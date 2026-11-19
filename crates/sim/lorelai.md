# Lorelai on the hosted battlefield

The data pack names hero template 409, Fish Food 410 and Splashdown 411. Her
ARAL seat sent six A and five B point requests and one C request targeting another
allied hero. The game returns cast confirmation after release. The simulator
also uses the same resource and cooldown checks for a bot. It can shield a
wounded teammate and place pools around a visible opponent.

Fish Food stuns on impact and leaves a damaging pool. Splashdown speeds movement
for allies and slows enemies while inside. Waterwall clears movement impairments,
absorbs incoming damage and drops a pool at the recipient's later position.
Standing in one's own active pool consumes it on the next cast. The public hero
description gives an eight-second lifetime for the pools; the 3.9 balance notes
supply later Fish Food range, energy and 0.7-second delay, and change its
empowered effect to a longer stun. The same notes supply Splashdown energy and
Waterwall energy, barrier strengths and six-second empowered duration.
[Hero description](https://www.vainglorygame.com/heroes/lorelai/) · [Published 3.9 changes](https://www.vainglorygame.com/news/update-3-9-hero-item-balance-changes/)

The 4.13 data pack has no complete named Lorelai rank table. Hosted rules use
two- and three-unit circular areas, discrete one-second Fish Food damage ticks,
a 25% ally movement bonus, health-scaled slowing and a four-second base barrier.
Cooldowns, basic-attack crystal damage, empowered bonuses, barrier stacking
and the damage, crystal-power and bonus-health scaling are partial local models.
The historical requests constrain selectors and legal targets; they do not
validate the full numeric 4.13 combat model. [Hosted implementation](src/lorelai.rs)
