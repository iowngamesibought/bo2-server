//! Public synthetic vectors, packed independently in Python and accepted by the
//! original console result decoders in LEAGUE04. No game bytes or real user data.
use bo2_server::league::{StatsResult, subdivision_result, team_id_result};

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn stats() -> StatsResult {
    StatsResult {
        team_id: 0x0102_0304_0506_0708,
        subdivision_id: 0x1122_3344_5566_7788,
        name: b"Synthetic Team".to_vec(),
        field_58: 23,
        rank: 29,
        rank_points: -123456789,
        real_points: -17,
        played: 2,
        wins: 3,
        losses: 4,
        bonus_used: 5,
        bonus_time: 6,
        streak: 7,
        career_wins: 8,
        skill: 1.25,
        variance: -2.5,
        placement_skill: 3.75,
    }
}

#[test]
fn result_bodies_match_original_client_accepted_vectors() {
    assert_eq!(
        team_id_result(0x0102_0304_0506_0708),
        hex("0a0807060504030201")
    );
    assert_eq!(
        subdivision_result(
            0x0102_0304_0506_0708,
            0x1122_3344_5566_7788,
            0x2233_4455_6677_8899,
            b"3,2,17,4\0",
        )
        .unwrap(),
        hex("0a08070605040302010a88776655443322110a9988776655443322130809000000332c322c31372c3400")
    );
    assert_eq!(
        stats().encode().unwrap(),
        hex(concat!(
            "0a08070605040302010a88776655443322111053796e746865746963205465616d00",
            "0817000000081d00000009eb32a4f8ffffffff07efffffff07020000000703000000",
            "070400000007050000000706000000070700000007080000000d0000a03f0d000020c00d00007040"
        ))
    );
}

#[test]
fn object_capacity_and_name_terminator_are_enforced() {
    assert!(subdivision_result(1, 2, 3, &[b'x'; 64]).is_ok());
    assert!(subdivision_result(1, 2, 3, &[b'x'; 65]).is_err());
    let mut value = stats();
    value.name = vec![b'x'; 64];
    assert!(value.encode().is_ok());
    value.name.push(b'x');
    assert!(value.encode().is_err());
    value.name = b"ab\0cd".to_vec();
    assert!(value.encode().is_err());
    // Limit is bytes, including for non-ASCII UTF-8 supplied by an adapter.
    value.name = "é".repeat(33).into_bytes();
    assert!(value.encode().is_err());
}

#[test]
fn nonfinite_skill_is_rejected_without_conflating_skill_and_points() {
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for index in 0..3 {
            let mut value = stats();
            match index {
                0 => value.skill = invalid,
                1 => value.variance = invalid,
                _ => value.placement_skill = invalid,
            }
            assert!(value.encode().is_err());
        }
    }
    // This is only wire encoding: it does not prescribe a ranking policy or
    // reject a negative finite synthetic variance as if the client did so.
    assert!(stats().encode().is_ok());
}
