diesel::table! {
    federated_queue (ticket) {
        ticket -> Text,
        phase -> Text,
        match_id -> Nullable<Text>,
    }
}

diesel::table! {
    federated_queue_members (ticket, player) {
        ticket -> Text,
        player -> Text,
        released -> Bool,
    }
}

diesel::table! {
    tickets (serial) {
        serial -> BigInt,
        player -> Text,
        match_id -> Nullable<Text>,
    }
}

diesel::table! {
    matches (id) {
        id -> Text,
        phase -> Text,
        offered_at -> BigInt,
    }
}

diesel::table! {
    events (cursor) {
        cursor -> BigInt,
        player -> Text,
        body -> Text,
    }
}

diesel::joinable!(federated_queue_members -> federated_queue (ticket));
diesel::joinable!(tickets -> matches (match_id));
diesel::allow_tables_to_appear_in_same_query!(federated_queue_members, federated_queue);
diesel::allow_tables_to_appear_in_same_query!(tickets, matches);

diesel::table! {
    native_offer_seats (room, player) {
        room -> Text,
        player -> Text,
        entry -> Text,
        position -> BigInt,
        accepted -> Bool,
    }
}
