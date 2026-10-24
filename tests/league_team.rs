//! Public synthetic vectors, packed independently in Python and accepted by the
//! original console result decoders in LEAGUE06. No game bytes or real user data.
use bo2_server::league::{
    HistorySubdivision, MAX_TEAM_MEMBERS, TeamMember, TeamRecord, season_history_result,
    team_members_result,
};
use serde_json::Value;

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn u64_of(value: &Value) -> u64 {
    u64::from_str_radix(value.as_str().unwrap(), 16).unwrap()
}

fn u32_of(value: &Value) -> u32 {
    u32::try_from(value.as_u64().unwrap()).unwrap()
}

fn bytes_of(value: &Value) -> Vec<u8> {
    hex(value.as_str().unwrap())
}

fn team_record(input: &Value) -> TeamRecord {
    TeamRecord {
        field_08: u64_of(&input["field_08"]),
        name: bytes_of(&input["name"]),
        data: bytes_of(&input["data"]),
        field_u32: u32_of(&input["field_u32"]),
    }
}

fn encode(kind: &str, input: &Value) -> Vec<u8> {
    match kind {
        "team_list" | "team_info" | "team_list_max" | "team_info_max" => {
            team_record(input).encode().unwrap()
        }
        "members" | "members_max15" | "members_empty" => {
            let members: Vec<TeamMember> = input["members"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| TeamMember {
                    id: u64_of(&m["id"]),
                    name: bytes_of(&m["name"]),
                })
                .collect();
            team_members_result(u64_of(&input["team_id"]), &members).unwrap()
        }
        "history_subdivision" | "history_false_empty" => HistorySubdivision {
            field_08: u64_of(&input["field_08"]),
            field_10: u64_of(&input["field_10"]),
            field_18: u64_of(&input["field_18"]),
            flag_20: input["flag_20"].as_bool().unwrap(),
            field_24: u32_of(&input["field_24"]),
            metadata: bytes_of(&input["metadata"]),
        }
        .encode()
        .unwrap(),
        "season_history" => {
            season_history_result(u64_of(&input["field_08"]), u64_of(&input["subdivision_id"]))
        }
        other => panic!("unexpected vector kind {other}"),
    }
}

#[test]
fn team_and_history_bodies_match_original_client_accepted_vectors() {
    let text = include_str!("fixtures/league06-results.json");
    let fixture: Value = serde_json::from_str(text).unwrap();
    let vectors = fixture["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 10);
    for vector in vectors {
        let kind = vector["kind"].as_str().unwrap();
        let expected = bytes_of(&vector["accepted_payload"]);
        assert_eq!(encode(kind, &vector["input"]), expected, "{kind}");
    }
}

fn record() -> TeamRecord {
    TeamRecord {
        field_08: 1,
        name: b"Team".to_vec(),
        data: vec![],
        field_u32: 0,
    }
}

#[test]
fn team_record_bounds_are_enforced() {
    let mut value = record();
    value.data = vec![0; 1024];
    assert!(value.encode().is_ok());
    // The original reader stores the declared length but reads only 1024 bytes,
    // desynchronizing the stream; never emit it.
    value.data.push(0);
    assert!(value.encode().is_err());
    let mut value = record();
    value.name = vec![b'x'; 64];
    assert!(value.encode().is_ok());
    value.name.push(b'x');
    assert!(value.encode().is_err());
    value.name = b"a\0b".to_vec();
    assert!(value.encode().is_err());
}

fn member(id: u64) -> TeamMember {
    TeamMember {
        id,
        name: b"m".to_vec(),
    }
}

#[test]
fn member_count_and_names_are_bounded() {
    let full: Vec<_> = (1..=MAX_TEAM_MEMBERS as u64).map(member).collect();
    assert!(team_members_result(1, &full).is_ok());
    // The client stores count 16 but reads no members; never emit it.
    let over: Vec<_> = (1..=16).map(member).collect();
    assert!(team_members_result(1, &over).is_err());
    let mut bad = vec![member(1)];
    bad[0].name = vec![b'x'; 65];
    assert!(team_members_result(1, &bad).is_err());
    bad[0].name = b"x\0".to_vec();
    assert!(team_members_result(1, &bad).is_err());
}

#[test]
fn history_metadata_is_bounded() {
    let mut value = HistorySubdivision {
        field_08: 1,
        field_10: 2,
        field_18: 3,
        flag_20: true,
        field_24: 4,
        metadata: vec![b'1'; 64],
    };
    assert!(value.encode().is_ok());
    value.metadata.push(b'1');
    assert!(value.encode().is_err());
}

#[test]
fn multi_row_envelope_matches_original_accepted_bytes() {
    use bo2_server::{league::StatsResult, protocol::results_reply};
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/league06-results.json")).unwrap();
    let envelope = &fixture["results_envelope"];
    let rows: Vec<Vec<u8>> = envelope["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            StatsResult {
                team_id: u64_of(&row["team_id"]),
                subdivision_id: u64_of(&row["subdivision_id"]),
                name: bytes_of(&row["name"]),
                field_58: 0,
                rank: u32_of(&row["rank"]),
                rank_points: 0,
                real_points: 0,
                played: 0,
                wins: u32_of(&row["wins"]) as i32,
                losses: 0,
                bonus_used: 0,
                bonus_time: 0,
                streak: 0,
                career_wins: 0,
                skill: 0.0,
                variance: 1.0,
                placement_skill: 0.0,
            }
            .encode()
            .unwrap()
        })
        .collect();
    let reply = results_reply(
        u64_of(&envelope["transaction"]),
        envelope["operation"].as_u64().unwrap() as u8,
        &rows,
    )
    .unwrap();
    // The harness payload starts at the envelope's transaction field (after inner type 1).
    assert_eq!(reply[1..], bytes_of(&envelope["accepted_payload"]));
}
