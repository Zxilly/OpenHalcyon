use halcyon_protocol::{Fields, Message};
use halcyon_sim::Position;

// All witnessed Idris B stops in the archive share this ground height.
// Limit the profile to their recorded horizontal extent.
const GROUND: f32 = f32::from_bits(0x3d19_2e1f);

fn observed_extent(position: Position) -> bool {
    (-58.21..=23.38).contains(&position.x) && (-1.83..=9.47).contains(&position.y)
}

pub(crate) fn stop(actor: u32, position: Position) -> Option<Message> {
    if !observed_extent(position) {
        return None;
    }
    let stop = Fields::StopActor {
        actor,
        values: [position.x, GROUND, position.y],
        flag: 0,
        remaining: &[],
    }
    .movement_message()
    .expect("fixed ARAL stop format");
    Some(stop)
}

pub(crate) fn recordable_stop(message: &Message) -> bool {
    let Ok(Fields::StopActor {
        actor: 1500..=1505,
        values: [x, height, z],
        flag: 0,
        remaining: [],
    }) = message.fields()
    else {
        return false;
    };
    height.to_bits() == GROUND.to_bits() && observed_extent(Position { x, y: z })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archives_only_the_verified_finite_ground_profile() {
        let mut frame = stop(1500, Position { x: -8., y: 5. }).unwrap();
        assert!(recordable_stop(&frame));
        frame.body[16] = 1;
        assert!(!recordable_stop(&frame));
        assert!(stop(1500, Position { x: -80., y: 5. }).is_none());
        assert!(stop(1500, Position { x: f32::NAN, y: 5. }).is_none());
    }
}
