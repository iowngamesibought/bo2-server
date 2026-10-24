//! Owned League authority: lifecycle, persistence and policy tests.
//! Invented IDs and rules only; these are not official season values.
use bo2_server::league_store::{
    Division, EntryPolicy, Rules, Season, SeasonState, Stats, Store, StoreError, format_minute,
    placement_division_v1,
};
use std::path::PathBuf;

const NOV: i64 = 1_793_491_200; // 2026-11-01 00:00 UTC
const FEB: i64 = 1_801_440_000; // 2027-02-01 00:00 UTC
const MAY: i64 = 1_809_129_600; // 2027-05-01 00:00 UTC

fn division(id: i32, min: f32, max: f32, size: u32) -> Division {
    Division {
        id,
        skill_min: min,
        skill_max: max,
        subdivision_size: size,
        can_place: id != 0,
    }
}

fn rules() -> Rules {
    Rules {
        league_id: 7,
        revision: 1,
        team_size: 4,
        teams_allowed: true,
        randoms_allowed: true,
        placement_games: 3,
        entry: EntryPolicy::Placement,
        divisions: vec![
            division(0, 0.0, 0.0, 0),
            division(1, -10.0, 0.0, 2),
            division(2, 0.0, 10.0, 2),
        ],
    }
}

/// Starting policy: everyone in division 1. Its configured subdivision size (2)
/// is deliberately small to show the shared division ignores it.
fn single_rules() -> Rules {
    Rules {
        entry: EntryPolicy::SingleDivision(1),
        ..rules()
    }
}

#[test]
fn single_division_puts_every_team_on_one_ladder() {
    let mut store = Store::open_in_memory().unwrap();
    store.put_rules(&single_rules()).unwrap();
    store.put_draft(&season(1, NOV, FEB)).unwrap();
    store.publish(7, 1).unwrap();
    let teams = [
        store.team_for_members(&[1]).unwrap(),
        store.team_for_members(&[2]).unwrap(),
        store.team_for_members(&[3]).unwrap(),
        store.team_for_members(&[10, 11, 12, 13]).unwrap(),
    ];
    let assigned: Vec<_> = teams
        .iter()
        .map(|&t| store.current_assignment(t, 7, NOV).unwrap().unwrap())
        .collect();
    // Placed immediately with zero games played: never pending placement.
    assert!(assigned.iter().all(|a| a.division_id == 1));
    assert!(assigned.iter().all(|a| a.metadata() == b"1,1"));
    let sub = assigned[0].subdivision_id;
    assert!(assigned.iter().all(|a| a.subdivision_id == sub));
    let ladder = store.ladder(sub, 100).unwrap();
    assert_eq!(ladder.len(), 4);
    assert_eq!(
        ladder.iter().map(|r| r.rank).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert!(ladder.iter().all(|r| r.played == 0));
    assert!(matches!(
        store.assign_division(teams[0], 7, 1, 2),
        Err(StoreError::Conflict(_))
    ));
    // A season rolled over under the same rules starts a fresh shared ladder.
    store.put_draft(&season(2, FEB, MAY)).unwrap();
    store.publish(7, 2).unwrap();
    let next = store.current_assignment(teams[0], 7, FEB).unwrap().unwrap();
    assert_eq!((next.season_id, next.division_id), (2, 1));
    assert_ne!(next.subdivision_id, sub);
}

#[test]
fn single_division_must_name_a_configured_nonzero_division() {
    let mut store = Store::open_in_memory().unwrap();
    for id in [0, 9] {
        let bad = Rules {
            entry: EntryPolicy::SingleDivision(id),
            ..rules()
        };
        assert!(matches!(store.put_rules(&bad), Err(StoreError::Invalid(_))));
    }
    let db = TempDb::new("single_rules");
    {
        let mut store = Store::open(&db.0).unwrap();
        store.put_rules(&single_rules()).unwrap();
    }
    let store = Store::open(&db.0).unwrap();
    assert_eq!(store.rules(7, 1).unwrap().unwrap(), single_rules());
}

fn season(id: i32, start: i64, end: i64) -> Season {
    Season {
        league_id: 7,
        season_id: id,
        name: format!("Season {id}"),
        start,
        end,
        rules_revision: 1,
        state: SeasonState::Draft,
    }
}

fn open_season(store: &mut Store) {
    store.put_rules(&rules()).unwrap();
    store.put_draft(&season(1, NOV, FEB)).unwrap();
    store.publish(7, 1).unwrap();
}

struct TempDb(PathBuf);
impl TempDb {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "private_league_store_{}_{name}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        Self(path)
    }
}
impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn rules_require_division_zero_and_are_immutable() {
    let mut store = Store::open_in_memory().unwrap();
    let mut bad = rules();
    bad.divisions.remove(0);
    assert!(matches!(store.put_rules(&bad), Err(StoreError::Invalid(_))));
    store.put_rules(&rules()).unwrap();
    store.put_rules(&rules()).unwrap();
    let mut changed = rules();
    changed.placement_games = 5;
    assert!(matches!(
        store.put_rules(&changed),
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(store.rules(7, 1).unwrap().unwrap(), rules());
}

#[test]
fn season_lifecycle_rejects_overlap_and_early_close() {
    let mut store = Store::open_in_memory().unwrap();
    open_season(&mut store);
    // Published seasons are immutable.
    assert!(store.put_draft(&season(1, NOV, MAY)).is_err());
    // Bounds must be whole minutes.
    assert!(store.put_draft(&season(2, FEB + 1, MAY)).is_err());
    store.put_draft(&season(2, FEB - 60, MAY)).unwrap();
    assert!(matches!(store.publish(7, 2), Err(StoreError::Conflict(_))));
    store.put_draft(&season(2, FEB, MAY)).unwrap();
    store.publish(7, 2).unwrap();
    // [start, end) is exclusive at the end.
    assert_eq!(
        store.active_season(7, FEB - 1).unwrap().unwrap().season_id,
        1
    );
    assert_eq!(store.active_season(7, FEB).unwrap().unwrap().season_id, 2);
    assert!(store.active_season(7, NOV - 1).unwrap().is_none());
    assert!(matches!(
        store.close(7, 1, FEB - 1),
        Err(StoreError::Conflict(_))
    ));
    store.close(7, 1, FEB).unwrap();
    assert_eq!(
        store.season(7, 1).unwrap().unwrap().state,
        SeasonState::Closed
    );
    let calendar = store.calendar(7).unwrap();
    assert_eq!(calendar.len(), 2);
    assert_eq!(calendar[0].start, "2026-11-01 00:00");
    assert_eq!(calendar[0].end, "2027-02-01 00:00");
    assert_eq!(calendar[1].id, 2);
}

#[test]
fn calendar_format_matches_independent_utc_values() {
    assert_eq!(format_minute(0), "1970-01-01 00:00");
    assert_eq!(format_minute(951_782_400), "2000-02-29 00:00");
    assert_eq!(format_minute(1_801_267_200), "2027-01-30 00:00");
    assert_eq!(format_minute(253_402_300_680), "9999-12-31 23:58");
}

#[test]
fn team_identity_is_canonical_and_validated() {
    let mut store = Store::open_in_memory().unwrap();
    let a = store.team_for_members(&[30, 10, 20]).unwrap();
    assert_ne!(a, 0);
    assert_eq!(store.team_for_members(&[10, 20, 30]).unwrap(), a);
    assert_eq!(store.team_members(a).unwrap(), vec![10, 20, 30]);
    assert_ne!(store.team_for_members(&[10, 20]).unwrap(), a);
    assert!(store.team_for_members(&[1, 1]).is_err());
    assert!(store.team_for_members(&[]).is_err());
    assert!(store.team_for_members(&[0]).is_err());
    assert!(store.team_for_members(&[u64::MAX]).is_err());
    // Large wire identities survive the signed SQLite column bit-for-bit.
    let high = store.team_for_members(&[u64::MAX - 1]).unwrap();
    assert_eq!(store.team_members(high).unwrap(), vec![u64::MAX - 1]);
    store.rename_team(a, b"Synthetic").unwrap();
    assert_eq!(store.team_name(a).unwrap(), b"Synthetic");
    assert!(store.rename_team(a, &[b'x'; 64]).is_err());
}

#[test]
fn enrollment_starts_in_placement_and_rejects_invalid_composition() {
    let mut store = Store::open_in_memory().unwrap();
    open_season(&mut store);
    let solo = store.team_for_members(&[100]).unwrap();
    assert!(
        store
            .current_assignment(solo, 7, NOV - 60)
            .unwrap()
            .is_none()
    );
    let first = store.current_assignment(solo, 7, NOV).unwrap().unwrap();
    assert_eq!(first.division_id, 0);
    assert_eq!(first.metadata(), b"1,0");
    assert_eq!(
        store.current_assignment(solo, 7, NOV + 60).unwrap(),
        Some(first.clone())
    );
    // Three members match neither a random (1) nor the arranged size (4).
    let trio = store.team_for_members(&[1, 2, 3]).unwrap();
    assert!(matches!(
        store.current_assignment(trio, 7, NOV),
        Err(StoreError::Invalid(_))
    ));
    // The failed request left no enrollment behind.
    assert!(
        store
            .stats_in_subdivision(&[trio], first.subdivision_id)
            .unwrap()
            .is_empty()
    );
    assert!(store.current_assignment(solo, 99, NOV).unwrap().is_none());
    assert!(matches!(
        store.current_assignment(solo, u64::MAX, NOV),
        Err(StoreError::NotFound(_))
    ));
    // Encoded through the original-accepted subdivision codec.
    let body = first.encode().unwrap();
    assert!(body.ends_with(b"\x13\x08\x03\x00\x00\x001,0"));
}

fn played(store: &mut Store, team: u64, games: i32, skill: f32) {
    let stats = Stats {
        played: games,
        placement_skill: skill,
        ..Stats::default()
    };
    store.put_stats(team, 7, 1, &stats).unwrap();
}

#[test]
fn division_assignment_requires_placement_and_fills_by_capacity() {
    let mut store = Store::open_in_memory().unwrap();
    open_season(&mut store);
    let teams: Vec<u64> = (1..=3)
        .map(|n| store.team_for_members(&[n * 1000]).unwrap())
        .collect();
    for &team in &teams {
        store.current_assignment(team, 7, NOV).unwrap();
    }
    played(&mut store, teams[0], 2, 5.0);
    assert!(matches!(
        store.assign_division(teams[0], 7, 1, 2),
        Err(StoreError::Conflict(_))
    ));
    assert!(store.assign_division(teams[0], 7, 1, 0).is_err());
    assert!(store.assign_division(teams[0], 7, 1, 9).is_err());
    let mut placed = vec![];
    for &team in &teams {
        played(&mut store, team, 3, 5.0);
        placed.push(store.assign_division(team, 7, 1, 2).unwrap());
    }
    assert!(placed.iter().all(|a| a.division_id == 2));
    assert_eq!(placed[0].subdivision_id, placed[1].subdivision_id);
    assert_ne!(placed[1].subdivision_id, placed[2].subdivision_id);
    assert_eq!(placed[0].metadata(), b"1,2");
    // The current-season lookup now returns the placed subdivision.
    assert_eq!(
        store.current_assignment(teams[0], 7, NOV).unwrap().unwrap(),
        placed[0]
    );
    store.close(7, 1, FEB).unwrap();
    assert!(store.put_stats(teams[0], 7, 1, &Stats::default()).is_err());
}

#[test]
fn ladder_order_is_deterministic_and_placement_is_unranked() {
    let mut store = Store::open_in_memory().unwrap();
    open_season(&mut store);
    let a = store.team_for_members(&[1]).unwrap();
    let b = store.team_for_members(&[2]).unwrap();
    let pending = store.current_assignment(a, 7, NOV).unwrap().unwrap();
    store.current_assignment(b, 7, NOV).unwrap();
    assert!(
        store
            .ladder(pending.subdivision_id, 10)
            .unwrap()
            .iter()
            .all(|r| r.rank == 0)
    );
    for (team, points, wins) in [(a, 50, 1), (b, 50, 2)] {
        let stats = Stats {
            played: 3,
            wins,
            rank_points: points,
            ..Stats::default()
        };
        store.put_stats(team, 7, 1, &stats).unwrap();
        store.assign_division(team, 7, 1, 2).unwrap();
    }
    let sub = store
        .current_assignment(a, 7, NOV)
        .unwrap()
        .unwrap()
        .subdivision_id;
    let ladder = store.ladder(sub, 10).unwrap();
    assert_eq!(
        ladder
            .iter()
            .map(|r| (r.team_id, r.rank))
            .collect::<Vec<_>>(),
        vec![(b, 1), (a, 2)]
    );
    assert_eq!(store.ladder(sub, 1).unwrap().len(), 1);
    let one = store.stats_in_subdivision(&[a, 999], sub).unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].rank, 2);
    assert!(one[0].encode().is_ok());
}

#[test]
fn placement_policy_v1_is_deterministic() {
    let r = rules();
    assert_eq!(placement_division_v1(&r, -5.0), Some(1));
    assert_eq!(placement_division_v1(&r, 0.0), Some(2));
    assert_eq!(placement_division_v1(&r, -100.0), Some(1));
    assert_eq!(placement_division_v1(&r, 100.0), Some(2));
    assert_eq!(placement_division_v1(&r, f32::NAN), None);
}

#[test]
fn state_survives_restart() {
    let db = TempDb::new("restart");
    let (team, before) = {
        let mut store = Store::open(&db.0).unwrap();
        open_season(&mut store);
        let team = store.team_for_members(&[42, 43, 44, 45]).unwrap();
        store.current_assignment(team, 7, NOV).unwrap();
        played(&mut store, team, 3, -2.0);
        (team, store.assign_division(team, 7, 1, 1).unwrap())
    };
    let mut store = Store::open(&db.0).unwrap();
    assert_eq!(store.team_for_members(&[45, 44, 43, 42]).unwrap(), team);
    assert_eq!(
        store.current_assignment(team, 7, NOV).unwrap().unwrap(),
        before
    );
    let rows = store
        .stats_in_subdivision(&[team], before.subdivision_id)
        .unwrap();
    assert_eq!(rows[0].played, 3);
    assert_eq!(rows[0].placement_skill, -2.0);
    assert_eq!(
        store.season(7, 1).unwrap().unwrap().state,
        SeasonState::Published
    );
}

#[test]
fn hardcoded_season_is_seeded_once_and_always_active() {
    use bo2_server::league_config::{SEASON_END, seed};
    let mut store = Store::open_in_memory().unwrap();
    seed(&mut store, &common::profile()).unwrap();
    seed(&mut store, &common::profile()).unwrap();
    for def in &common::profile().leagues {
        for now in [NOV, MAY, SEASON_END - 1] {
            assert_eq!(
                store.active_season(def.id, now).unwrap().unwrap().season_id,
                1
            );
        }
        assert!(store.active_season(def.id, SEASON_END).unwrap().is_none());
        let calendar = store.calendar(def.id).unwrap();
        assert_eq!(
            (calendar[0].start.as_str(), calendar[0].end.as_str()),
            ("2020-01-01 00:00", "2038-01-19 03:14")
        );
    }
    // One minute past the client's signed 32-bit limit is refused.
    let mut late = season(9, NOV, SEASON_END + 60);
    late.league_id = 7;
    store.put_rules(&rules()).unwrap();
    assert!(matches!(
        store.put_draft(&late),
        Err(StoreError::Invalid(_))
    ));
}

mod common;
