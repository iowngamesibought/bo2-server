//! Name-derived identity and platform ticket online-ID extraction (LEAGUE06).
//! Invented names; expected IDs were computed with an independent Tiger
//! implementation (libgcrypt via Git's GPG), first eight bytes little-endian.
use bo2_server::identity::{name_player_id, ticket_online_id};
mod common;
use common::ticket;

const LAYOUT: [u16; 6] = [8, 1, 7, 7, 2, 4];

#[test]
fn name_player_id_matches_independent_tiger() {
    assert_eq!(name_player_id(b"abc"), 0xf258_c1e8_8414_ab2a);
    assert_eq!(name_player_id(b"synthetic_player"), 0x465c_d22b_05fa_8828);
    // 0x81e378 lowercases ASCII before hashing.
    assert_eq!(name_player_id(b"Synth-Player_01"), 0x28ed_6396_4e93_d228);
}

#[test]
fn online_id_is_read_from_the_sixth_body_field() {
    assert_eq!(
        ticket_online_id(&ticket(b"Synth-Player_01", LAYOUT)).unwrap(),
        b"Synth-Player_01"
    );
    assert!(ticket_online_id(&ticket(&[b'a'; 16], LAYOUT)).is_ok());
}

#[test]
fn malformed_tickets_are_rejected_not_guessed() {
    let good = ticket(b"synthetic_player", LAYOUT);
    let mut wrong_version = good.clone();
    wrong_version[0] = 0x21;
    assert!(ticket_online_id(&wrong_version).is_err());
    let mut wrong_size = good.clone();
    wrong_size.push(0);
    assert!(ticket_online_id(&wrong_size).is_err());
    assert!(ticket_online_id(&ticket(b"synthetic_player", [8, 1, 7, 7, 4, 2])).is_err());
    for name in [&b"ab"[..], b"bad name", b"bad.name", &[b'a'; 17]] {
        assert!(ticket_online_id(&ticket(name, LAYOUT)).is_err());
    }
    // Bytes after the terminator must be padding.
    let mut padded = good.clone();
    let name_at = 8 + 4 + (4 + 20) + (4 + 4) + (4 + 8) * 2 + (4 + 8) + 4;
    padded[name_at + 20] = b'x';
    assert!(ticket_online_id(&padded).is_err());
    for n in 0..good.len() {
        assert!(ticket_online_id(&good[..n]).is_err());
    }
}
