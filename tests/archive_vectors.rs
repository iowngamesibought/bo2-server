//! Key archive and counter result bodies equal the payloads the original client
//! deserializers accepted in emulation (0x81c1b0 keys, 0x818a80 counters;
//! LEAGUE06 archive-results). Invented values only.
use bo2_server::archive;
use serde_json::Value;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn encoders_match_emulator_accepted_payloads() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/archive-vectors.json")).unwrap();
    for v in fixture["vectors"].as_array().unwrap() {
        let mut got = vec![];
        for pair in v["values"].as_array().unwrap() {
            let a = pair[0].as_u64().unwrap();
            let b = pair[1].as_i64().unwrap();
            got.extend(match v["kind"].as_str().unwrap() {
                "keys" => archive::key_result(a as u16, b),
                "counters" => archive::counter_result(a as u32, b),
                other => panic!("unknown vector kind {other}"),
            });
        }
        assert_eq!(got, hex(v["payload"].as_str().unwrap()));
    }
}
