diesel::table! {
    community_players (id) {
        id -> Text,
        handle -> Text,
        online -> Bool,
        invisible -> Bool,
        receive_presence -> Bool,
        status -> Text,
        last_seen -> BigInt,
    }
}
diesel::table! {
    community_friend_requests (requester, recipient) {
        requester -> Text,
        recipient -> Text,
        created -> BigInt,
    }
}
diesel::table! {
    community_friends (player, friend) {
        player -> Text,
        friend -> Text,
        favorite -> Bool,
    }
}
diesel::allow_tables_to_appear_in_same_query!(
    community_players,
    community_friends,
    community_friend_requests
);
diesel::table! {
    community_items(id) {
        id -> Text,
        slot -> Text,
        name -> Text,
    }
}
diesel::table! {
    community_inventory(player,item) {
        player -> Text,
        item -> Text,
        count -> BigInt,
    }
}
diesel::table! {
    community_equipment(player,slot) {
        player -> Text,
        slot -> Text,
        item -> Text,
    }
}
diesel::table! {
    community_item_grants(id) {
        id -> Text,
        player -> Text,
        item -> Text,
        count -> BigInt,
    }
}
diesel::table! {
    community_quests(id) {
        id -> Text,
        category -> Text,
        title -> Text,
        goal -> BigInt,
        skippable -> Bool,
        reward_item -> Nullable<Text>,
        reward_count -> Nullable<BigInt>,
    }
}
diesel::table! {
    community_quest_states(player,quest) {
        player -> Text,
        quest -> Text,
        progress -> BigInt,
        state -> Text,
    }
}
diesel::table! {
    community_quest_progress(id) {
        id -> Text,
        player -> Text,
        quest -> Text,
        amount -> BigInt,
    }
}
diesel::table! {
    community_redemptions(player,request) {
        player -> Text,
        operation -> Text,
        sku -> Text,
        request -> Binary,
    }
}
diesel::table! {
    community_members(group_id,player) {
        group_id -> Text,
        kind -> Text,
        player -> Text,
        role -> Text,
        side -> BigInt,
    }
}
diesel::allow_tables_to_appear_in_same_query!(
    community_inventory,
    community_items,
    community_equipment
);
diesel::table! {
    community_groups(id) {
        id -> Text,
        kind -> Text,
        name -> Text,
        tag -> Text,
        motto -> Text,
        mode -> Text,
        invite_only -> Bool,
        lobby_open -> Bool,
        code -> Nullable<Text>,
        created -> BigInt,
    }
}
diesel::table! {
    community_invites(group_id,player) {
        group_id -> Text,
        player -> Text,
        inviter -> Text,
        created -> BigInt,
    }
}
diesel::table! {
    community_party_queues(party) {
        party -> Text,
        ticket -> Text,
    }
}
diesel::allow_tables_to_appear_in_same_query!(
    community_groups,
    community_invites,
    community_members,
    community_players
);
