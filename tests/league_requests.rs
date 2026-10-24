//! Invented request values packed independently with Python struct, not game
//! bytes or captured accounts. The parser is not used to generate these vectors.
//! q(n)=0a+pack('<Q',n); u(n)=08+pack('<I',n);
//! i(n)=07+pack('<i',n); f(n)=0d+pack('<f',n);
//! a=[6e]+u(24)+pack('<IQQQ',3,7,3,9). No array-end byte.
use bo2_server::league_requests::{
    MAX_ARRAY_ITEMS, MAX_BODY_BYTES, MAX_REQUESTED_ROWS, Request, StatsUpload,
    Value::{Null, Present},
    parse_request, parse_task_payload,
};

const VECTORS: &[(u8, u8, &str)] = &[
    (
        0x51,
        0x1,
        "6e081800000003000000070000000000000003000000000000000900000000000000",
    ),
    (0x51, 0x2, "0a2a0000000000000003050817000000080c000000"),
    (
        0x51,
        0x3,
        "0a2a000000000000006e081800000003000000070000000000000003000000000000000900000000000000",
    ),
    (0x51, 0x4, "0a2a00000000000000105465737400"),
    (
        0x51,
        0x6,
        "6e081800000003000000070000000000000003000000000000000900000000000000",
    ),
    (
        0x51,
        0x8,
        "6e081800000003000000070000000000000003000000000000000900000000000000",
    ),
    (
        0x51,
        0x14,
        "6e081800000003000000070000000000000003000000000000000900000000000000",
    ),
    (
        0x51,
        0x15,
        "0a2a000000000000000a54000000000000006e081800000003000000070000000000000003000000000000000900000000000000",
    ),
    (
        0x52,
        0x1,
        "0a2a000000000000000a540000000000000003060a7b0000000000000007efffffff07020000000703000000070400000007050000000706000000070700000007080000000d0000a03f0d000020c00d00007040",
    ),
    (
        0x52,
        0x2,
        "6e0818000000030000000700000000000000030000000000000009000000000000000a5400000000000000",
    ),
    (0x52, 0x3, "0a54000000000000000803000000080c000000"),
    (0x52, 0x4, "0a2a000000000000000a54000000000000000801000000"),
];

fn hex(value: &str) -> Vec<u8> {
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    assert!(remainder.is_empty());
    pairs
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn all_known_requests_match_independent_vectors_and_preserve_member_order() {
    let ids = vec![7, 3, 9];
    let expected = [
        Request::TeamId {
            member_ids: ids.clone(),
        },
        Request::TeamsList {
            field_00: Present(42),
            field_01: 5,
            field_02: Present(23),
            field_03: Present(12),
        },
        Request::Subdivisions {
            team_id: Present(42),
            league_ids: ids.clone(),
        },
        Request::Rename {
            team_id: Present(42),
            name: Present(b"Test".to_vec()),
        },
        Request::TeamInfo {
            team_ids: ids.clone(),
        },
        Request::TeamMembers {
            team_ids: ids.clone(),
        },
        Request::HistorySubdivisions {
            subdivision_ids: ids.clone(),
        },
        Request::SeasonHistory {
            team_id: Present(42),
            league_id: Present(84),
            season_ids: ids.clone(),
        },
        Request::UploadStats(StatsUpload {
            team_id: Present(42),
            subdivision_id: Present(84),
            field_18: 6,
            rank_points_bits: Present(123),
            counters: [-17, 2, 3, 4, 5, 6, 7, 8].map(Present),
            skill: Present(1.25),
            variance: Present(-2.5),
            placement_skill: Present(3.75),
        }),
        Request::Stats {
            team_ids: ids,
            subdivision_id: Present(84),
        },
        Request::Ladder {
            subdivision_id: Present(84),
            field_01: Present(3),
            limit: Present(12),
        },
        Request::RankedStats {
            team_id: Present(42),
            subdivision_id: Present(84),
            limit: Present(1),
        },
    ];
    for ((service, operation, wire), want) in VECTORS.iter().zip(expected) {
        assert_eq!(
            parse_request(*service, *operation, &hex(wire)).unwrap(),
            want
        );
    }
}

#[test]
fn nulls_are_not_zero_or_invented_update_operations() {
    // u64 null, u64 null, nonnullable u8=0, u64 null, eight i32 nulls,
    // three f32 nulls. Every nullable field is an explicit 14 tag.
    let mut upload = hex("1414030014");
    upload.extend([20; 11]);
    assert_eq!(
        parse_request(0x52, 1, &upload).unwrap(),
        Request::UploadStats(StatsUpload {
            team_id: Null,
            subdivision_id: Null,
            field_18: 0,
            rank_points_bits: Null,
            counters: [Null; 8],
            skill: Null,
            variance: Null,
            placement_skill: Null,
        })
    );
    assert_eq!(
        parse_request(0x51, 2, &hex("1403001414")).unwrap(),
        Request::TeamsList {
            field_00: Null,
            field_01: 0,
            field_02: Null,
            field_03: Null,
        }
    );
    assert_eq!(
        parse_request(0x51, 4, &hex("1414")).unwrap(),
        Request::Rename {
            team_id: Null,
            name: Null
        }
    );
    assert_eq!(
        parse_request(0x52, 4, &hex("141414")).unwrap(),
        Request::RankedStats {
            team_id: Null,
            subdivision_id: Null,
            limit: Null,
        }
    );
    // A tagged present all-ones bit pattern is distinct from the null tag.
    assert_eq!(
        parse_request(0x51, 4, &hex("0affffffffffffffff1000")).unwrap(),
        Request::Rename {
            team_id: Present(u64::MAX),
            name: Present(vec![]),
        }
    );
    // The upload byte cannot use a nullable tag.
    upload[2] = 20;
    assert!(parse_request(0x52, 1, &upload).is_err());
}

#[test]
fn every_prefix_truncation_and_trailing_byte_is_rejected() {
    for (service, operation, wire) in VECTORS {
        let mut body = hex(wire);
        for len in 0..body.len() {
            assert!(
                parse_request(*service, *operation, &body[..len]).is_err(),
                "{service:x}/{operation:x} prefix{len}"
            );
        }
        body.push(0); // padding is not part of an exact request body
        assert!(parse_request(*service, *operation, &body).is_err());
    }
    assert!(parse_request(0x51, 5, &[]).is_err());
    assert!(parse_request(0x53, 1, &[]).is_err());
}

#[test]
fn arrays_validate_type_length_count_and_owned_budget_before_allocation() {
    assert_eq!(
        parse_request(0x51, 1, &hex("6e080000000000000000")).unwrap(),
        Request::TeamId { member_ids: vec![] }
    );
    for malformed in [
        "6d0808000000010000000100000000000000", // wrong element marker
        "6e0708000000010000000100000000000000", // wrong length type
        "6e0807000000010000000100000000000000", // byte/count mismatch
        "6e0808000000020000000100000000000000", // count mismatch
        "6e08ffffffffffffffff",                 // multiplication/large count
        "6e080800000001000000ffffffffffffffff", // no untagged null representation
        "6e0800000000000000006f",               // no array-end byte
    ] {
        assert!(
            parse_request(0x51, 1, &hex(malformed)).is_err(),
            "{malformed}"
        );
    }
    let mut body = vec![110, 8];
    body.extend(((MAX_ARRAY_ITEMS * 8) as u32).to_le_bytes());
    body.extend((MAX_ARRAY_ITEMS as u32).to_le_bytes());
    body.extend(vec![0; MAX_ARRAY_ITEMS * 8]);
    assert!(parse_request(0x51, 1, &body).is_ok());
    body[2..6].copy_from_slice(&(((MAX_ARRAY_ITEMS + 1) * 8) as u32).to_le_bytes());
    body[6..10].copy_from_slice(&((MAX_ARRAY_ITEMS + 1) as u32).to_le_bytes());
    body.extend([0; 8]);
    assert!(parse_request(0x51, 1, &body).is_err());
    assert!(parse_request(0x51, 1, &vec![0; MAX_BODY_BYTES + 1]).is_err());
}

#[test]
fn rename_bound_is_bytes_including_wire_nul_not_unicode_characters() {
    let mut body = hex("0a010000000000000010");
    body.extend([0xff; 63]); // raw non-UTF8 is a wire value, not a name policy
    body.push(0);
    assert!(parse_request(0x51, 4, &body).is_ok());
    body.insert(body.len() - 1, b'x');
    assert!(parse_request(0x51, 4, &body).is_err());
    body.truncate(body.len() - 1);
    assert!(parse_request(0x51, 4, &body).is_err());
}

#[test]
fn upload_types_and_owned_float_row_budgets_are_enforced() {
    let original = hex(VECTORS[8].2);
    let mut wrong = original.clone();
    wrong[20] = 9; // signed response rating tag is wrong in upload
    assert!(parse_request(0x52, 1, &wrong).is_err());
    for field in 0..3 {
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut body = original.clone();
            let offset = body.len() - 15 + field * 5 + 1;
            body[offset..offset + 4].copy_from_slice(&invalid.to_le_bytes());
            assert!(parse_request(0x52, 1, &body).is_err());
        }
    }
    let mut rows = hex(VECTORS[11].2);
    let at = rows.len() - 4;
    rows[at..].copy_from_slice(&MAX_REQUESTED_ROWS.to_le_bytes());
    assert!(parse_request(0x52, 4, &rows).is_ok());
    rows[at..].copy_from_slice(&(MAX_REQUESTED_ROWS + 1).to_le_bytes());
    assert!(parse_request(0x52, 4, &rows).is_err());
}

#[test]
fn task_adapter_accepts_all_minimal_padding_lengths_without_trimming_fields() {
    let mut seen = [false; 8];
    for name_len in 0..8 {
        // Independent body layout: tagged u64(42), tag16, name, NUL.
        let mut body = hex("0a2a0000000000000010");
        body.extend(vec![b'x'; name_len]);
        body.push(0);
        let mut task = vec![0x51, 3, 4];
        task.extend_from_slice(&body);
        // Real client tasks: body, a 0x00 terminator, then minimal padding.
        let padding = bo2_server::protocol::task_padding(task.len() + 1);
        seen[padding] = true;
        task.push(0);
        task.extend((0..padding).map(|n| 0xa1 + n as u8));
        assert_eq!(
            parse_task_payload(&task).unwrap(),
            Request::Rename {
                team_id: Present(42),
                name: Present(vec![b'x'; name_len]),
            }
        );
        // An extra whole block is never minimal padding.
        task.extend([0x7b; 8]);
        assert!(parse_task_payload(&task).is_err());
        if padding != 0 {
            let mut exact_with_padding = body.clone();
            exact_with_padding.extend(vec![0xa1; padding]);
            assert!(parse_request(0x51, 4, &exact_with_padding).is_err());
        }
    }
    assert!(seen.into_iter().all(|v| v));
}

#[test]
fn task_adapter_preserves_every_vector_with_nonzero_padding_and_trailing_zero_fields() {
    for (service, operation, wire) in VECTORS {
        let body = hex(wire);
        let expected = parse_request(*service, *operation, &body).unwrap();
        let mut task = vec![*service, 3, *operation];
        task.extend_from_slice(&body);
        let padding = bo2_server::protocol::task_padding(task.len() + 1);
        task.push(0);
        task.extend(vec![0xda; padding]);
        assert_eq!(parse_task_payload(&task).unwrap(), expected);
        // Unlike trimming, this cannot remove trailing zero bytes of actual
        // LE64/LE32 fields or the string terminator in the original body.
        // Frame = 4 MAC bytes + task data, a multiple of 8.
        assert_eq!((task.len() + 4) % 8, 0);
        task.pop();
        assert!(parse_task_payload(&task).is_err());
    }
}

#[test]
fn task_adapter_rejects_bad_header_unknown_operation_and_bad_bounds() {
    for malformed in [
        vec![],
        vec![0x51],
        vec![0x51, 3],
        vec![0x51, 8, 1],
        vec![0x51, 3, 5],
        vec![0x53, 3, 1],
    ] {
        assert!(parse_task_payload(&malformed).is_err());
    }
    let mut body = hex(VECTORS[1].2);
    let padding = bo2_server::protocol::task_padding(3 + body.len() + 1);
    body.push(0);
    body.extend(vec![0; padding]);
    // A correct body cannot compensate for a malformed operation tag.
    let mut wrong_header = vec![0x51, 8, 2];
    wrong_header.extend_from_slice(&body);
    assert!(parse_task_payload(&wrong_header).is_err());
    assert!(parse_task_payload(&vec![0; MAX_BODY_BYTES + 11]).is_err());
}
