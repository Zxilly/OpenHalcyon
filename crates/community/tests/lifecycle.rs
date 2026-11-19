use halcyon_community::{
    Action, Community, CommunityError, GroupKind, GroupSettings, ItemDefinition, QuestDefinition,
    Role,
};
use serde_json::Value;
use uuid::Uuid;

async fn players(store: &Community, names: &[&str]) -> Vec<Uuid> {
    let mut ids = Vec::new();
    for name in names {
        let id = Uuid::new_v4();
        store
            .observe_player(id, (*name).into(), true)
            .await
            .unwrap();
        ids.push(id);
    }
    ids
}

#[tokio::test]
async fn friendship_requires_recipient_confirmation_and_preserves_presence_privacy() {
    let store = Community::open(":memory:".as_ref()).unwrap();
    let ids = players(&store, &["Harbor", "Lighthouse", "Relay"]).await;
    let [harbor, light, relay] = ids[..] else {
        unreachable!()
    };
    assert_eq!(
        store.resolve_handle("lighthouse".into()).await.unwrap(),
        light
    );
    store
        .execute(harbor, Action::RequestFriend { target: light })
        .await
        .unwrap();
    assert!(matches!(
        store
            .execute(
                relay,
                Action::ResolveFriend {
                    requester: harbor,
                    accept: true
                }
            )
            .await,
        Err(CommunityError::Missing(_))
    ));
    let incoming = store.execute(light, Action::Friends).await.unwrap();
    assert_eq!(incoming["requests"][0]["playerId"], harbor.to_string());
    assert_eq!(incoming["requests"][0]["incoming"], true);
    store
        .execute(
            light,
            Action::ResolveFriend {
                requester: harbor,
                accept: true,
            },
        )
        .await
        .unwrap();
    store
        .execute(
            light,
            Action::SetPresence {
                status: "available".into(),
            },
        )
        .await
        .unwrap();
    let favorites = store
        .execute(
            harbor,
            Action::FavoriteFriend {
                target: light,
                favorite: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(favorites["friends"][0]["status"], "available");
    assert_eq!(favorites["friends"][0]["favorite"], true);
    let reciprocal = store.execute(light, Action::Friends).await.unwrap();
    assert_eq!(reciprocal["friends"][0]["favorite"], false);
    store
        .execute(light, Action::SetInvisible { invisible: true })
        .await
        .unwrap();
    let hidden = store.execute(harbor, Action::Friends).await.unwrap();
    assert_eq!(hidden["friends"][0]["online"], false);
    assert!(hidden["friends"][0]["status"].is_null());
    store
        .execute(harbor, Action::RemoveFriend { target: light })
        .await
        .unwrap();
    assert_eq!(
        store.execute(light, Action::Friends).await.unwrap()["friends"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn group_invites_roles_and_party_layout_survive_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("community.sqlite");
    let store = Community::open(&path).unwrap();
    let ids = players(&store, &["Captain", "Quartermaster", "Ranger"]).await;
    let [owner, officer, outsider] = ids[..] else {
        unreachable!()
    };
    let guild = GroupKind::Guild;
    let group = store
        .execute(
            owner,
            Action::CreateGroup {
                kind: guild,
                name: "Pathfinders".into(),
                settings: GroupSettings {
                    tag: Some("PATH".into()),
                    ..Default::default()
                },
            },
        )
        .await
        .unwrap();
    let id = Uuid::parse_str(group["id"].as_str().unwrap()).unwrap();
    assert!(matches!(
        store
            .execute(outsider, Action::JoinOpenGuild { group: id })
            .await,
        Err(CommunityError::Forbidden)
    ));
    store
        .execute(
            owner,
            Action::Invite {
                kind: guild,
                target: officer,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .execute(
                outsider,
                Action::ResolveInvite {
                    group: id,
                    accept: true
                }
            )
            .await,
        Err(CommunityError::Missing(_))
    ));
    store
        .execute(
            officer,
            Action::ResolveInvite {
                group: id,
                accept: true,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .execute(
                officer,
                Action::Invite {
                    kind: guild,
                    target: outsider
                }
            )
            .await,
        Err(CommunityError::Forbidden)
    ));
    store
        .execute(
            owner,
            Action::SetRole {
                kind: guild,
                target: officer,
                role: Role::Officer,
            },
        )
        .await
        .unwrap();
    store
        .execute(
            officer,
            Action::Invite {
                kind: guild,
                target: outsider,
            },
        )
        .await
        .unwrap();
    store
        .execute(
            outsider,
            Action::ResolveInvite {
                group: id,
                accept: true,
            },
        )
        .await
        .unwrap();
    store
        .execute(owner, Action::LeaveGroup { kind: guild })
        .await
        .unwrap();
    let group = store
        .execute(officer, Action::Group { kind: guild })
        .await
        .unwrap();
    assert_eq!(group["members"][0]["role"], "owner");
    assert_eq!(group["memberCount"], 2);

    let party = store
        .execute(
            owner,
            Action::CreateGroup {
                kind: GroupKind::Party,
                name: "Practice party".into(),
                settings: GroupSettings {
                    mode: Some("practice".into()),
                    ..Default::default()
                },
            },
        )
        .await
        .unwrap();
    let join = party["joinCode"].as_str().unwrap().to_owned();
    store
        .execute(officer, Action::JoinPartyByCode { code: join })
        .await
        .unwrap();
    store.execute(owner, Action::BalanceParty).await.unwrap();
    drop(store);
    let reopened = Community::open(&path).unwrap();
    let saved = reopened
        .execute(
            officer,
            Action::Group {
                kind: GroupKind::Party,
            },
        )
        .await
        .unwrap();
    assert_eq!(saved["mode"], "practice");
    assert_eq!(saved["members"][1]["side"], 1);
    assert!(matches!(
        reopened
            .execute(
                officer,
                Action::DisbandGroup {
                    kind: GroupKind::Party
                }
            )
            .await,
        Err(CommunityError::Forbidden)
    ));
    reopened
        .execute(
            owner,
            Action::DisbandGroup {
                kind: GroupKind::Party,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        reopened
            .execute(
                officer,
                Action::Group {
                    kind: GroupKind::Party
                }
            )
            .await
            .unwrap(),
        Value::Null
    );
}

#[tokio::test]
async fn quests_and_inventory_can_only_be_earned_once() {
    let store = Community::open(":memory:".as_ref()).unwrap();
    let actor = players(&store, &["Ranger"]).await[0];
    let item = "wayfinder_badge";
    let quest = "practice_milestone";
    let action = || Action::Equip {
        slot: "badge".into(),
        item: Some(item.into()),
    };
    store
        .define_item(ItemDefinition {
            id: item.into(),
            slot: "badge".into(),
            name: "Wayfinder".into(),
        })
        .await
        .unwrap();
    store
        .define_quest(QuestDefinition {
            id: quest.into(),
            category: "practice".into(),
            title: "Practice milestone".into(),
            goal: 2,
            skippable: false,
            reward: Some((item.into(), 1)),
        })
        .await
        .unwrap();
    assert!(matches!(
        store.execute(actor, action()).await,
        Err(CommunityError::Forbidden)
    ));
    let progress = Uuid::new_v4();
    store
        .record_progress(progress, actor, quest.into(), 1)
        .await
        .unwrap();
    store
        .record_progress(progress, actor, quest.into(), 1)
        .await
        .unwrap();
    assert!(matches!(
        store
            .execute(
                actor,
                Action::ClaimQuest {
                    quest: quest.into()
                }
            )
            .await,
        Err(CommunityError::Conflict(_))
    ));
    store
        .record_progress(Uuid::new_v4(), actor, quest.into(), 1)
        .await
        .unwrap();
    let (first, second) = tokio::join!(
        store.execute(
            actor,
            Action::ClaimQuest {
                quest: quest.into()
            }
        ),
        store.execute(
            actor,
            Action::ClaimQuest {
                quest: quest.into()
            }
        ),
    );
    assert_eq!(first.unwrap()["items"][0]["state"], "claimed");
    assert_eq!(second.unwrap()["items"][0]["state"], "claimed");
    let loadout = store.execute(actor, action()).await.unwrap();
    assert_eq!(loadout["items"][0]["count"], 1);
    assert_eq!(loadout["equipped"]["badge"], item);
    let grant = Uuid::new_v4();
    store
        .award_item(grant, actor, item.into(), 2)
        .await
        .unwrap();
    store
        .award_item(grant, actor, item.into(), 2)
        .await
        .unwrap();
    let loadout = store.execute(actor, Action::Inventory).await.unwrap();
    assert_eq!(loadout["items"][0]["count"], 3);
}
