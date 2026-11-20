diesel::table! {
    peer_identity (singleton) {
        singleton -> Integer,
        pool -> Text,
        node -> Text,
        sequence -> BigInt,
    }
}

diesel::table! {
    hosted (id) {
        id -> Text,
        address -> Text,
        result -> Nullable<Text>,
        delivered -> Integer,
    }
}

diesel::table! {
    outgoing_chat (id, home) {
        id -> Text,
        home -> Text,
        body -> Text,
        expires -> BigInt,
        next_attempt -> BigInt,
        // SQLite's implicit rowid is the legacy enqueue order.
        rowid -> BigInt,
    }
}

diesel::table! {
    enqueue_payloads (id) {
        id -> Text,
        payload -> Text,
    }
}

diesel::define_sql_function! {
    fn unixepoch() -> diesel::sql_types::BigInt;
}

diesel::define_sql_function! {
    #[sql_name = "unixepoch"]
    fn unixepoch_after(time: diesel::sql_types::Text, modifier: diesel::sql_types::Text)
        -> diesel::sql_types::BigInt;
}
