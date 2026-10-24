//! Owned online WAD (LEAGUE06). The generated WAD also passed the original
//! loader 0x55cc24 in emulation (playlists loaded, League config parsed,
//! 84 rotation entries); these tests pin the container and text contracts.
use bo2_server::{league_config, wad};

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// Minimal stored-block reader, independent of the encoder's code path.
fn inflate_stored(stream: &[u8]) -> Vec<u8> {
    assert_eq!(&stream[..2], &[0x78, 0x01]);
    let mut at = 2;
    let mut out = vec![];
    loop {
        let header = stream[at];
        assert_eq!(header & 0b110, 0, "stored blocks only");
        let len = u16::from_le_bytes([stream[at + 1], stream[at + 2]]);
        let nlen = u16::from_le_bytes([stream[at + 3], stream[at + 4]]);
        assert_eq!(len, !nlen);
        out.extend_from_slice(&stream[at + 5..at + 5 + usize::from(len)]);
        at += 5 + usize::from(len);
        if header & 1 == 1 {
            break;
        }
    }
    assert_eq!(at + 4, stream.len());
    out
}

#[test]
fn zlib_stored_matches_python_zlib_level0() {
    // zlib.compress(data, 0) from CPython for the same inputs.
    assert_eq!(wad::zlib_stored(b""), hex("7801010000ffff00000001"));
    assert_eq!(
        wad::zlib_stored(b"hello wad"),
        hex("7801010900f6ff68656c6c6f20776164118b0371")
    );
    // Large input: CPython splits blocks differently, so compare content and
    // its independently computed Adler-32 (167277244) instead of bytes.
    let big: Vec<u8> = (0..76_800).map(|i| (i % 256) as u8).collect();
    let stream = wad::zlib_stored(&big);
    assert_eq!(inflate_stored(&stream), big);
    assert_eq!(stream[stream.len() - 4..], 167_277_244u32.to_be_bytes());
}

#[test]
fn online_wad_layout_and_members() {
    let bytes =
        league_config::online_wad(&common::profile(), league_config::WAD_TIMESTAMP).unwrap();
    assert!(bytes.len() <= wad::MAX_WAD_BYTES);
    let word = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    assert_eq!(word(0), wad::MAGIC);
    assert_eq!(word(4), league_config::WAD_TIMESTAMP);
    assert_eq!(word(8), 2);
    assert_eq!(word(12) as i32, -1);
    let mut members = vec![];
    for i in 0..2 {
        let e = 16 + i * 0x2c;
        let name = &bytes[e..e + 32];
        let name = String::from_utf8(name[..name.iter().position(|&b| b == 0).unwrap()].to_vec());
        let (size, len, offset) = (
            word(e + 0x20) as usize,
            word(e + 0x24) as usize,
            word(e + 0x28) as usize,
        );
        let data = inflate_stored(&bytes[offset..offset + size]);
        assert_eq!(data.len(), len);
        members.push((name.unwrap(), String::from_utf8(data).unwrap()));
    }
    assert_eq!(members[0].0, "playlists.info");
    assert_eq!(members[1].0, "competition.info");
    assert_eq!(
        members[0].1,
        league_config::playlists_info(&common::profile())
    );
    assert_eq!(
        members[1].1,
        league_config::leagues_info(&common::profile())
    );
}

#[test]
fn playlist_text_follows_parser_contracts() {
    let text = league_config::playlists_info(&common::profile());
    // Rotation entries must be CSV; one per map/gametype for each League and
    // public playlist (336 loaded by the original parser in emulation).
    let entries: Vec<&str> = text.lines().filter(|l| l.starts_with("arena_")).collect();
    let expected: usize = common::profile()
        .leagues
        .iter()
        .map(|d| d.gametypes.len())
        .chain(
            common::profile()
                .playlists
                .iter()
                .map(|p| p.gametypes.len()),
        )
        .map(|n| n * common::profile().maps.len())
        .sum();
    assert_eq!(entries.len(), expected);
    assert!(
        entries
            .iter()
            .all(|l| l.split(',').count() == 3 && !l.contains(' '))
    );
    for def in &common::profile().leagues {
        assert!(text.contains(&format!("leagueID {}\n", def.id)));
        assert!(text.contains(&format!("category {}\nfilter competition\n", def.category)));
    }
    // Settings come first, in the parser's `settings` section.
    assert!(
        text.starts_with(
            "version 1\nsettings\nsetdvar party_minimum 2\nsetdvar prematch_minimum 2\n"
        )
    );
    // Calendar in leagues.info is rendered from the same constants as the store.
    let leagues = league_config::leagues_info(&common::profile());
    assert!(leagues.contains("\"start\":\"2020-01-01 00:00\",\"end\":\"2038-01-19 03:14\""));
}

#[test]
fn wad_limits_are_enforced() {
    let data = vec![b'x'; wad::MAX_MEMBER_BYTES + 1];
    let member = wad::Member {
        name: "big.info",
        data: &data,
    };
    assert!(wad::build(1, -1, &[member]).is_err());
    let small = wad::Member {
        name: "a-name-that-is-far-too-long-for-the-slot",
        data: b"",
    };
    assert!(wad::build(1, -1, &[small]).is_err());
    // A WAD over the 64 KiB download buffer is refused.
    let filler = vec![0u8; 0x10000];
    let member = wad::Member {
        name: "filler",
        data: &filler,
    };
    assert!(wad::build(1, -1, &[member]).is_err());
}

#[test]
fn public_playlists_follow_loader_rules() {
    let text = league_config::playlists_info(&common::profile());
    // Every category section precedes every playlist section: inside a playlist,
    // `category <key>` assigns the playlist's category (original loader).
    let first_playlist = text.find("\nplaylist ").unwrap();
    for c in common::profile().categories {
        let at = text.find(&format!("category {}\nname", c.key)).unwrap();
        assert!(at < first_playlist, "{}", c.key);
        // Public categories carry no filter (the public menu filters on "").
        assert!(!text[at..].lines().nth(1).unwrap().starts_with("filter"));
    }
    // Playlist IDs are unique, within 0..=199, and every gametype is declared.
    let mut ids: Vec<u32> = common::profile()
        .leagues
        .iter()
        .map(|d| d.playlist_id)
        .collect();
    ids.extend(common::profile().playlists.iter().map(|p| p.id));
    let unique: std::collections::BTreeSet<u32> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len());
    assert!(ids.iter().all(|&id| id <= 199));
    for p in &common::profile().playlists {
        assert!(
            common::profile()
                .categories
                .iter()
                .any(|c| c.key == p.category)
        );
        for g in &p.gametypes {
            assert!(text.contains(&format!("gametype {g}\nscript {g}\n")), "{g}");
        }
        // Public playlists are not League playlists.
        let section = &text[text.find(&format!("playlist {}\n", p.id)).unwrap()..];
        let section = &section[..section[1..]
            .find("\nplaylist ")
            .map_or(section.len(), |e| e + 1)];
        assert!(!section.contains("leagueID"));
    }
}

mod common;
