diesel::table! { metadata(key) { key -> Text, data -> Binary, } }
diesel::table! { log(idx) { idx -> BigInt, data -> Binary, } }
diesel::table! { sm_checkpoint(id) { id -> Integer, meta -> Nullable<Binary>, image -> Binary, } }
diesel::table! { sm_applied(idx) { idx -> BigInt, entry -> Binary, } }
