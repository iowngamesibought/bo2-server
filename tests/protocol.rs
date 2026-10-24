use bo2_server::protocol::*;

#[test]
fn raw_measurement_failure_uses_the_shared_error_branch() {
    assert_eq!(
        raw_measurement_failure(0x0807060504030201),
        [5, 1, 2, 3, 4, 5, 6, 7, 8, 1, 4, 0]
    );
}

#[test]
fn reply_diagnostics_distinguish_missing_empty_and_data_without_contents() {
    // Independently written type-1 wire bodies; invented transaction/value.
    let missing = [1, 10, 1, 0, 0, 0, 0, 0, 0, 0, 8, 0xe8, 3, 0, 0];
    assert_eq!(
        task_reply_summary(&missing).unwrap(),
        TaskReplySummary {
            error: 1000,
            results: 0,
            result_bytes: 0,
        }
    );
    let empty = [
        1, 10, 1, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 3, 7, 8, 0, 0, 0, 0,
    ];
    assert_eq!(
        task_reply_summary(&empty).unwrap(),
        TaskReplySummary {
            error: 0,
            results: 0,
            result_bytes: 0,
        }
    );
    let value = [
        1, 10, 1, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 3, 7, 8, 1, 0, 0, 0, 8, 1, 0, 0, 0, 19, 8, 2,
        0, 0, 0, 0xab, 0xcd,
    ];
    assert_eq!(
        task_reply_summary(&value).unwrap(),
        TaskReplySummary {
            error: 0,
            results: 1,
            result_bytes: 8,
        }
    );
    for n in 0..27 {
        assert!(task_reply_summary(&value[..n]).is_err());
    }
    let mut invalid = value;
    invalid[23] = 2;
    assert!(task_reply_summary(&invalid).is_err());
    let mut invalid = missing.to_vec();
    invalid.push(0);
    assert!(task_reply_summary(&invalid).is_err());
}

fn fixture(name: &str) -> Vec<u8> {
    let v: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/synthetic.json")).unwrap();
    let s = v[name].as_str().unwrap();
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect()
}
#[test]
fn auth_and_login_match_independent_bit_packing() {
    let auth = fixture("auth_request");
    let request = parse_auth(&auth[4..]).unwrap();
    assert_eq!(request.title, 0x777);
    assert_eq!(request.ticket_version, 0x31000000);
    let token = std::array::from_fn(|i| i as u8);
    assert_eq!(candidate_auth_reply(&token), fixture("auth_reply"));
    let login = fixture("login");
    let parsed = parse_login(&login[12..]).unwrap();
    assert_eq!((parsed.title, parsed.seed), (0, 0));
    assert_eq!(parsed.token, token);
    for n in 0..auth.len() - 4 {
        assert!(parse_auth(&auth[4..4 + n]).is_err());
    }
    for n in 0..login.len() - 12 {
        assert!(parse_login(&login[12..12 + n]).is_err());
    }
    let mut overlong = auth[4..].to_vec();
    overlong.push(0);
    assert!(parse_auth(&overlong).is_err());
}
#[test]
fn reply_envelopes_match_independent_vectors() {
    assert_eq!(ready(1), fixture("ready"));
    let mut body = vec![0];
    body.extend(empty_success(1, 5));
    assert_eq!(frame(&body).unwrap(), fixture("empty_success"));
}
#[test]
fn client_crypto_matches_original_ppc_output() {
    let wire = fixture("original_client_frame");
    let payload = fixture("original_client_payload");
    assert_eq!(
        encode_client_fixture(&payload, &CANDIDATE_KEY, 0).unwrap(),
        wire
    );
    let decoded = decode_client(&wire[4..], &CANDIDATE_KEY).unwrap();
    assert_eq!(decoded.inner_type, payload[0]);
    assert_eq!(decoded.payload_with_padding, &payload[1..]);
    let mut corrupt = wire[4..].to_vec();
    corrupt[8] ^= 1;
    assert!(decode_client(&corrupt, &CANDIDATE_KEY).is_err());
    assert!(decode_client(&wire[4..], &[1; 24]).is_err());
    for n in 0..wire.len() - 4 {
        assert!(decode_client(&wire[4..4 + n], &CANDIDATE_KEY).is_err());
    }
}
#[test]
fn server_crypto_matches_independent_client_accepted_frames() {
    let mut p = vec![4, 10];
    p.extend(0x123456789abcdef0u64.to_le_bytes());
    assert_eq!(
        encode_server(&p, &CANDIDATE_KEY, 42).unwrap(),
        fixture("independent_server_ready")
    );
    let sequence = fixture("independent_server_time_sequence");
    let first_len = u32::from_le_bytes(sequence[..4].try_into().unwrap()) as usize + 4;
    assert_eq!(
        encode_server(
            &time_reply(0x3456789abcdef012, 0x12345678),
            &CANDIDATE_KEY,
            42
        )
        .unwrap(),
        sequence[first_len..]
    );
}
#[test]
fn adversarial_lengths_and_tags_are_rejected() {
    assert!(frame(&vec![0; MAX_FRAME + 1]).is_err());
    assert!(encode_server(&[], &CANDIDATE_KEY, 0).is_err());
    assert!(encode_server(&vec![0; MAX_FRAME], &CANDIDATE_KEY, 0).is_err());
    let mut auth = fixture("auth_request")[4..].to_vec();
    auth[2] ^= 2;
    assert!(parse_auth(&auth).is_err());
    let login = fixture("login");
    let mut extra = login[12..].to_vec();
    extra.push(0);
    assert!(parse_login(&extra).is_err());
}
