use super::tests::{next, until};
use crate::Rooms;
use halcyon_protocol::{ACK_FRAME, Fields, MatchCipher, Message, hello};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{Duration, timeout},
};
use uuid::Uuid;
async fn shop_request(
    socket: &mut TcpStream,
    cipher: &MatchCipher,
    actor: u32,
    item: u32,
) -> Vec<Message> {
    let request = Message::new(0x0439, [actor.to_be_bytes(), item.to_be_bytes()].concat());
    let barrier = Message::new(0, [0; 6]);
    let mut bytes = cipher.encode(&request).unwrap();
    bytes.extend(cipher.encode(&barrier).unwrap());
    socket.write_all(&bytes).await.unwrap();
    let mut reply = Vec::new();
    loop {
        let frame = next(socket, cipher).await;
        if frame == barrier {
            return reply;
        }
        if matches!(frame.opcode, 0x0439 | 0x041c | 0x041d | 0x043d | 0x044b)
            && frame.body[..4] == actor.to_be_bytes()
        {
            reply.push(frame);
        }
    }
}

pub(super) async fn shop_sequence(
    clients: &mut [TcpStream],
    cipher: &MatchCipher,
    rooms: &Rooms,
    id: Uuid,
    address: &str,
    player: Uuid,
) {
    let shared = rooms.room(id).unwrap();
    // Independent seats reproduce the other recorded opening purchases with
    // their own two thousand gold, including a direct, component-free upgrade.
    for (seat, sequence) in [
        (4, &[(459, 300), (512, 750), (462, 650)][..]),
        (5, &[(469, 300), (538, 500), (475, 800)][..]),
        (1, &[(505, 1150), (461, 650)][..]),
        (2, &[(513, 600), (477, 300), (475, 800)]),
        (
            3,
            &[(458, 300), (458, 300), (477, 300), (505, 850), (473, 250)],
        ),
    ] {
        let actor = 1500 + seat as u32;
        for &(item, paid) in sequence {
            let frames = shop_request(&mut clients[seat], cipher, actor, item).await;
            let i = frames
                .iter()
                .position(|m| m.opcode == 0x439 && m.body[4..8] == item.to_be_bytes())
                .unwrap();
            assert!(
                matches!(frames[i+1].fields().unwrap(),Fields::EntityStat{stat:6,value,..} if value==-(paid as f32))
            );
            assert!(frames[i+2..].iter().any(|m|matches!(m.fields().unwrap(),Fields::ItemGrant{item:granted,..} if granted==item)));
        }
    }
    let socket = &mut clients[0];
    assert!(shop_request(socket, cipher, 1501, 459).await.is_empty());
    let mut instances = std::collections::BTreeMap::new();
    for (item, paid, stats) in [
        (459, 300, &[(5, 15.)][..]),
        (460, 300, &[(15, 0.1)]),
        (477, 300, &[]),
        (463, 400, &[(15, -0.1), (15, 0.2)]),
    ] {
        let frames = shop_request(socket, cipher, 1500, item).await;
        let expected = if item == 463 {
            vec![0x439, 0x41d, 0x41c, 0x44b, 0x43d, 0x41c]
        } else if stats.is_empty() {
            vec![0x439, 0x41d, 0x43d]
        } else {
            vec![0x439, 0x41d, 0x43d, 0x41c]
        };
        assert_eq!(
            frames.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            expected
        );
        assert!(
            matches!(frames[1].fields().unwrap(),Fields::EntityStat{stat:6,value,..} if value==-(paid as f32))
        );
        let mut mods = vec![];
        for m in frames {
            match m.fields().unwrap() {
                Fields::ItemGrant { item, instance, .. } => {
                    instances.insert(item, instance);
                }
                Fields::StatModifier { stat, value, .. } => mods.push((stat, value)),
                Fields::ItemRemoval { instance, .. } => assert_eq!(instance, instances[&460]),
                _ => {}
            }
        }
        assert_eq!(mods, stats);
    }
    assert!(shop_request(socket, cipher, 1500, 509).await.is_empty());
    // Advance real simulation time with the hero still on its starting pad.
    // Passive ARAL awards finance the observed late upgrades.
    {
        let mut room = shared.lock().unwrap();
        for _ in 0..(160 * halcyon_sim::TICK_HZ) {
            if room.world.entities[0].gold >= 2150 {
                break;
            }
            room.world.step();
        }
        assert!(room.world.entities[0].gold >= 2150);
    }
    let frames = shop_request(socket, cipher, 1500, 509).await;
    let start = frames.iter().position(|m| m.opcode == 0x439).unwrap();
    let frames = &frames[start..];
    assert_eq!(
        frames.iter().map(|m| m.opcode).collect::<Vec<_>>(),
        [
            0x439, 0x41d, 0x41c, 0x44b, 0x41c, 0x44b, 0x43d, 0x41c, 0x41c
        ]
    );
    assert!(matches!(
        frames[1].fields().unwrap(),
        Fields::EntityStat {
            stat: 6,
            value: -1800.,
            ..
        }
    ));
    for (i, component) in [(3, 463), (5, 459)] {
        assert!(
            matches!(frames[i].fields().unwrap(),Fields::ItemRemoval{instance,flags:1,..} if instance==instances[&component])
        );
    }
    let Fields::ItemGrant {
        instance: current, ..
    } = frames[6].fields().unwrap()
    else {
        panic!()
    };
    let boots = shop_request(socket, cipher, 1500, 478).await;
    let boots = boots
        .iter()
        .find_map(|m| match m.fields().ok()? {
            Fields::ItemGrant {
                item: 478,
                instance,
                ..
            } => Some(instance),
            _ => None,
        })
        .unwrap();

    socket.shutdown().await.unwrap();
    timeout(Duration::from_secs(3), async {
        while shared.lock().unwrap().seats[0].peers[0].is_some() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let mut back = TcpStream::connect(address).await.unwrap();
    back.write_all(&hello("localhost").unwrap()).await.unwrap();
    let mut ack = [0; 5];
    back.read_exact(&mut ack).await.unwrap();
    assert_eq!(ack, ACK_FRAME);
    back.write_all(
        &cipher
            .encode(&Message::new(0x03e8, player.to_string().as_bytes()))
            .unwrap(),
    )
    .await
    .unwrap();
    until(&mut back, cipher, 0x046c).await;
    for msg in [
        Message::new(0x046e, [0; 6]),
        Message::new(0x0471, [0, 0, 5, 220, 1, 0]),
    ] {
        back.write_all(&cipher.encode(&msg).unwrap()).await.unwrap();
    }
    let mut restored = vec![];
    let mut restored_modifiers = vec![];
    let mut balance = None;
    loop {
        let m = next(&mut back, cipher).await;
        if m.opcode == 0x046e {
            break;
        }
        if m.opcode == 0x03f3 && m.body[8..12] == 1500u32.to_be_bytes() {
            balance = m.spawn_gold();
        }
        match m.fields().unwrap() {
            Fields::ItemGrant {
                actor: 1500,
                item,
                instance,
            } => restored.push((item, instance)),
            Fields::StatModifier {
                actor: 1500,
                stat,
                value,
                ..
            } => restored_modifiers.push((stat, value)),
            _ => {}
        }
    }
    assert_eq!(restored, [(509, current), (478, boots)]);
    assert_eq!(restored_modifiers, [(5, 45.), (15, 0.4)]);
    let (gold, worth) = balance.unwrap();
    assert_eq!(worth - gold, 3450.);
}
