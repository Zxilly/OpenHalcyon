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
