diesel::table! {
    players (id) {
        id -> Text,
        name -> Text,
        recovery -> Binary,
        last_seen -> BigInt,
    }
}

diesel::table! {
    sessions (key) {
        key -> Binary,
        player -> Text,
    }
}

diesel::table! {
    local_accounts (login) {
        login -> Text,
        player -> Text,
        hash -> Text,
    }
}

diesel::table! {
    notification_events (player, ordinal) {
        player -> Text,
        ordinal -> BigInt,
        event_cursor -> BigInt,
    }
}

diesel::table! {
    events (cursor) {
        cursor -> BigInt,
        player -> Text,
        body -> Text,
    }
}

diesel::table! {
    matches (id) {
        id -> Text,
        mode -> Text,
        phase -> Text,
        address -> Nullable<Text>,
        result -> Nullable<Text>,
        offered_at -> BigInt,
        difficulty -> Nullable<Text>,
        talent_levels -> Nullable<Text>,
    }
}

diesel::table! {
    tickets (serial) {
        serial -> BigInt,
        player -> Text,
        mode -> Text,
        match_id -> Nullable<Text>,
        accepted -> Bool,
        batch -> Nullable<Text>,
        seat_limit -> Nullable<BigInt>,
        difficulty -> Nullable<Text>,
        queued_at_ms -> Nullable<BigInt>,
    }
}

diesel::table! {
    federated_chat_receipts (pool, room, id) {
        pool -> Text,
        room -> Text,
        id -> Text,
        seen -> BigInt,
    }
}

diesel::table! {
    federated_queue (ticket) {
        ticket -> Text,
        player -> Text,
        pool -> Text,
        mode -> Text,
        seats -> BigInt,
        accepted_match -> Nullable<Text>,
        cancel -> Bool,
        phase -> Text,
        match_id -> Nullable<Text>,
        address -> Nullable<Text>,
        result -> Nullable<Text>,
        enqueue_schema -> BigInt,
        difficulty -> Nullable<Text>,
        queued_at_ms -> Nullable<BigInt>,
    }
}

diesel::table! {
    federated_site_reports (ticket) {
        ticket -> Text,
        estimates -> Text,
    }
}

diesel::table! {
    federated_queue_members (ticket, player) {
        ticket -> Text,
        player -> Text,
        pos -> BigInt,
        accepted_match -> Nullable<Text>,
        released -> Bool,
    }
}

diesel::table! {
    federated_talent_levels (ticket) {
        ticket -> Text,
        levels -> Text,
    }
}

diesel::table! {
    native_rejections (player, room) {
        player -> Text,
        room -> Text,
        ticket -> Nullable<Text>,
    }
}

diesel::table! {
    native_settlements (room) {
        room -> Text,
        at_ms -> BigInt,
    }
}

diesel::table! {
    native_offer_seats (room, player) {
        room -> Text,
        player -> Text,
        entry -> Text,
        position -> BigInt,
        accepted -> Bool,
    }
}

diesel::table! {
    spectator_sessions (player) {
        player -> Text,
        request -> Text,
        match_id -> Text,
        spectator_id -> Nullable<Text>,
        address -> Nullable<Text>,
        cancelled -> Bool,
    }
}

diesel::table! {
    pooled_spectators (player) {
        player -> Text,
        friend -> Text,
        pool -> Text,
        host -> Text,
        match_id -> Text,
        mode -> Text,
        request -> Text,
        address -> Nullable<Text>,
        spectator_id -> Nullable<Text>,
        cancelled -> Bool,
    }
}

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

diesel::table! {
    community_items (id) {
        id -> Text,
        slot -> Text,
        name -> Text,
    }
}

diesel::table! {
    community_inventory (player, item) {
        player -> Text,
        item -> Text,
        count -> BigInt,
    }
}

diesel::table! {
    community_equipment (player, slot) {
        player -> Text,
        slot -> Text,
        item -> Text,
    }
}

diesel::table! {
    community_item_grants (id) {
        id -> Text,
        player -> Text,
        item -> Text,
        count -> BigInt,
    }
}

diesel::table! {
    community_quests (id) {
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
    community_quest_states (player, quest) {
        player -> Text,
        quest -> Text,
        progress -> BigInt,
        state -> Text,
    }
}

diesel::table! {
    community_quest_progress (id) {
        id -> Text,
        player -> Text,
        quest -> Text,
        amount -> BigInt,
    }
}

diesel::table! {
    community_redemptions (player, request) {
        player -> Text,
        operation -> Text,
        sku -> Text,
        request -> Binary,
    }
}

diesel::table! {
    community_members (group_id, player) {
        group_id -> Text,
        kind -> Text,
        player -> Text,
        role -> Text,
        side -> BigInt,
    }
}

diesel::table! {
    community_groups (id) {
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
    community_invites (group_id, player) {
        group_id -> Text,
        player -> Text,
        inviter -> Text,
        created -> BigInt,
    }
}

diesel::table! {
    community_party_queues (party) {
        party -> Text,
        ticket -> Text,
    }
}

diesel::table! {
    ce_preferences (player, category) {
        player -> Text,
        category -> Text,
        data -> Text,
    }
}

diesel::table! {
    ce_metrics (player, match_id) {
        player -> Text,
        match_id -> Text,
        data -> Text,
    }
}

diesel::table! {
    ce_scores (player, achievement) {
        player -> Text,
        achievement -> Text,
        score -> BigInt,
    }
}

diesel::table! {
    ce_messages (id) {
        id -> Text,
        sender -> Text,
        recipient -> Text,
        body -> Text,
        created -> BigInt,
        dismissed -> Bool,
    }
}

diesel::table! {
    ce_match_handles (room, player) {
        room -> Text,
        player -> Text,
        handle -> Text,
    }
}

diesel::table! {
    ce_honor (match_id, source, target) {
        match_id -> Text,
        source -> Text,
        target -> Text,
        kind -> Text,
    }
}

diesel::table! {
    ce_pending_gifts (player, target, sku) {
        player -> Text,
        target -> Text,
        sku -> Text,
    }
}

diesel::table! {
    ce_wait (player) {
        player -> Text,
        mode -> Text,
        joined -> BigInt,
    }
}

diesel::table! {
    ce_wait_history (mode) {
        mode -> Text,
        seconds -> Double,
    }
}

diesel::table! {
    ce_provider_orders (id) {
        id -> Text,
        player -> Text,
        request -> Text,
        product -> Text,
        item -> Text,
        quantity -> BigInt,
        expires -> BigInt,
        receipt -> Nullable<Binary>,
    }
}

diesel::table! {
    ce_provider_identity (player) {
        player -> Text,
        valid_until -> BigInt,
    }
}

diesel::table! {
    ce_migrations (name) {
        name -> Text,
    }
}

diesel::table! {
    ce_completed (player, room) {
        player -> Text,
        room -> Text,
        won -> Nullable<Bool>,
        hero -> Nullable<Text>,
        mode -> Nullable<Text>,
        day -> Nullable<BigInt>,
    }
}

diesel::table! {
    ce_player_matches (player) {
        player -> Text,
        completed -> BigInt,
        wins -> BigInt,
        non_tutorial -> BigInt,
        aral -> BigInt,
        blitz -> BigInt,
        blitz_rounds_casual -> BigInt,
        casual -> BigInt,
        ranked -> BigInt,
        last_won -> Nullable<Bool>,
        streak -> BigInt,
    }
}

diesel::table! {
    ce_daily_wins (player, day) {
        player -> Text,
        day -> BigInt,
        wins -> BigInt,
    }
}

diesel::table! {
    ce_hero_matches (player, hero) {
        player -> Text,
        hero -> Text,
        completed -> BigInt,
        wins -> BigInt,
    }
}

diesel::joinable!(notification_events -> events (event_cursor));
diesel::joinable!(federated_queue_members -> federated_queue (ticket));
diesel::joinable!(federated_site_reports -> federated_queue (ticket));
diesel::joinable!(federated_talent_levels -> federated_queue (ticket));
diesel::joinable!(tickets -> matches (match_id));
diesel::joinable!(spectator_sessions -> matches (match_id));
diesel::joinable!(community_friends -> community_players (friend));
diesel::joinable!(community_inventory -> community_items (item));
diesel::joinable!(community_equipment -> community_items (item));
diesel::joinable!(community_invites -> community_groups (group_id));
diesel::joinable!(community_members -> community_groups (group_id));

diesel::allow_tables_to_appear_in_same_query!(
    players,
    sessions,
    local_accounts,
    notification_events,
    events,
    matches,
    tickets,
    federated_chat_receipts,
    federated_queue,
    federated_site_reports,
    federated_queue_members,
    federated_talent_levels,
    native_rejections,
    native_settlements,
    native_offer_seats,
    spectator_sessions,
    pooled_spectators,
    community_players,
    community_friend_requests,
    community_friends,
    community_items,
    community_inventory,
    community_equipment,
    community_item_grants,
    community_quests,
    community_quest_states,
    community_quest_progress,
    community_redemptions,
    community_members,
    community_groups,
    community_invites,
    community_party_queues,
    ce_preferences,
    ce_metrics,
    ce_scores,
    ce_messages,
    ce_match_handles,
    ce_honor,
    ce_pending_gifts,
    ce_wait,
    ce_wait_history,
    ce_provider_orders,
    ce_provider_identity,
    ce_migrations,
    ce_completed,
    ce_player_matches,
    ce_daily_wins,
    ce_hero_matches,
);
