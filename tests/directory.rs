//! Session directory 0x15 (LEAGUE06). The request bytes below were produced by
//! the ORIGINAL client builders (create 0x820450, delete 0x82078c, search
//! 0x8208d0) in emulation from invented field values, and matched independent
//! Python packing. They contain no real addresses, keys or captured traffic.
use bo2_server::{
    directory::{Directory, Request, parse_task_payload},
    league_directory::{SearchResult, create_id_result},
    protocol,
};
use std::time::{Duration, Instant};

const CREATE: &str = concat!(
    "150301130825000000404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f60",
    "61626364082200000008120000000a0807060504030201130810000000a0a1a2a3a4a5a6a7a8a9aaabac",
    "adaeaf07e80300000701000000070700000014070b00000007e9030000070000000007030000000d0000",
    "c03f07050000001407060000000707000000"
);
const DELETE: &str = "150303130808000000a1a2a3a4a5a6a7a8";
const SEARCH: &str = concat!(
    "150305080200000008000000000832000000070100000007070000000703000000070b00000007e90300",
    "00070000000007020000000d0000c03f0d0000803e07050000000706000000070700000007080000000d",
    "0000003f0d0000803e0d0000003e0d0000803f"
);

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}

/// Append minimal CBC padding, as decode_client yields the task payload.
fn padded(body: &[u8]) -> Vec<u8> {
    let mut data = body.to_vec();
    let used = data.len();
    data.push(0);
    let pad = bo2_server::protocol::task_padding(used + 1);
    data.extend(std::iter::repeat_n(0x5a, pad));
    data
}

#[test]
fn original_serializer_bytes_parse() {
    let Request::Create(ad) = parse_task_payload(&padded(&hex(CREATE))).unwrap() else {
        panic!("create");
    };
    assert_eq!(ad.address, std::array::from_fn(|i| 0x40 + i as u8));
    assert_eq!((ad.field_118, ad.field_110), (0x22, 18));
    assert_eq!(ad.peer_id, 0x0102_0304_0506_0708);
    assert_eq!(ad.peer_key, std::array::from_fn(|i| 0xa0 + i as u8));
    // 0x7fffffff travels as null (tag 20) and comes back as the sentinel.
    assert_eq!(ad.attributes, [1000, 1, 7, 0x7fff_ffff, 11, 1001, 0, 3]);
    assert_eq!(ad.party_aggregate, 1.5);
    assert_eq!(ad.geo, [5, 0x7fff_ffff, 6, 7]);
    assert_eq!(
        parse_task_payload(&padded(&hex(DELETE))).unwrap(),
        Request::Delete {
            id: [0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8]
        }
    );
    let Request::Search { limit, query } = parse_task_payload(&padded(&hex(SEARCH))).unwrap()
    else {
        panic!("search");
    };
    assert_eq!(limit, 50);
    assert_eq!(query.ints, [1, 7, 3, 11, 1001, 0, 2]);
    assert_eq!(query.geo_weights, [0.5, 0.25, 0.125, 1.0]);
}

/// Matching query: +0x144=7 / +0x14c=11 / playlist +0x150=1001 as advertised.
fn search_as(dir: &mut Directory, caller: u64, now: Instant) -> Vec<u8> {
    dir.handle_task(caller, 5, &padded(&hex(SEARCH)), now)
        .unwrap()
}

#[test]
fn create_search_delete_lifecycle() {
    let mut dir = Directory::default();
    let t0 = Instant::now();
    let host = 0x1111;
    let reply = dir.handle_task(host, 1, &padded(&hex(CREATE)), t0).unwrap();
    // Reply: success envelope with one 8-byte directory-ID blob (decoder 0x8211c0).
    assert_eq!(
        reply.len(),
        protocol::results_reply(1, 1, &[create_id_result(&[0; 8])])
            .unwrap()
            .len()
    );
    let id: [u8; 8] = reply[reply.len() - 8..].try_into().unwrap();
    assert_eq!(
        reply,
        protocol::results_reply(1, 1, &[create_id_result(&id)]).unwrap()
    );
    // The host does not find its own session; another player does.
    assert_eq!(search_as(&mut dir, host, t0), protocol::empty_success(5, 5));
    let Request::Create(ad) = parse_task_payload(&padded(&hex(CREATE))).unwrap() else {
        unreachable!()
    };
    let expected = SearchResult {
        address: ad.address,
        directory_id: id,
        field_118: ad.field_118,
        field_110: ad.field_110,
        field_114: 0,
        peer_id: ad.peer_id,
        peer_key: ad.peer_key,
        fields_13c_through_158: ad.attributes,
        field_15c: ad.party_aggregate,
        fields_160_through_16c: ad.geo,
    };
    assert_eq!(
        search_as(&mut dir, 0x2222, t0),
        protocol::results_reply(5, 5, &[expected.encode().unwrap()]).unwrap()
    );
    // Only the owner may delete; afterwards nothing is found.
    let mut delete = vec![0x15, 3, 3, 19, 8];
    delete.extend(8u32.to_le_bytes());
    delete.extend(id);
    assert!(dir.handle_task(0x2222, 6, &padded(&delete), t0).is_err());
    assert_eq!(
        dir.handle_task(host, 6, &padded(&delete), t0).unwrap(),
        protocol::empty_success(6, 3)
    );
    assert_eq!(
        search_as(&mut dir, 0x2222, t0),
        protocol::empty_success(5, 5)
    );
}

#[test]
fn leases_owner_disconnect_and_filters() {
    let mut dir = Directory::default();
    let t0 = Instant::now();
    dir.handle_task(0x1111, 1, &padded(&hex(CREATE)), t0)
        .unwrap();
    assert_eq!(dir.len(), 1);
    // Owner disconnect removes its sessions.
    dir.drop_owner(0x1111);
    assert!(dir.is_empty());
    // Lease expiry.
    dir.handle_task(0x1111, 1, &padded(&hex(CREATE)), t0)
        .unwrap();
    let later = t0 + bo2_server::directory::LEASE + Duration::from_secs(1);
    assert_eq!(
        search_as(&mut dir, 0x2222, later),
        protocol::empty_success(5, 5)
    );
    assert!(dir.is_empty());
    // A different playlist (+0x20 in the query) does not match.
    dir.handle_task(0x1111, 1, &padded(&hex(CREATE)), t0)
        .unwrap();
    let mut other = hex(SEARCH);
    // Query ints start after the 3-byte header and three tagged u32 (15 bytes);
    // the fifth i32 (playlist) is at 3 + 15 + 4*5 + 1.
    let at = 3 + 15 + 4 * 5 + 1;
    assert_eq!(&other[at..at + 4], &1001i32.to_le_bytes());
    other[at..at + 4].copy_from_slice(&1002i32.to_le_bytes());
    assert_eq!(
        dir.handle_task(0x2222, 5, &padded(&other), t0).unwrap(),
        protocol::empty_success(5, 5)
    );
}

#[test]
fn malformed_directory_requests_fail_closed() {
    let mut short_address = hex(CREATE);
    short_address[5] = 36; // declared address length 36 instead of 37
    assert!(parse_task_payload(&padded(&short_address)).is_err());
    let mut other_disc = hex(SEARCH);
    other_disc[4] = 1;
    assert!(parse_task_payload(&padded(&other_disc)).is_err());
    let mut data = padded(&hex(DELETE));
    data.extend([0; 8]);
    assert!(parse_task_payload(&data).is_err());
}
