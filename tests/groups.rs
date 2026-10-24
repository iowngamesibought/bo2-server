//! Group population counts 0x1c (LEAGUE06). Invented group IDs.
use bo2_server::{
    groups::{Groups, count_result},
    protocol,
};

fn task(op: u8, groups: &[u32]) -> Vec<u8> {
    let mut data = vec![0x1c, 3, op, 108, 8];
    data.extend(((groups.len() * 4) as u32).to_le_bytes());
    data.extend((groups.len() as u32).to_le_bytes());
    for g in groups {
        data.extend(g.to_le_bytes());
    }
    let used = data.len();
    data.push(0);
    let pad = bo2_server::protocol::task_padding(used + 1);
    data.extend(std::iter::repeat_n(0x33, pad));
    data
}

#[test]
fn counts_are_truthful_and_follow_connections() {
    let mut groups = Groups::default();
    assert_eq!(
        groups.handle_task(1, 7, &task(1, &[2, 5])).unwrap(),
        protocol::empty_success(7, 1)
    );
    groups.handle_task(2, 8, &task(1, &[5])).unwrap();
    let expected = [count_result(0, 0), count_result(2, 1), count_result(5, 2)];
    assert_eq!(
        groups.handle_task(3, 9, &task(4, &[0, 2, 5])).unwrap(),
        protocol::results_reply(9, 4, &expected).unwrap()
    );
    // A later setGroups replaces membership; disconnect removes it.
    groups.handle_task(1, 10, &task(1, &[0])).unwrap();
    assert_eq!(
        (groups.count(0), groups.count(2), groups.count(5)),
        (1, 0, 1)
    );
    groups.drop_connection(2);
    assert_eq!(groups.count(5), 0);
    // Empty query: zero results.
    assert_eq!(
        groups.handle_task(3, 11, &task(4, &[])).unwrap(),
        protocol::empty_success(11, 4)
    );
}

#[test]
fn malformed_group_requests_fail_closed() {
    let mut groups = Groups::default();
    let mut bad = task(4, &[1, 2]);
    bad[4 + 1] = 9; // byte length no longer matches count
    assert!(groups.handle_task(1, 1, &bad).is_err());
    assert!(groups.handle_task(1, 1, &task(2, &[1])).is_err());
    let too_many: Vec<u32> = (0..0x401).collect();
    assert!(groups.handle_task(1, 1, &task(4, &too_many)).is_err());
}
