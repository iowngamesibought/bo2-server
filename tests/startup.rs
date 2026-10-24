//! Startup services 0x1b/3, 8/1, 8/3, 0x44/1 (LEAGUE06). Request shapes match
//! the original builders' output (emulated), result bodies passed the original
//! deserializers. Invented identities and blobs.
use bo2_server::{league_store::Store, protocol, startup};

fn task(service: u8, op: u8, body: &[u8]) -> Vec<u8> {
    let mut data = vec![service, 3, op];
    data.extend(body);
    let used = data.len();
    data.push(0);
    let pad = bo2_server::protocol::task_padding(used + 1);
    data.extend(std::iter::repeat_n(0x77, pad));
    data
}

fn call(store: &mut Store, caller: u64, data: &[u8]) -> Vec<u8> {
    startup::handle_task(store, caller, 3, data, 1_800_000_000).unwrap()
}

#[test]
fn private_region_survives_client_advertisement_and_league_comparison() {
    let mut store = Store::open_in_memory().unwrap();
    assert_eq!(
        call(&mut store, 1, &task(0x1b, 3, &[])),
        protocol::results_reply(3, 3, &[startup::geolocation_result()]).unwrap()
    );
    // Four empty strings and two zero floats: no claimed physical location.
    let body = startup::geolocation_result();
    assert_eq!(&body[..8], &[16, 0, 16, 0, 16, 0, 16, 0]);
    assert_eq!(&body[8..18], &[13, 0, 0, 0, 0, 13, 0, 0, 0, 0]);
    assert_eq!(body.len(), 38);
    let mut matching_levels = 0;
    for field in body[18..].as_chunks::<5>().0 {
        assert_eq!(field[0], 8);
        let local = u32::from_le_bytes(field[1..].try_into().unwrap());
        // Original 0x52d514 maps unknown local IDs to the typed-i32 null
        // sentinel; 0x15b6b8 compares the received value to the local ID.
        let advertised = if local == u32::MAX {
            i32::MAX as u32
        } else {
            local
        };
        if local != advertised {
            break;
        }
        matching_levels += 1;
    }
    // League's normal/empty-search stages demand >=1; shared IDs satisfy all
    // four levels. The old all-0xffffffff reply fails this regression with 0.
    assert_eq!(matching_levels, 4);
}

#[test]
fn profiles_round_trip_between_players() {
    let mut store = Store::open_in_memory().unwrap();
    // profile set: i32 version 2, blob of 0x400 bytes (as the client sends it).
    let mut blob = vec![0u8; 0x400];
    blob[..4].copy_from_slice(b"prof");
    let mut set = vec![7];
    set.extend(2i32.to_le_bytes());
    set.extend([19, 8]);
    set.extend(0x400u32.to_le_bytes());
    set.extend(&blob);
    assert_eq!(
        call(&mut store, 0x1111, &task(8, 3, &set)),
        protocol::empty_success(3, 3)
    );
    // Another player reads both a known and an unknown identity.
    let mut get = vec![10];
    get.extend(0x1111u64.to_le_bytes());
    get.push(10);
    get.extend(0x9999u64.to_le_bytes());
    let expected = startup::profile_result(0x1111, 2, &blob).unwrap();
    assert_eq!(
        call(&mut store, 0x2222, &task(8, 1, &get)),
        protocol::results_reply(3, 1, &[expected]).unwrap()
    );
}

#[test]
fn presence_is_acknowledged_and_bad_input_refused() {
    let mut store = Store::open_in_memory().unwrap();
    // Presence lives in its own connection-scoped module.
    let mut live = bo2_server::presence::Presence::default();
    let mut presence = vec![10];
    presence.extend(0u64.to_le_bytes());
    presence.extend([19, 8]);
    presence.extend(20u32.to_le_bytes());
    presence.extend([0xab; 20]);
    assert_eq!(
        live.handle_task(0x1111, 7, 3, &task(0x44, 1, &presence))
            .unwrap(),
        protocol::empty_success(3, 1)
    );
    // Lookup as sent by a real console: own ID, then the task terminator.
    let mut get = vec![10];
    get.extend(0x1111u64.to_le_bytes());
    assert_eq!(
        live.handle_task(0x1111, 7, 4, &task(0x44, 2, &get))
            .unwrap(),
        protocol::results_reply(
            4,
            2,
            &[bo2_server::presence::presence_result(true, &[0xab; 20])]
        )
        .unwrap()
    );
    live.drop_connection(7);
    assert_eq!(
        live.handle_task(0x2222, 8, 5, &task(0x44, 2, &get))
            .unwrap(),
        protocol::results_reply(5, 2, &[bo2_server::presence::presence_result(false, &[])])
            .unwrap()
    );
    // Oversized profile, extra padding block, unknown operation.
    let mut big = vec![7];
    big.extend(2i32.to_le_bytes());
    big.extend([19, 8]);
    big.extend(0x401u32.to_le_bytes());
    big.extend(vec![0; 0x401]);
    assert!(startup::handle_task(&mut store, 1, 3, &task(8, 3, &big), 0).is_err());
    let mut geo = task(0x1b, 3, &[]);
    geo.extend([0; 8]);
    assert!(startup::handle_task(&mut store, 1, 3, &geo, 0).is_err());
    assert!(startup::handle_task(&mut store, 1, 3, &task(0x1b, 9, &[]), 0).is_err());
    // Client debug log line 0x43/5 is acknowledged.
    let mut log = vec![8];
    log.extend(1u32.to_le_bytes());
    log.extend([16]);
    log.extend(b"Party_ReadMember\0");
    assert_eq!(
        call(&mut store, 1, &task(0x43, 5, &log)),
        protocol::empty_success(3, 5)
    );
}
