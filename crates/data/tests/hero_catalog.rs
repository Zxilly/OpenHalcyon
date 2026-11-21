use halcyon_data::catalog_from_trace;
use halcyon_types::DRAFT_CATALOG_ENTRY_TAG;
use serde::Serialize;
use std::io::{Cursor, ErrorKind};

fn import(payload: &str) -> std::io::Result<Vec<String>> {
    #[derive(Serialize)]
    struct CatalogRow<'a> {
        dir: Direction,
        opcode: u16,
        payload: &'a str,
    }
    #[derive(Serialize)]
    enum Direction {
        #[serde(rename = "S2C")]
        ServerToClient,
    }
    let row = CatalogRow {
        dir: Direction::ServerToClient,
        opcode: DRAFT_CATALOG_ENTRY_TAG,
        payload,
    };
    catalog_from_trace(Cursor::new(serde_json::to_vec(&row)?))
}

#[test]
fn accepts_mixed_case_and_stops_at_the_first_nul() {
    let payload = format!("2a52696E676f2A00{}", "fF".repeat(30));
    assert_eq!(import(&payload).unwrap(), ["*Ringo*"]);
}

#[test]
fn rejects_bad_sizes_non_hex_digits_and_invalid_names() {
    for payload in [
        "0".repeat(75),
        "00".repeat(39),
        format!("4100{}g0", "00".repeat(35)),
        format!("4100{}é", "00".repeat(35)),
        "00".repeat(38),
        "41".repeat(38),
        format!("41204200{}", "00".repeat(34)),
    ] {
        assert_eq!(import(&payload).unwrap_err().kind(), ErrorKind::InvalidData);
    }
}
