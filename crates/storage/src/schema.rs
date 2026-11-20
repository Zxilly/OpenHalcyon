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
        body -> Text,
    }
}

diesel::joinable!(notification_events -> events (event_cursor));
diesel::allow_tables_to_appear_in_same_query!(notification_events, events);
