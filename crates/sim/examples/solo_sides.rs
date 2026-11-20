use halcyon_data::{BotDifficulty, GameData};
use halcyon_sim::{Event, Kind, World};

// Native Solo slots 14..18 and 19..23 from the 147219 opening declarations.
const WEST: [&str; 5] = ["Amael", "Inara", "Ardan", "Tony", "Varya"];
const EAST: [&str; 5] = ["Skye", "Alpha", "Tony", "Idris", "Baron"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args()
        .nth(1)
        .ok_or("expected the external delivery directory")?;
    let data = GameData::load(dir)?;
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
