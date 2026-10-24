//! Data-driven competition assets and persistent season policy.
// TODO(league-seasons): replace the ever-running season with operator-managed
// seasons (draft/publish/close/rollover already exist in `league_store`).
use crate::league_store::{
    Division, EntryPolicy, MAX_SEASON_END, Rules, Season, SeasonState, Store, StoreError,
    format_minute,
};

/// 2020-01-01 00:00 UTC. Earlier than any plausible deployment clock.
pub const SEASON_START: i64 = 1_577_836_800;
/// 2038-01-19 03:14 UTC, the latest end the client's signed 32-bit compare accepts.
pub const SEASON_END: i64 = MAX_SEASON_END;
pub const SEASON_ID: i32 = 1;
pub const SEASON_NAME: &str = "Season 1";
pub const RULES_REVISION: u32 = 1;
/// The one shared division every team enters.
pub const SHARED_DIVISION: i32 = 1;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct LeagueDef {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub team_size: u32,
    pub teams_allowed: bool,
    pub randoms_allowed: bool,
    pub placement_games: u32,
    /// Playlist ID (0..=199) and its `configured filter` category key.
    pub playlist_id: u32,
    pub category: String,
    /// Gametype script names (verified present in the supported client).
    pub gametypes: Vec<String>,
}

/// Required PATCH version declared by the WAD: -1 matches a declined PATCH.
pub const WAD_PATCH_VERSION: i32 = -1;
/// WAD header timestamp; bump whenever the generated members change.
pub const WAD_TIMESTAMP: u32 = 5;
/// Minimum players to start a League match (owned policy, see playlists_info).
pub const MIN_PLAYERS: u32 = 2;

/// `competition member` JSON for the 0x2cd478 loader: the same League/season/division
/// IDs the store is seeded with.
// TODO(league-config): icons and news reference game materials/strings whose
// valid names are not recovered; they are omitted and the client falls back.
pub fn leagues_info(profile: &crate::profile::Profile) -> String {
    let leagues: Vec<String> = profile
        .leagues
        .iter()
        .map(|def| {
            format!(
                concat!(
                    "{{\"id\":{},\"name\":\"{}\",\"description\":\"{}\",",
                    "\"teams_allowed\":{},\"randoms_allowed\":{},\"team_size\":{},",
                    "\"placement_games\":{},\"placement_window\":0.0,",
                    "\"bonus_pool\":0,\"bonus_per_win\":0,\"bonus_time\":\"{}\",",
                    "\"bonus_days\":1,\"bonus_cumulative\":false,",
                    "\"elo_mean\":0.0,\"elo_scale\":400.0,\"elo_k\":32.0,",
                    "\"seasons\":[{{\"id\":{},\"name\":\"{}\",\"start\":\"{}\",\"end\":\"{}\"}}]}}"
                ),
                def.id,
                def.name,
                def.description,
                def.teams_allowed,
                def.randoms_allowed,
                def.team_size,
                def.placement_games,
                format_minute(SEASON_START),
                SEASON_ID,
                SEASON_NAME,
                format_minute(SEASON_START),
                format_minute(SEASON_END),
            )
        })
        .collect();
    format!(
        concat!(
            "{{\"version\":1,\"leagues\":[{}],\"divisions\":[",
            "{{\"id\":0,\"name\":\"Placement\",\"description\":\"\",",
            "\"skill_min\":-1000.0,\"skill_max\":1000.0,\"subdivision_size\":1000,",
            "\"can_place\":false,\"tweak_skill\":false}},",
            "{{\"id\":{},\"name\":\"League\",\"description\":\"\",",
            "\"skill_min\":-1000.0,\"skill_max\":1000.0,\"subdivision_size\":1000,",
            "\"can_place\":true,\"tweak_skill\":false}}],",
            "\"nouns\":[],\"numbers\":[]}}"
        ),
        leagues.join(","),
        SHARED_DIVISION
    )
}

/// `playlist member` text for the 0x37ea88 parser: one `configured filter` category
/// and one ranked League playlist per series, then the public categories and
/// playlists (`public_config`), all rotating launch maps.
pub fn playlists_info(profile: &crate::profile::Profile) -> String {
    let mut out = String::from("version 1\n");
    // Apply the profile's minimum-player setting tokens before declaring
    // playlists. The two-player policy is shared by private deployments.
    out.push_str("settings\n");
    for setting in &profile.minimum_player_settings {
        out.push_str(&format!("setdvar {setting} {MIN_PLAYERS}\n"));
    }
    let mut gametypes: Vec<&str> = profile
        .leagues
        .iter()
        .flat_map(|d| d.gametypes.iter().map(String::as_str))
        .chain(
            profile
                .playlists
                .iter()
                .flat_map(|p| p.gametypes.iter().map(String::as_str)),
        )
        .collect();
    gametypes.sort_unstable();
    gametypes.dedup();
    for gametype in gametypes {
        out.push_str(&format!("gametype {gametype}\nscript {gametype}\n"));
    }
    for def in &profile.leagues {
        out.push_str(&format!(
            "category {}\nfilter {}\nname english \"{}\"\ndescription english \"{}\"\n",
            def.category, profile.league_category_filter, def.name, def.description
        ));
    }
    crate::public_config::render_categories(&mut out, &profile.categories);
    for def in &profile.leagues {
        out.push_str(&format!(
            concat!(
                "playlist {}\nname english \"{}\"\ndescription english \"{}\"\n",
                "category {}\nleagueID {}\nranked\nminparty 1\nmaxparty {}\n"
            ),
            def.playlist_id, def.name, def.description, def.category, def.id, def.team_size
        ));
        for map in &profile.maps {
            for gametype in &def.gametypes {
                // Rotation entries are CSV: the parser ungets the line and re-reads
                // it in comma mode (tokenizer flag +0x40b), so spaces would break it.
                out.push_str(&format!("{map},{gametype},1\n"));
            }
        }
    }
    crate::public_config::render_playlists(&mut out, &profile.maps, &profile.playlists);
    out
}

/// The online WAD served for `configured publisher bundle`.
pub fn online_wad(
    profile: &crate::profile::Profile,
    timestamp: u32,
) -> Result<Vec<u8>, crate::protocol::ProtocolError> {
    let leagues = leagues_info(profile);
    let playlists = playlists_info(profile);
    crate::wad::build(
        timestamp,
        WAD_PATCH_VERSION,
        &[
            crate::wad::Member {
                name: &profile.playlist_member_name,
                data: playlists.as_bytes(),
            },
            crate::wad::Member {
                name: &profile.league_member_name,
                data: leagues.as_bytes(),
            },
        ],
    )
}

pub fn rules(def: &LeagueDef) -> Rules {
    let division = |id: i32| Division {
        id,
        skill_min: -1000.0,
        skill_max: 1000.0,
        subdivision_size: 1000,
        can_place: id != 0,
    };
    Rules {
        league_id: def.id,
        revision: RULES_REVISION,
        team_size: def.team_size,
        teams_allowed: def.teams_allowed,
        randoms_allowed: def.randoms_allowed,
        placement_games: def.placement_games,
        entry: EntryPolicy::SingleDivision(SHARED_DIVISION),
        divisions: vec![division(0), division(SHARED_DIVISION)],
    }
}

pub fn season(def: &LeagueDef) -> Season {
    Season {
        league_id: def.id,
        season_id: SEASON_ID,
        name: SEASON_NAME.into(),
        start: SEASON_START,
        end: SEASON_END,
        rules_revision: RULES_REVISION,
        state: SeasonState::Draft,
    }
}

/// Idempotently install the rules and the published ever-running season.
pub fn seed(store: &mut Store, profile: &crate::profile::Profile) -> Result<(), StoreError> {
    for def in &profile.leagues {
        store.put_rules(&rules(def))?;
        match store.season(def.id, SEASON_ID)? {
            None => {
                store.put_draft(&season(def))?;
                store.publish(def.id, SEASON_ID)?;
            }
            Some(existing) => {
                let expected = Season {
                    state: SeasonState::Published,
                    ..season(def)
                };
                if existing != expected {
                    return Err(StoreError::Conflict(
                        "stored season differs from the hardcoded season",
                    ));
                }
            }
        }
    }
    Ok(())
}
