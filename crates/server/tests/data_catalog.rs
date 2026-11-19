use halcyon_data::GameData;
use serde_json::json;
use std::{fs, process::Command};

#[test]
fn exported_catalog_loads_on_a_peer_without_the_trace() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("data");
    fs::create_dir(&dir).unwrap();
    for name in [
        "units",
        "items",
        "entity_manifest",
        "rules",
        "ability_raw",
        "official_rules",
        "combat_constants",
    ] {
        fs::write(dir.join(format!("{name}.json")), "[]").unwrap();
    }
    let catalog_record = |name: &str| {
        let field = format!(
            "{:0<76}",
            name.as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        json!({"dir":"S2C", "opcode":0x453, "payload":field}).to_string()
    };
    let trace = root.path().join("match_decoded.jsonl");
    fs::write(
        &trace,
        [
            json!({"dir":"C2S", "opcode":0x453, "payload":"private"}).to_string(),
            catalog_record("*Hero*"),
            json!({"dir":"S2C", "opcode":0x3ed, "handle":"private"}).to_string(),
            catalog_record("Hero_Skin_Snow"),
        ]
        .join("\n"),
    )
    .unwrap();
    let original = GameData::load(&dir).unwrap();
    let export = Command::new(env!("CARGO_BIN_EXE_openhalcyon"))
        .arg("export-catalog")
        .arg("--trace")
        .arg(&trace)
        .output()
        .unwrap();
    assert!(
        export.status.success(),
        "{}",
        String::from_utf8_lossy(&export.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Vec<String>>(&export.stdout).unwrap(),
        ["*Hero*", "Hero_Skin_Snow"]
    );
    fs::write(dir.join("catalog.json"), export.stdout).unwrap();
    fs::remove_file(trace).unwrap();
    let peer = GameData::load(&dir).unwrap();
    assert_eq!(peer.content_digest, original.content_digest);
    assert_eq!(peer.hero_names, ["Hero"]);
    assert_eq!(peer.skin_names, ["Hero_Skin_Snow"]);
}
