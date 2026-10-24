//! Stats result bodies equal the payloads the original client deserializers
//! accepted in emulation (column rows 0x89120c, wrappers 0x826dd0; LEAGUE06
//! stats-results). Invented values only.
use bo2_server::{league_store::LeaderboardRow, stats};
use serde_json::Value;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn row(v: &Value) -> (LeaderboardRow, Vec<u8>) {
    let a = v.as_array().unwrap();
    let row = LeaderboardRow {
        leaderboard: 0,
        entity: a[0].as_u64().unwrap(),
        rating: a[1].as_i64().unwrap(),
        rank: a[2].as_u64().unwrap(),
        columns: a[4]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_i64().unwrap() as i32)
            .collect(),
    };
    (row, a[3].as_str().unwrap().as_bytes().to_vec())
}

#[test]
fn encoders_match_emulator_accepted_payloads() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/stats-vectors.json")).unwrap();
    let vectors = fixture["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 3);
    for v in vectors {
        let expected = hex(v["payload"].as_str().unwrap());
        let mut got = vec![];
        match v["kind"].as_str().unwrap() {
            "rows" => {
                for r in v["rows"].as_array().unwrap() {
                    let (row, name) = row(r);
                    got.extend(stats::row_result(&row, &name).unwrap());
                }
            }
            "wrappers" => {
                for w in v["wrappers"].as_array().unwrap() {
                    let lb = w[0].as_u64().unwrap() as u32;
                    let entity = w[1].as_u64().unwrap();
                    let present = (!w[2].is_null()).then(|| row(&w[2]));
                    got.extend(
                        stats::wrapper_result(
                            lb,
                            entity,
                            present.as_ref().map(|(r, n)| (r, n.as_slice())),
                        )
                        .unwrap(),
                    );
                }
            }
            other => panic!("unknown vector kind {other}"),
        }
        assert_eq!(got, expected);
    }
}
