//! Storage service 0x0a: request layouts from the client builders and the
//! startup reply policy (LEAGUE06). Invented file contents only.
use bo2_server::{
    league_config,
    league_store::Store,
    protocol,
    storage::{
        self, ERROR_NO_FILE, PATCH_DECLINE, Request, blob_result, is_patch_name, parse_task_payload,
    },
};

fn string(out: &mut Vec<u8>, s: &str) {
    out.push(16);
    out.extend(s.as_bytes());
    out.push(0);
}
fn u64v(out: &mut Vec<u8>, v: u64) {
    out.push(10);
    out.extend(v.to_le_bytes());
}
fn u32v(out: &mut Vec<u8>, v: u32) {
    out.push(8);
    out.extend(v.to_le_bytes());
}
fn u16v(out: &mut Vec<u8>, v: u16) {
    out.push(6);
    out.extend(v.to_le_bytes());
}

/// `0x0a, 3, op, body` plus minimal CBC padding, as decode_client yields it.
fn task(op: u8, body: &[u8]) -> Vec<u8> {
    let mut data = vec![0x0a, 3, op];
    data.extend(body);
    let used = data.len();
    data.push(0);
    let pad = bo2_server::protocol::task_padding(used + 1);
    data.extend(std::iter::repeat_n(0xa5, pad));
    data
}

#[test]
fn client_request_layouts_parse() {
    let mut b = vec![];
    string(&mut b, "example_online_english.bundle");
    assert_eq!(
        parse_task_payload(&task(7, &b)).unwrap(),
        Request::PublisherGet {
            filename: "example_online_english.bundle".into()
        }
    );
    let mut b = vec![];
    u32v(&mut b, 0);
    u16v(&mut b, 1);
    u16v(&mut b, 0);
    b.push(20); // null filename
    assert_eq!(
        parse_task_payload(&task(6, &b)).unwrap(),
        Request::PublisherList { filename: None }
    );
    let mut b = vec![];
    string(&mut b, "player_state");
    u64v(&mut b, 0);
    assert_eq!(
        parse_task_payload(&task(3, &b)).unwrap(),
        Request::UserGet {
            filename: "player_state".into(),
            owner: 0
        }
    );
    let mut b = vec![];
    u64v(&mut b, 0x1234);
    u32v(&mut b, 0);
    u16v(&mut b, 50);
    u16v(&mut b, 0);
    b.push(20);
    assert_eq!(
        parse_task_payload(&task(5, &b)).unwrap(),
        Request::UserList { owner: 0x1234 }
    );
    // Upload with and without the optional trailing u64.
    for trailing in [false, true] {
        let mut b = vec![];
        string(&mut b, "player_state");
        b.extend([1, 0]);
        b.push(19);
        u32v(&mut b, 3);
        b.extend([7, 8, 9]);
        if trailing {
            u64v(&mut b, 99);
        }
        assert_eq!(
            parse_task_payload(&task(1, &b)).unwrap(),
            Request::Upload {
                filename: "player_state".into(),
                data: vec![7, 8, 9]
            }
        );
    }
}

#[test]
fn malformed_requests_fail_closed() {
    let mut b = vec![];
    string(&mut b, "../escape");
    assert!(parse_task_payload(&task(7, &b)).is_err());
    let mut b = vec![];
    string(&mut b, "ok");
    let mut data = task(7, &b);
    data.extend([0; 8]); // an extra full padding block
    assert!(parse_task_payload(&data).is_err());
    assert!(parse_task_payload(&task(9, &[])).is_err());
    let mut b = vec![];
    string(&mut b, "big");
    b.extend([1, 1, 19]);
    u32v(&mut b, storage::MAX_USER_FILE_BYTES as u32 + 1);
    assert!(parse_task_payload(&task(1, &b)).is_err());
}

#[test]
fn file_name_classes() {
    assert!(is_patch_name("optional_patch.00"));
    assert!(is_patch_name("x.07"));
    assert!(!is_patch_name("example_online_english.bundle"));
    assert!(!is_patch_name("a.0"));
    assert!(common::profile().is_bundle_name("example_online_french.bundle"));
    assert!(!common::profile().is_bundle_name("other_online_english.bundle"));
}

fn reply(store: &mut Store, data: &[u8], wad: &[u8]) -> Vec<u8> {
    storage::handle_task(
        store,
        0x42,
        9,
        data,
        1_800_000_000,
        &bo2_server::profile::PublisherAssets {
            bundle: wad.to_vec(),
            prefix: common::profile().bundle_prefix,
            suffix: common::profile().bundle_suffix,
        },
    )
    .unwrap()
}

#[test]
fn startup_and_new_player_storage_flow() {
    let mut store = Store::open_in_memory().unwrap();
    let wad = league_config::online_wad(&common::profile(), league_config::WAD_TIMESTAMP).unwrap();
    let mut b = vec![];
    string(&mut b, "example_online_english.bundle");
    assert_eq!(
        reply(&mut store, &task(7, &b), &wad),
        protocol::results_reply(9, 7, &[blob_result(&wad)]).unwrap()
    );
    let mut b = vec![];
    string(&mut b, "optional_patch.00");
    assert_eq!(
        reply(&mut store, &task(7, &b), &wad),
        protocol::results_reply(9, 7, &[blob_result(&PATCH_DECLINE)]).unwrap()
    );
    let mut b = vec![];
    string(&mut b, "optional_heatmap");
    assert_eq!(
        reply(&mut store, &task(7, &b), &wad),
        protocol::error_reply(9, ERROR_NO_FILE).unwrap()
    );
    // New player: no stats file yet -> error, so the client installs defaults.
    let mut get = vec![];
    string(&mut get, "player_state");
    u64v(&mut get, 0);
    assert_eq!(
        reply(&mut store, &task(3, &get), &wad),
        protocol::error_reply(9, ERROR_NO_FILE).unwrap()
    );
    let mut up = vec![];
    string(&mut up, "player_state");
    up.extend([1, 0, 19]);
    u32v(&mut up, 4);
    up.extend([1, 2, 3, 4]);
    assert_eq!(
        reply(&mut store, &task(1, &up), &wad),
        protocol::empty_success(9, 1)
    );
    // Read-after-write returns the exact uploaded bytes, by owner 0 or by ID.
    let expected = protocol::results_reply(9, 3, &[blob_result(&[1, 2, 3, 4])]).unwrap();
    assert_eq!(reply(&mut store, &task(3, &get), &wad), expected);
    let mut by_id = vec![];
    string(&mut by_id, "player_state");
    u64v(&mut by_id, 0x42);
    assert_eq!(reply(&mut store, &task(3, &by_id), &wad), expected);
}

mod common;
