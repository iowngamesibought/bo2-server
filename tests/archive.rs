//! Key archive 0x0f and counters 0x17: client request layouts (builders
//! 0x81bbdc, 0x81bf20, 0x818654, 0x818820) with invented values, and the owned
//! persistence policy.
use bo2_server::{
    archive::{self, KeyWrite, Request},
    league_store::Store,
    protocol,
};

const ME: u64 = 0x1111_2222_3333_4444;

fn task(service: u8, op: u8, body: &[u8]) -> Vec<u8> {
    let mut data = vec![service, 3, op];
    data.extend(body);
    let used = data.len();
    data.push(0);
    data.extend(std::iter::repeat_n(0x5a, protocol::task_padding(used + 1)));
    data
}

fn owner_category(out: &mut Vec<u8>, owner: u64, category: u16) {
    out.push(10);
    out.extend(owner.to_le_bytes());
    out.push(6);
    out.extend(category.to_le_bytes());
}

fn write_body(owner: u64, category: u16, keys: &[(u16, i64, u8)]) -> Vec<u8> {
    let mut b = vec![];
    owner_category(&mut b, owner, category);
    for &(key, value, kind) in keys {
        b.push(6);
        b.extend(key.to_le_bytes());
        b.push(9);
        b.extend(value.to_le_bytes());
        b.extend([3, kind]);
    }
    b
}

fn read_body(owner: u64, category: u16, keys: &[u16]) -> Vec<u8> {
    let mut b = vec![];
    owner_category(&mut b, owner, category);
    b.extend([1, 0]);
    for key in keys {
        b.push(6);
        b.extend(key.to_le_bytes());
    }
    b
}

fn call(store: &mut Store, caller: u64, data: &[u8]) -> Vec<u8> {
    archive::handle_task(store, caller, 5, data, 1_800_000_000).unwrap()
}

#[test]
fn request_layouts_parse() {
    assert_eq!(
        archive::parse_task_payload(&task(0x0f, 1, &write_body(ME, 3, &[(1000, 7, 1)]))).unwrap(),
        Request::ArchiveWrite {
            owner: ME,
            category: 3,
            keys: vec![KeyWrite {
                key: 1000,
                value: 7,
                write_type: 1
            }]
        }
    );
    // The League key-archive read seen on a console: own ID, category 3,
    // keys 1000 and 1001.
    assert_eq!(
        archive::parse_task_payload(&task(0x0f, 2, &read_body(ME, 3, &[1000, 1001]))).unwrap(),
        Request::ArchiveRead {
            owner: ME,
            category: 3,
            flag: false,
            keys: vec![1000, 1001]
        }
    );
    let mut add = vec![];
    for (id, delta) in [(4u32, 1i64), (9, -2)] {
        add.push(8);
        add.extend(id.to_le_bytes());
        add.push(9);
        add.extend(delta.to_le_bytes());
    }
    assert_eq!(
        archive::parse_task_payload(&task(0x17, 1, &add)).unwrap(),
        Request::CounterAdd(vec![(4, 1), (9, -2)])
    );
    let mut bad = task(0x0f, 2, &read_body(ME, 3, &[1]));
    bad.extend([0; 8]);
    assert!(archive::parse_task_payload(&bad).is_err());
    assert!(archive::parse_task_payload(&task(0x0f, 9, &[])).is_err());
}

#[test]
fn archive_persists_and_reads_back_in_request_order() {
    let mut store = Store::open_in_memory().unwrap();
    call(
        &mut store,
        ME,
        &task(0x0f, 1, &write_body(ME, 3, &[(1000, 5, 0), (1001, 2, 0)])),
    );
    // Add (the client default), keep-higher, keep-lower.
    call(
        &mut store,
        ME,
        &task(
            0x0f,
            1,
            &write_body(ME, 3, &[(1000, 3, 1), (1001, 1, 2), (1002, 9, 3)]),
        ),
    );
    assert_eq!(
        call(
            &mut store,
            ME,
            &task(0x0f, 2, &read_body(ME, 3, &[1001, 1000, 1002, 1003]))
        ),
        protocol::results_reply(
            5,
            2,
            &[
                archive::key_result(1001, 2),
                archive::key_result(1000, 8),
                archive::key_result(1002, 9),
                archive::key_result(1003, 0)
            ]
        )
        .unwrap()
    );
    // No keys: the whole category. Another category is separate.
    assert_eq!(
        call(&mut store, 7, &task(0x0f, 2, &read_body(ME, 3, &[]))),
        protocol::results_reply(
            5,
            2,
            &[
                archive::key_result(1000, 8),
                archive::key_result(1001, 2),
                archive::key_result(1002, 9)
            ]
        )
        .unwrap()
    );
    assert_eq!(store.archive_value(ME, 4, 1000).unwrap(), None);
    // Writing someone else's archive is refused.
    assert!(
        archive::handle_task(
            &mut store,
            7,
            5,
            &task(0x0f, 1, &write_body(ME, 3, &[(1, 1, 0)])),
            0
        )
        .is_err()
    );
}

#[test]
fn counters_are_global_sums() {
    let mut store = Store::open_in_memory().unwrap();
    let add = |store: &mut Store, caller: u64, id: u32, delta: i64| {
        let mut b = vec![8];
        b.extend(id.to_le_bytes());
        b.push(9);
        b.extend(delta.to_le_bytes());
        call(store, caller, &task(0x17, 1, &b))
    };
    assert_eq!(add(&mut store, 1, 10001, 3), protocol::empty_success(5, 1));
    add(&mut store, 2, 10001, 4);
    let mut b = vec![];
    for id in [10001u32, 10002] {
        b.push(8);
        b.extend(id.to_le_bytes());
    }
    assert_eq!(
        call(&mut store, 1, &task(0x17, 2, &b)),
        protocol::results_reply(
            5,
            2,
            &[
                archive::counter_result(10001, 7),
                archive::counter_result(10002, 0)
            ]
        )
        .unwrap()
    );
}
