//! Public synthetic values serialized by the original client SDK in LEAGUE05.
//! All 15 outputs matched independent Python packing before export. These bytes
//! contain invented inputs only, not game instructions or real account traffic.
use bo2_server::league_requests::{
    Request,
    Value::{Null, Present},
    parse_request,
};

const VECTORS: &[(&str, &str)] = &[
    (
        "team_lookup",
        "5103016e08100000000200000008070605040302018877665544332211",
    ),
    (
        "team_list",
        "5103020a080706050403020103020807000000080c000000",
    ),
    (
        "subdivisions",
        "5103030a08070605040302016e08100000000200000008070605040302018877665544332211",
    ),
    (
        "rename",
        "5103040a08070605040302011053796e746865746963205465616d00",
    ),
    (
        "team_info",
        "5103066e08100000000200000008070605040302018877665544332211",
    ),
    (
        "team_members",
        "5103086e08100000000200000008070605040302018877665544332211",
    ),
    (
        "history_subdivisions",
        "5103146e08100000000200000008070605040302018877665544332211",
    ),
    (
        "season_history",
        "5103150a08070605040302010a88776655443322116e08100000000200000008070605040302018877665544332211",
    ),
    (
        "stats_upload",
        "5203010a08070605040302010a887766554433221103030aeb32a4f8ffffffff07efffffff07020000000703000000070400000007050000000706000000070700000007080000000d0000a03f0d000020c00d00007040",
    ),
    (
        "stats_by_team",
        "5203026e081000000002000000080706050403020188776655443322110a8877665544332211",
    ),
    ("ladder", "5203030a88776655443322110807000000080c000000"),
    (
        "ranked_team",
        "5203040a08070605040302010a8877665544332211080c000000",
    ),
    (
        "stats_upload_all_nulls",
        "52030114140300141414141414141414141414",
    ),
    ("team_lookup_empty", "5103016e080000000000000000"),
    (
        "subdivision_null_team",
        "510303146e08100000000200000008070605040302018877665544332211",
    ),
];

fn hex(value: &str) -> Vec<u8> {
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    assert!(remainder.is_empty());
    pairs
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn original_sdk_requests_are_accepted_and_nulls_preserved() {
    for (name, encoded) in VECTORS {
        let bytes = hex(encoded);
        assert_eq!(bytes[1], 3);
        let request = parse_request(bytes[0], bytes[2], &bytes[3..])
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        match *name {
            "stats_upload" => {
                let Request::UploadStats(s) = request else {
                    panic!("wrong variant")
                };
                assert_eq!(s.team_id, Present(0x0102_0304_0506_0708));
                assert_eq!(s.subdivision_id, Present(0x1122_3344_5566_7788));
                assert_eq!(s.field_18, 3);
                assert_eq!(s.rank_points_bits, Present((-123456789_i64) as u64));
                assert_eq!(s.counters, [-17, 2, 3, 4, 5, 6, 7, 8].map(Present));
                assert_eq!(
                    (s.skill, s.variance, s.placement_skill),
                    (Present(1.25), Present(-2.5), Present(3.75))
                );
            }
            "stats_upload_all_nulls" => {
                let Request::UploadStats(s) = request else {
                    panic!("wrong variant")
                };
                assert_eq!(
                    (s.team_id, s.subdivision_id, s.rank_points_bits),
                    (Null, Null, Null)
                );
                assert_eq!(s.field_18, 0);
                assert_eq!(s.counters, [Null; 8]);
                assert_eq!((s.skill, s.variance, s.placement_skill), (Null, Null, Null));
            }
            "team_lookup_empty" => assert_eq!(request, Request::TeamId { member_ids: vec![] }),
            "subdivision_null_team" => assert_eq!(
                request,
                Request::Subdivisions {
                    team_id: Null,
                    league_ids: vec![0x0102_0304_0506_0708, 0x1122_3344_5566_7788]
                }
            ),
            _ => {}
        }
    }
}

#[test]
fn authenticated_native_request_bodies_reach_parser_with_nonzero_padding() {
    use bo2_server::league_requests::parse_task_payload;
    use bo2_server::protocol::{CANDIDATE_KEY, decode_client, encode_client_fixture};
    for (name, encoded) in VECTORS {
        let task = hex(encoded);
        let expected = parse_request(task[0], task[2], &task[3..]).unwrap();
        // Real framing (console-confirmed): service byte sits at plaintext
        // byte 4, then tag/op/body and a 0x00 terminator before padding.
        let mut payload = task.clone();
        payload.push(0);
        // Existing independently verified crypto codec; fixture padding is seed.
        let wire = encode_client_fixture(&payload, &CANDIDATE_KEY, 42).unwrap();
        let verified = decode_client(&wire[4..], &CANDIDATE_KEY).unwrap();
        assert_eq!(verified.inner_type, task[0]);
        let mut data = vec![verified.inner_type];
        data.extend_from_slice(&verified.payload_with_padding);
        assert_eq!(parse_task_payload(&data).unwrap(), expected, "{name}");
        // Even a MAC-valid extra block cannot be accepted as minimal padding.
        payload.extend_from_slice(&[0; 8]);
        let excess = encode_client_fixture(&payload, &CANDIDATE_KEY, 43).unwrap();
        let verified = decode_client(&excess[4..], &CANDIDATE_KEY).unwrap();
        let mut data = vec![verified.inner_type];
        data.extend_from_slice(&verified.payload_with_padding);
        assert!(parse_task_payload(&data).is_err(), "{name}");
    }
}
