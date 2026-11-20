use halcyon_data::{BotDifficulty, GameData};
use halcyon_sim::{BotDiagnostics, Event, Kind, World};

// Native Solo slots 14..18 and 19..23 from the 147219 opening declarations.
const WEST: [&str; 5] = ["Amael", "Inara", "Ardan", "Tony", "Varya"];
const EAST: [&str; 5] = ["Skye", "Alpha", "Tony", "Idris", "Baron"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = args
        .next()
        .ok_or("expected the external delivery directory")?;
    let data = GameData::load(&dir)?;
    match (args.next().as_deref(), args.next()) {
        (Some("--bot-diagnostics"), None) => return diagnose(&data, false),
        (Some("--native-map-diagnostics"), Some(extra)) => {
            let maps = GameData::load_with_extra(dir, Some(std::path::Path::new(&extra)))?;
            return diagnose(&maps, true);
        }
        (None, None) => {}
        _ => return Err("expected --bot-diagnostics or --native-map-diagnostics EXTRA".into()),
    }
    println!("mode,seed,winner,ticks,west_towers,east_towers,first_tower,first_tower_tick");
    for mode in ["fallback", "mirror", "recorded", "swapped"] {
        for seed in 0..32 {
            let mut w = if mode == "fallback" {
                World::new(seed, 0)
            } else {
                World::from_data(seed, 0, &data)?
            };
            if mode != "fallback" {
                let teams = match mode {
                    "mirror" => [WEST, WEST],
                    "recorded" => [WEST, EAST],
                    _ => [EAST, WEST],
                };
                for seat in 0..10 {
                    w.select_hero(seat, teams[seat % 2][seat / 2], &data)?;
                }
            }
            w.use_solo_mid_lane()?;
            w.set_bot_difficulty(Some(BotDifficulty::Medium));
            let mut first = None;
            while w.tick < 200_000 && w.winner.is_none() {
                for event in w.step() {
                    if let Event::Death { id, .. } = event
                        && first.is_none()
                        && let Some(e) = w
                            .entities
                            .iter()
                            .find(|e| e.id == id && e.kind == Kind::Tower)
                    {
                        first = Some((e.id + 1499, w.tick));
                    }
                }
            }
            let (tower, tick) = first.unwrap_or_default();
            let [west, east] = w.team_scores();
            println!(
                "{mode},{seed},{},{},{},{},{tower},{tick}",
                w.winner.map_or("timeout".into(), |t| t.to_string()),
                w.tick,
                west.structure_kills.towers,
                east.structure_kills.towers
            );
        }
    }
    Ok(())
}

fn diagnose(data: &GameData, use_mesh: bool) -> Result<(), Box<dyn std::error::Error>> {
    let seed = 0;
    let recorded = || -> Result<World, String> {
        let mut world = World::from_data(seed, 0, data)?;
        for seat in 0..10 {
            world.select_hero(seat, [WEST, EAST][seat % 2][seat / 2], data)?;
        }
        world.use_solo_mid_lane()?;
        if use_mesh {
            world.install_native_navigation(data.native_maps().ok_or("map supplement missing")?)?;
        }
        world.set_bot_difficulty(Some(BotDifficulty::Medium));
        Ok(world)
    };
    let mut plain = recorded()?;
    let mut measured = recorded()?;
    let mut diagnostics = BotDiagnostics::new(&measured);
    while measured.tick < 200_000 && measured.winner.is_none() {
        measured.step_diagnosed(&mut diagnostics);
        plain.step();
        if measured.tick.is_multiple_of(1_000) {
            assert_eq!(
                measured.state_digest(),
                plain.state_digest(),
                "tick {}",
                measured.tick
            );
        }
    }
    let digest = measured.state_digest();
    assert_eq!(digest, plain.state_digest());
    println!(
        "mode=recorded seed={seed} ticks={} winner={:?} state_digest={digest}",
        measured.tick, measured.winner
    );
    for (actor, row) in diagnostics.heroes() {
        let rejects: u64 = row.rejected_orders.values().sum();
        let average = row
            .decision_time
            .as_nanos()
            .checked_div(row.decisions.into());
        println!(
            "actor={actor} template={} team={} decisions={} total_ns={} avg_ns={average:?} max_ns={} rejected={rejects}/{} reasons={:?} stalled_walk_ticks={}/{} longest_stall_ticks={}",
            row.template,
            row.team,
            row.decisions,
            row.decision_time.as_nanos(),
            row.max_decision_time.as_nanos(),
            row.decisions,
            row.rejected_orders,
            row.stalled_walk_ticks,
            row.walk_attempt_ticks,
            row.longest_stall_ticks
        );
    }
    if measured.winner.is_none() {
        return Err("diagnostic match reached its tick cap without a winner".into());
    }
    Ok(())
}
