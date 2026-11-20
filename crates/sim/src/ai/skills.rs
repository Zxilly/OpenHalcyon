use super::*;

impl Room<'_> {
    /// Select an intent from the same learned abilities, cooldowns and resource checks as a player.
    pub(super) fn hero_combat_order(&self, caster: usize, enemy: &Entity) -> Option<Command> {
        use AbilitySlot::{A, B, C};
        use CastTarget::SelfCast;
        let hero = &self.entities[caster];
        let distance = hero.position.dist(enemy.position);
        let point = CastTarget::Point(enemy.position);
        let target = CastTarget::Entity(enemy.id);
        let cast = |eligible, slot, target| {
            (eligible && self.can_cast(caster, slot, target))
                .then_some(Command::Cast { slot, target })
        };
        let specific = match EntityTemplate::from_raw(hero.type_id) {
            Some(EntityTemplate::Adagio) => {
                cast(self.verse_reaches(caster, enemy.position), C, SelfCast)
            }
            Some(EntityTemplate::Lyra) => self
                .sigil_order(caster, enemy)
                .or_else(|| cast(self.bulwark_reaches(caster, enemy.position), B, SelfCast)),
            Some(EntityTemplate::Amael) => self.amael_kick_order(caster, enemy),
            Some(EntityTemplate::Karas) => cast(self.abilities.twister.is_some(), B, point),
            Some(EntityTemplate::Catherine) => {
                cast(self.pursuit_reaches(caster, enemy.position), A, SelfCast)
                    .or_else(|| {
                        cast(
                            distance <= enemy.stats.range.max(hero.stats.range) + 1.,
                            B,
                            SelfCast,
                        )
                    })
                    .or_else(|| cast(true, C, point))
            }
            Some(EntityTemplate::Yates) => {
                cast(self.overwhelm_reaches(caster, enemy.position), B, SelfCast)
                    .or_else(|| cast(true, A, point))
                    .or_else(|| cast(true, C, point))
            }
            Some(EntityTemplate::Lorelai) => self.lorelai_action(caster, enemy.position),
            Some(EntityTemplate::Reim) => cast(
                self.chill_winds_radius(caster)
                    .is_some_and(|radius| distance <= radius),
                B,
                SelfCast,
            )
            .or_else(|| cast(true, A, point)),
            Some(EntityTemplate::Idris) => cast(true, B, point),
            _ => None,
        };
        specific
            .or_else(|| {
                cast(
                    enemy.hp < enemy.max_hp * 0.35 && distance > hero.stats.range,
                    C,
                    target,
                )
            })
            .or_else(|| cast(true, A, target))
            .or_else(|| cast(distance <= hero.stats.range, B, SelfCast))
    }
}
