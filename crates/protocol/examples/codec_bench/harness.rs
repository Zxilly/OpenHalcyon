use std::hint::black_box;

use halcyon_protocol::{Fields, MatchCipher, Message, ProjectileAction, ProjectileFire};

const FIXTURES: usize = 64;
const CASES: [Case; 7] = [
    Case::Movement,
    Case::Position,
    Case::Impact,
    Case::Death,
    Case::Tracked,
    Case::Point,
    Case::Directed,
];

#[derive(Clone, Copy)]
enum Case {
    Movement,
    Position,
    Impact,
    Death,
    Tracked,
    Point,
    Directed,
}

impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Movement => "movement",
            Self::Position => "position",
            Self::Impact => "impact",
            Self::Death => "death",
            Self::Tracked => "projectile_tracked",
            Self::Point => "projectile_point",
            Self::Directed => "projectile_directed",
        }
    }

    fn message(self, i: usize) -> Message {
        let actor = 1500 + i as u16;
        let x = i as f32 * 0.625;
        match self {
            Self::Movement => Message::movement(x, -x),
            Self::Position => Message::position(actor, -x, x + 0.25),
            Self::Impact => Message::health_impact(actor.into(), 1500, -x, 2, 1, 0),
            Self::Death => Message::actor_death(actor.into(), 1500),
            _ => self.action(i).expect("projectile case").message().unwrap(),
        }
    }

    fn action(self, i: usize) -> Option<ProjectileAction<'static>> {
        let x = i as f32 * 0.625;
        let slot = i as u8;
        Some(match self {
            Self::Tracked => ProjectileAction::FireTracked(fire(i, [x], [slot, slot, 0])),
            Self::Point => ProjectileAction::FirePoint(fire(i, [x, 0.5, -x, 7.], [slot, 0])),
            Self::Directed => ProjectileAction::FireDirected(fire(
                i,
                [x, 0.5, -x, 1., -1., 0., 2., 7.],
                [slot, 0],
            )),
            _ => return None,
        })
    }

    fn verify(self, frame: &Message) {
        let fields = frame.fields().unwrap();
        assert!(matches!(
            (self, fields),
            (Self::Movement, Fields::Movement { .. })
                | (Self::Position, Fields::Position { .. })
                | (Self::Impact, Fields::HealthImpact { .. })
                | (Self::Death, Fields::ActorDeath { .. })
                | (
                    Self::Tracked,
                    Fields::Projectile(ProjectileAction::FireTracked(_))
                )
                | (
                    Self::Point,
                    Fields::Projectile(ProjectileAction::FirePoint(_))
                )
                | (
                    Self::Directed,
                    Fields::Projectile(ProjectileAction::FireDirected(_))
                )
        ));
    }
}

fn fire<const N: usize, const S: usize>(
    i: usize,
    values: [f32; N],
    slots: [u8; S],
) -> ProjectileFire<'static, N, S> {
    ProjectileFire {
        projectile: 4000 + i as u32,
        profile_word: 0,
        values,
        operand: i as u16,
        slots,
        padding: &[],
    }
}

fn owned_to_destination(action: &ProjectileAction<'_>, dst: &mut [u8]) -> usize {
    let message = action.message().unwrap();
    dst[..message.body.len()].copy_from_slice(&message.body);
    message.body.len()
}

// The new inherent writer takes precedence over this trait. Compiling the
// exact same harness with an older protocol library exercises its owned API.
#[allow(dead_code)]
trait OlderWriter {
    fn write_body(&self, dst: &mut [u8]) -> std::io::Result<usize>;
}
impl OlderWriter for ProjectileAction<'_> {
    fn write_body(&self, dst: &mut [u8]) -> std::io::Result<usize> {
        Ok(owned_to_destination(self, dst))
    }
}

struct Fixture {
    message: Message,
    plain: Vec<u8>,
    framed: Vec<u8>,
}

fn make_fixtures(case: Case, cipher: &MatchCipher) -> [Fixture; FIXTURES] {
    std::array::from_fn(|i| {
        let message = case.message(i);
        case.verify(&message);
        let mut plain = message.opcode.to_be_bytes().to_vec();
        plain.extend(&message.body);
        let framed = cipher.encode(&message).unwrap();
        assert_eq!(cipher.decode(&framed[2..]).unwrap().opcode, message.opcode);
        assert_eq!(
            &cipher.decode_owned(framed[2..].to_vec()).unwrap().body[..message.body.len()],
            message.body
        );
        if let Some(action) = case.action(i) {
            let mut owned = [0; 64];
            let mut reused = [0; 64];
            let size = owned_to_destination(&action, &mut owned);
            assert_eq!(action.write_body(&mut reused).unwrap(), size);
            assert_eq!(owned, reused);
        }
        Fixture {
            message,
            plain,
            framed,
        }
    })
}

type Sample<'a> = dyn FnMut(&str, &str, usize, &mut dyn FnMut(usize)) + 'a;

fn task(
    sample: &mut Sample<'_>,
    case: Case,
    operation: &str,
    iterations: usize,
    mut execute: impl FnMut(usize),
) {
    sample(case.name(), operation, iterations, &mut |n| {
        for i in 0..n {
            execute(black_box(i & (FIXTURES - 1)));
        }
    });
}

pub fn run(iterations: usize, sample: &mut Sample<'_>) {
    assert!(iterations >= FIXTURES);
    let cipher = MatchCipher::new("00000000-0000-0000-0000-000000000000");
    let matrix = CASES.map(|case| make_fixtures(case, &cipher));
    // FNV-1a identifies the synthetic inputs and outputs across both builds.
    let fingerprint = matrix
        .iter()
        .flatten()
        .flat_map(|f| f.plain.iter().chain(&f.framed).copied())
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
    println!("# synthetic fixture fingerprint={fingerprint:016x}");
    for (case, fixtures) in CASES.into_iter().zip(&matrix) {
        task(sample, case, "owned_encode", iterations, |i| {
            black_box(case.message(i));
        });
        task(sample, case, "fields_decode", iterations, |i| {
            black_box(black_box(&fixtures[i].message).fields().unwrap());
        });
        task(sample, case, "plain_decode_owned", iterations, |i| {
            black_box(Message::from_plain(black_box(&fixtures[i].plain)).unwrap());
        });
        task(sample, case, "cipher_encode", iterations, |i| {
            black_box(cipher.encode(black_box(&fixtures[i].message)).unwrap());
        });
        let mut output = Vec::with_capacity(64);
        task(sample, case, "cipher_encode_into", iterations, |i| {
            output.clear();
            cipher
                .encode_into(black_box(&fixtures[i].message), &mut output)
                .unwrap();
            black_box(output.as_slice());
        });
        task(sample, case, "cipher_decode", iterations, |i| {
            black_box(cipher.decode(black_box(&fixtures[i].framed[2..])).unwrap());
        });
        task(sample, case, "cipher_decode_owned_input", iterations, |i| {
            let owned = black_box(&fixtures[i].framed[2..]).to_vec();
            black_box(cipher.decode_owned(owned).unwrap());
        });
        if case.action(0).is_some() {
            let mut dst = [0; 64];
            task(sample, case, "projectile_owned_to_dst", iterations, |i| {
                let action = black_box(case.action(i).unwrap());
                let size = owned_to_destination(&action, &mut dst);
                black_box(&dst[..size]);
            });
            task(sample, case, "projectile_reused_to_dst", iterations, |i| {
                let action = black_box(case.action(i).unwrap());
                let size = action.write_body(&mut dst).unwrap();
                black_box(&dst[..size]);
            });
        }
    }
}

pub fn iterations() -> usize {
    std::env::args()
        .nth(1)
        .map_or(250_000, |arg| arg.parse().expect("iteration count"))
}
