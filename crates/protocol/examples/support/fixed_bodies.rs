//! Compare the bidirectional fixed layouts against the original prefix and
//! any opaque capture suffix, independently of the transport round trip.
use halcyon_protocol::{
    DeathBody, Fields, ImpactBody, Message, MoveCmd, Opcode, PositionBody, VisibilityBody,
};
use std::io;

pub(super) fn verify(message: &Message) -> io::Result<()> {
    let body = &message.body;
    let mut buf = [0; 46]; // Largest supported projectile body after cipher padding.
    let used = match message.known_opcode() {
        Some(Opcode::EntityState) if matches!(body.len(), 8 | 14) => {
            VisibilityBody::read_body(body)?.write_body(&mut buf)?
        }
        Some(Opcode::MoveCmd) => MoveCmd::read_body(body)?.write_body(&mut buf)?,
        Some(Opcode::Position) => PositionBody::read_body(body)?.write_body(&mut buf)?,
        Some(Opcode::ActorDeath) => DeathBody::read_body(body)?.write_body(&mut buf)?,
        Some(Opcode::HealthImpact) => ImpactBody::read_body(body)?.write_body(&mut buf)?,
        Some(
            Opcode::FireProjectile
            | Opcode::FirePointProjectile
            | Opcode::FireDirectedProjectile
            | Opcode::DetonateProjectile,
        ) => {
            let Fields::Projectile(p) = message.fields()? else {
                return Ok(());
            };
            let used = p.write_body(&mut buf)?;
            if used != body.len() {
                return Err(io::Error::other("projectile suffix length mismatch"));
            }
            used
        }
        _ => return Ok(()),
    };
    if body.get(..used) != Some(&buf[..used]) {
        return Err(io::Error::other("fixed codec differs from original bytes"));
    }
    Ok(())
}
