//! Supported executable metadata and owned defaults, expressed as Rust code.
//! Only addresses/lengths are embedded; client identifiers come from the local ELF.
use crate::{
    client_data::read_token,
    elf::Elf,
    league_config::LeagueDef,
    profile::Profile,
    public_config::{CategoryDef, PlaylistDef},
};

pub(crate) struct Build {
    pub label: &'static str,
    pub input_bytes: u64,
    pub input_sha256: &'static str,
}

pub(crate) const SUPPORTED_BUILD: Build = Build {
    label: "supported-client-1",
    input_bytes: 14215768,
    input_sha256: "ba7fd70b68028f833dc1f1f38d2d382485592f1ae25e28804eac583fd8596c39",
};

pub(crate) fn profile(elf: &Elf<'_>) -> Result<Profile, &'static str> {
    Ok(Profile {
        schema_version: 1,
        notice: "Local profile generated from a verified user-supplied executable; keep private."
            .into(),
        bundle_prefix: read_token(elf, &[(0x009735a4, 15)])?,
        bundle_suffix: read_token(elf, &[(0x0097360c, 4)])?,
        playlist_member_name: read_token(elf, &[(0x0093fdf0, 17)])?,
        league_member_name: read_token(elf, &[(0x0097361c, 12)])?,
        league_category_filter: read_token(elf, &[(0x008fa98d, 11)])?,
        minimum_player_settings: vec![
            read_token(elf, &[(0x008faaa4, 16)])?,
            read_token(elf, &[(0x0091b3a8, 7), (0x008faaa9, 11)])?,
        ],
        maps: vec![
            read_token(elf, &[(0x0096d294, 5)])?,
            read_token(elf, &[(0x0096d1f4, 11)])?,
            read_token(elf, &[(0x0096d1cc, 10)])?,
            read_token(elf, &[(0x008f5e5c, 8)])?,
            read_token(elf, &[(0x0096d244, 10)])?,
            read_token(elf, &[(0x0096d26c, 11)])?,
            read_token(elf, &[(0x0096d2ac, 11)])?,
            read_token(elf, &[(0x0096d2fc, 11)])?,
            read_token(elf, &[(0x0096d2d4, 12)])?,
            read_token(elf, &[(0x0096d324, 7)])?,
            read_token(elf, &[(0x0096d344, 8)])?,
            read_token(elf, &[(0x0096d3bc, 10)])?,
            read_token(elf, &[(0x0096d394, 10)])?,
            read_token(elf, &[(0x0096d36c, 10)])?,
        ],
        leagues: vec![
            LeagueDef {
                id: 1,
                name: "Series 1".into(),
                description: "Private competition series 1.".into(),
                team_size: 4,
                teams_allowed: true,
                randoms_allowed: true,
                placement_games: 5,
                playlist_id: 1,
                category: read_token(elf, &[(0x008fa98d, 7), (0x0098d395, 3), (0x008e01ad, 3)])?,
                gametypes: vec![
                    read_token(elf, &[(0x0090cbc0, 3)])?,
                    read_token(elf, &[(0x008ebf62, 3)])?,
                    read_token(elf, &[(0x0096df40, 4)])?,
                ],
            },
            LeagueDef {
                id: 2,
                name: "Series 2".into(),
                description: "Private competition series 2.".into(),
                team_size: 4,
                teams_allowed: true,
                randoms_allowed: true,
                placement_games: 5,
                playlist_id: 2,
                category: read_token(
                    elf,
                    &[
                        (0x008f972c, 6),
                        (0x008e179a, 4),
                        (0x0014465a, 2),
                        (0x008e28bf, 3),
                    ],
                )?,
                gametypes: vec![
                    read_token(elf, &[(0x0096df40, 4)])?,
                    read_token(elf, &[(0x008e0cda, 2)])?,
                    read_token(elf, &[(0x0096df28, 3)])?,
                ],
            },
        ],
        categories: vec![
            CategoryDef {
                key: read_token(elf, &[(0x008e3087, 4)])?,
                name: "Category 1".into(),
                description: "Private playlist category 1.".into(),
            },
            CategoryDef {
                key: read_token(elf, &[(0x008f805d, 6), (0x008e34a0, 4)])?,
                name: "Category 2".into(),
                description: "Private playlist category 2.".into(),
            },
        ],
        playlists: vec![
            PlaylistDef {
                id: 10,
                name: "Playlist 10".into(),
                description: "Private playlist 10.".into(),
                category: read_token(elf, &[(0x008e3087, 4)])?,
                max_party: 6,
                gametypes: vec![read_token(elf, &[(0x0090cbc0, 3)])?],
            },
            PlaylistDef {
                id: 11,
                name: "Playlist 11".into(),
                description: "Private playlist 11.".into(),
                category: read_token(elf, &[(0x008e3087, 4)])?,
                max_party: 4,
                gametypes: vec![read_token(elf, &[(0x0090cbc1, 2)])?],
            },
            PlaylistDef {
                id: 12,
                name: "Playlist 12".into(),
                description: "Private playlist 12.".into(),
                category: read_token(elf, &[(0x008e3087, 4)])?,
                max_party: 6,
                gametypes: vec![read_token(elf, &[(0x008ebf62, 3)])?],
            },
            PlaylistDef {
                id: 13,
                name: "Playlist 13".into(),
                description: "Private playlist 13.".into(),
                category: read_token(elf, &[(0x008e3087, 4)])?,
                max_party: 6,
                gametypes: vec![read_token(elf, &[(0x0096e018, 4)])?],
            },
            PlaylistDef {
                id: 14,
                name: "Playlist 14".into(),
                description: "Private playlist 14.".into(),
                category: read_token(elf, &[(0x008e3087, 4)])?,
                max_party: 6,
                gametypes: vec![read_token(elf, &[(0x0096df40, 4)])?],
            },
            PlaylistDef {
                id: 15,
                name: "Playlist 15".into(),
                description: "Private playlist 15.".into(),
                category: read_token(elf, &[(0x008e3087, 4)])?,
                max_party: 6,
                gametypes: vec![read_token(elf, &[(0x008e0cda, 2)])?],
            },
            PlaylistDef {
                id: 16,
                name: "Playlist 16".into(),
                description: "Private playlist 16.".into(),
                category: read_token(elf, &[(0x008e3087, 4)])?,
                max_party: 6,
                gametypes: vec![read_token(elf, &[(0x0096df88, 3)])?],
            },
            PlaylistDef {
                id: 17,
                name: "Playlist 17".into(),
                description: "Private playlist 17.".into(),
                category: read_token(elf, &[(0x008e3087, 4)])?,
                max_party: 6,
                gametypes: vec![read_token(elf, &[(0x0096df28, 3)])?],
            },
            PlaylistDef {
                id: 18,
                name: "Playlist 18".into(),
                description: "Private playlist 18.".into(),
                category: read_token(elf, &[(0x008e3087, 4)])?,
                max_party: 6,
                gametypes: vec![read_token(elf, &[(0x0096df58, 2)])?],
            },
            PlaylistDef {
                id: 19,
                name: "Playlist 19".into(),
                description: "Private playlist 19.".into(),
                category: read_token(elf, &[(0x008e3087, 4)])?,
                max_party: 6,
                gametypes: vec![
                    read_token(elf, &[(0x0090cbc0, 3)])?,
                    read_token(elf, &[(0x008ebf62, 3)])?,
                    read_token(elf, &[(0x0096e018, 4)])?,
                    read_token(elf, &[(0x0096df40, 4)])?,
                    read_token(elf, &[(0x0096df58, 2)])?,
                ],
            },
            PlaylistDef {
                id: 30,
                name: "Playlist 30".into(),
                description: "Private playlist 30.".into(),
                category: read_token(elf, &[(0x008f805d, 6), (0x008e34a0, 4)])?,
                max_party: 4,
                gametypes: vec![read_token(elf, &[(0x0090b238, 3)])?],
            },
            PlaylistDef {
                id: 31,
                name: "Playlist 31".into(),
                description: "Private playlist 31.".into(),
                category: read_token(elf, &[(0x008f805d, 6), (0x008e34a0, 4)])?,
                max_party: 4,
                gametypes: vec![read_token(elf, &[(0x0096dfa0, 3)])?],
            },
            PlaylistDef {
                id: 32,
                name: "Playlist 32".into(),
                description: "Private playlist 32.".into(),
                category: read_token(elf, &[(0x008f805d, 6), (0x008e34a0, 4)])?,
                max_party: 4,
                gametypes: vec![read_token(elf, &[(0x0096dfb8, 3)])?],
            },
            PlaylistDef {
                id: 33,
                name: "Playlist 33".into(),
                description: "Private playlist 33.".into(),
                category: read_token(elf, &[(0x008f805d, 6), (0x008e34a0, 4)])?,
                max_party: 4,
                gametypes: vec![read_token(elf, &[(0x0096dfe8, 4)])?],
            },
        ],
    })
}

pub(crate) fn hosts(elf: &Elf<'_>) -> Result<[String; 2], &'static str> {
    Ok([
        read_token(elf, &[(0x0096e638, 32)])?,
        read_token(elf, &[(0x0096e6ac, 33)])?,
    ])
}
