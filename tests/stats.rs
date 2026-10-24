//! Stats service 4: request layouts as the console sent them (byte structure
//! from real 4/1 and 4/11 tasks, invented identities), owned leaderboard policy.
use bo2_server::{
    league_store::{LeaderboardRow, Store},
    protocol,
    stats::{self, Request, WriteRow},
};

const ME: u64 = 0x1111_2222_3333_4444;
const OTHER: u64 = 0x5555_6666_7777_8888;

fn task(op: u8, body: &[u8]) -> Vec<u8> {
    let mut data = vec![4, 3, op];
    data.extend(body);
    let used = data.len();
    data.push(0);
    data.extend(std::iter::repeat_n(0x30, protocol::task_padding(used + 1)));
    data
}

fn write_row(out: &mut Vec<u8>, lb: u32, entity: u64, kind: u8, rating: i64, cols: &[i32]) {
    out.push(8);
    out.extend(lb.to_le_bytes());
    out.push(10);
    out.extend(entity.to_le_bytes());
    out.extend([3, kind]);
    out.push(9);
    out.extend(rating.to_le_bytes());
    for c in cols {
        out.push(7);
        out.extend(c.to_le_bytes());
    }
}

fn call(store: &mut Store, caller: u64, data: &[u8]) -> Vec<u8> {
    stats::handle_task(store, caller, 9, data, 1_800_000_000).unwrap()
}

#[test]
fn console_write_and_multi_read_layouts_parse() {
    // Same shape as a real 4/1: three rows, one i32 column each.
    let mut b = vec![];
    write_row(&mut b, 5102, ME, 0, 1, &[1]);
    write_row(&mut b, 5086, ME, 1, 5, &[5]);
    write_row(&mut b, 5087, ME, 0, 7, &[7]);
    let Request::Write(rows) = stats::parse_task_payload(&task(1, &b)).unwrap() else {
        panic!("expected write")
    };
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[1],
        WriteRow {
            leaderboard: 5086,
            entity: ME,
            write_type: 1,
            rating: 5,
            columns: vec![5]
        }
    );
    // Same shape as a real 4/11: one entity, seven leaderboards.
    let mut b = vec![8];
    b.extend(1u32.to_le_bytes());
    b.push(10);
    b.extend(ME.to_le_bytes());
    b.push(8);
    b.extend(7u32.to_le_bytes());
    for lb in [10006u32, 10005, 10004, 10003, 10002, 10001, 10012] {
        b.push(8);
        b.extend(lb.to_le_bytes());
    }
    assert_eq!(
        stats::parse_task_payload(&task(11, &b)).unwrap(),
        Request::Multi {
            entities: vec![ME],
            leaderboards: vec![10006, 10005, 10004, 10003, 10002, 10001, 10012]
        }
    );
    // Extra padding block and unknown operation are refused.
    let mut padded = task(11, &b);
    padded.extend([0; 8]);
    assert!(stats::parse_task_payload(&padded).is_err());
    assert!(stats::parse_task_payload(&task(9, &[])).is_err());
}

#[test]
fn writes_persist_rank_and_read_back() {
    let mut store = Store::open_in_memory().unwrap();
    store.record_player(ME, b"me", 0).unwrap();
    store.record_player(OTHER, b"other", 0).unwrap();
    let mut b = vec![];
    write_row(&mut b, 10001, ME, 0, 50, &[3, 4]);
    assert_eq!(
        call(&mut store, ME, &task(1, &b)),
        protocol::empty_success(9, 1)
    );
    let mut b = vec![];
    write_row(&mut b, 10001, OTHER, 0, 80, &[1, 1]);
    call(&mut store, OTHER, &task(1, &b));

    // Writing someone else's row is refused and changes nothing.
    let mut b = vec![];
    write_row(&mut b, 10001, OTHER, 0, 1, &[0, 0]);
    assert!(stats::handle_task(&mut store, ME, 9, &task(1, &b), 0).is_err());

    let me = LeaderboardRow {
        leaderboard: 10001,
        entity: ME,
        rating: 50,
        rank: 2,
        columns: vec![3, 4],
    };
    let other = LeaderboardRow {
        leaderboard: 10001,
        entity: OTHER,
        rating: 80,
        rank: 1,
        columns: vec![1, 1],
    };
    // By rank from 1.
    let mut b = vec![8];
    b.extend(10001u32.to_le_bytes());
    b.push(10);
    b.extend(1u64.to_le_bytes());
    b.push(8);
    b.extend(0x65u32.to_le_bytes());
    assert_eq!(
        call(&mut store, ME, &task(4, &b)),
        protocol::results_reply(
            9,
            4,
            &[
                stats::row_result(&other, b"other").unwrap(),
                stats::row_result(&me, b"me").unwrap()
            ]
        )
        .unwrap()
    );
    // By entity, and the multi read with a present and an absent board.
    let mut b = vec![8];
    b.extend(10001u32.to_le_bytes());
    b.push(10);
    b.extend(ME.to_le_bytes());
    assert_eq!(
        call(&mut store, OTHER, &task(3, &b)),
        protocol::results_reply(9, 3, &[stats::row_result(&me, b"me").unwrap()]).unwrap()
    );
    let mut b = vec![8];
    b.extend(1u32.to_le_bytes());
    b.push(10);
    b.extend(ME.to_le_bytes());
    b.push(8);
    b.extend(2u32.to_le_bytes());
    for lb in [10001u32, 10002] {
        b.push(8);
        b.extend(lb.to_le_bytes());
    }
    assert_eq!(
        call(&mut store, ME, &task(11, &b)),
        protocol::results_reply(
            9,
            11,
            &[
                stats::wrapper_result(10001, ME, Some((&me, b"me"))).unwrap(),
                stats::wrapper_result(10002, ME, None).unwrap()
            ]
        )
        .unwrap()
    );
}

#[test]
fn write_types_follow_owned_policy() {
    let mut store = Store::open_in_memory().unwrap();
    let write = |store: &mut Store, kind: u8, rating: i64, cols: &[i32]| {
        let mut b = vec![];
        write_row(&mut b, 7, ME, kind, rating, cols);
        call(store, ME, &task(1, &b));
        store.leaderboard_row(7, ME).unwrap().unwrap()
    };
    assert_eq!(write(&mut store, 0, 10, &[1, 2]).rating, 10);
    let added = write(&mut store, 1, 5, &[1, 1]);
    assert_eq!((added.rating, added.columns.clone()), (15, vec![2, 3]));
    assert_eq!(write(&mut store, 2, 12, &[0, 0]).rating, 15);
    assert_eq!(write(&mut store, 2, 20, &[9, 9]).columns, vec![9, 9]);
    assert_eq!(write(&mut store, 3, 30, &[0, 0]).rating, 20);
    assert_eq!(write(&mut store, 0, 1, &[]).rating, 1);
}

#[test]
fn pivot_page_centers_on_the_entity() {
    let mut store = Store::open_in_memory().unwrap();
    for i in 1..=10u64 {
        let mut b = vec![];
        write_row(&mut b, 3, i, 0, (100 - i) as i64, &[]);
        call(&mut store, i, &task(1, &b));
    }
    // Entity 8 is rank 8; a page of 4 starts at rank 6.
    let page = store.leaderboard_page(3, 6, 4).unwrap();
    assert_eq!(
        page.iter().map(|r| (r.entity, r.rank)).collect::<Vec<_>>(),
        vec![(6, 6), (7, 7), (8, 8), (9, 9)]
    );
    let mut b = vec![8];
    b.extend(3u32.to_le_bytes());
    b.push(10);
    b.extend(8u64.to_le_bytes());
    b.push(8);
    b.extend(4u32.to_le_bytes());
    let expected: Vec<Vec<u8>> = page
        .iter()
        .map(|r| stats::row_result(r, b"").unwrap())
        .collect();
    assert_eq!(
        call(&mut store, 1, &task(5, &b)),
        protocol::results_reply(9, 5, &expected).unwrap()
    );
}
