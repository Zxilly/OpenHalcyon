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
    federated_queue_members (ticket, player) {
        ticket -> Text,
        player -> Text,
        pos -> BigInt,
        accepted_match -> Nullable<Text>,
        released -> Bool,
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
    events (cursor) {
        cursor -> BigInt,
        player -> Text,
        body -> Text,
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
    community_party_queues (party) {
        party -> Text,
        ticket -> Text,
    }
}

diesel::joinable!(federated_queue_members -> federated_queue (ticket));
diesel::joinable!(tickets -> matches (match_id));
diesel::joinable!(spectator_sessions -> matches (match_id));
diesel::allow_tables_to_appear_in_same_query!(federated_queue_members, federated_queue);
diesel::allow_tables_to_appear_in_same_query!(tickets, matches);
diesel::allow_tables_to_appear_in_same_query!(spectator_sessions, matches);
diesel::allow_tables_to_appear_in_same_query!(community_party_queues, tickets);

diesel::table! {
    native_offer_seats (room, player) {
        room -> Text,
        player -> Text,
        entry -> Text,
        position -> BigInt,
        accepted -> Bool,
    }
}
