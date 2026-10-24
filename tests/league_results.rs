//! Win-only result tracking from 0x52/1 uploads (owned interim policy).
//! Synthetic bodies with invented IDs; the parser is the shipped request parser.
use bo2_server::league_requests::{Request, StatsUpload, parse_request};
use bo2_server::league_store::{
    Division, EntryPolicy, Rules, Season, SeasonState, Store, UploadOutcome,
};
use std::path::PathBuf;

const NOV: i64 = 1_793_491_200; // 2026-11-01 00:00 UTC
const FEB: i64 = 1_801_440_000; // 2027-02-01 00:00 UTC

fn rules() -> Rules {
    let division = |id: i32, min: f32, max: f32| Division {
        id,
        skill_min: min,
        skill_max: max,
        subdivision_size: 50,
        can_place: id != 0,
    };
    Rules {
        league_id: 7,
        revision: 1,
        team_size: 4,
        teams_allowed: true,
        randoms_allowed: true,
        placement_games: 3,
        entry: EntryPolicy::SingleDivision(1),
        divisions: vec![division(0, 0.0, 0.0), division(1, -10.0, 10.0)],
    }
}

fn open(store: &mut Store) {
    store.put_rules(&rules()).unwrap();
    store
        .put_draft(&Season {
            league_id: 7,
            season_id: 1,
            name: "Season 1".into(),
            start: NOV,
            end: FEB,
            rules_revision: 1,
            state: SeasonState::Draft,
        })
        .unwrap();
    store.publish(7, 1).unwrap();
}

/// A 0x52/1 body as the client serializer lays it out (League04/05 contract).
fn body(team: u64, sub: u64, played: i32, wins: i32, losses: i32) -> Vec<u8> {
    let mut out = vec![];
    for id in [team, sub] {
        out.push(10);
        out.extend(id.to_le_bytes());
    }
    out.extend([3, 0]);
    out.push(10);
    out.extend(1234u64.to_le_bytes());
    for counter in [55, played, wins, losses, 1, 2, 3, 9] {
        out.push(7);
        out.extend(counter.to_le_bytes());
    }
    for skill in [12.5f32, 0.7, 3.0] {
        out.push(13);
        out.extend(skill.to_le_bytes());
    }
    out
}

fn upload(raw: &[u8]) -> StatsUpload {
    match parse_request(0x52, 1, raw).unwrap() {
        Request::UploadStats(upload) => upload,
        other => panic!("unexpected {other:?}"),
    }
}

/// Upload as the team's own (solo) member, as a real client would.
fn send(store: &mut Store, raw: &[u8]) -> UploadOutcome {
    let upload = upload(raw);
    let caller = match upload.team_id {
        bo2_server::league_requests::Value::Present(team) => {
            store.team_members(team).map_or(1, |m| m[0])
        }
        bo2_server::league_requests::Value::Null => 1,
    };
    store
        .record_upload(caller, &upload, raw, NOV + 600)
        .unwrap()
}

#[test]
fn uploads_from_non_members_are_not_applied() {
    let mut store = Store::open_in_memory().unwrap();
    open(&mut store);
    let (team, sub) = enrolled(&mut store, 1);
    let raw = body(team, sub, 1, 1, 0);
    assert_eq!(
        store.record_upload(2, &upload(&raw), &raw, NOV).unwrap(),
        UploadOutcome::NotApplied("caller not in team")
    );
    assert_eq!(store.stats_in_subdivision(&[team], sub).unwrap()[0].wins, 0);
    assert_eq!(store.upload_receipts(team).unwrap()[0].caller, 2);
}

fn enrolled(store: &mut Store, member: u64) -> (u64, u64) {
    let team = store.team_for_members(&[member]).unwrap();
    let sub = store
        .current_assignment(team, 7, NOV)
        .unwrap()
        .unwrap()
        .subdivision_id;
    (team, sub)
}

#[test]
fn wins_follow_client_snapshots_and_other_fields_stay_default() {
    let mut store = Store::open_in_memory().unwrap();
    open(&mut store);
    let (team, sub) = enrolled(&mut store, 1);
    // Provisional loss, then the final win snapshot for the same match.
    assert_eq!(
        send(&mut store, &body(team, sub, 1, 0, 1)),
        UploadOutcome::Unchanged
    );
    assert_eq!(
        send(&mut store, &body(team, sub, 1, 1, 0)),
        UploadOutcome::WinRecorded { wins: 1 }
    );
    // A retry of the same snapshot does not count twice.
    assert_eq!(
        send(&mut store, &body(team, sub, 1, 1, 0)),
        UploadOutcome::Unchanged
    );
    // A loss keeps wins; the next win builds on the value read back from us.
    assert_eq!(
        send(&mut store, &body(team, sub, 2, 1, 1)),
        UploadOutcome::Unchanged
    );
    assert_eq!(
        send(&mut store, &body(team, sub, 3, 2, 1)),
        UploadOutcome::WinRecorded { wins: 2 }
    );
    let row = &store.stats_in_subdivision(&[team], sub).unwrap()[0];
    assert_eq!(row.wins, 2);
    // Everything else the client uploaded is ignored and stays at defaults.
    assert_eq!(
        (
            row.played,
            row.losses,
            row.real_points,
            row.rank_points,
            row.streak
        ),
        (0, 0, 0, 0, 0)
    );
    assert_eq!(
        (row.skill, row.variance, row.placement_skill),
        (0.0, 1.0, 0.0)
    );
    assert!(row.encode().is_ok());
}

#[test]
fn implausible_or_unbound_uploads_are_receipted_but_not_applied() {
    let mut store = Store::open_in_memory().unwrap();
    open(&mut store);
    let (team, sub) = enrolled(&mut store, 1);
    let (other, other_sub) = enrolled(&mut store, 2);
    assert_eq!(other_sub, sub); // one shared subdivision
    let cases = [
        (body(team, sub, 1, 2, 0), "wins not stored or stored+1"),
        (body(team, sub, 1, -1, 0), "wins not stored or stored+1"),
        (body(team, 999, 1, 1, 0), "team not in subdivision"),
        (body(other, 999, 1, 1, 0), "team not in subdivision"),
    ];
    for (raw, reason) in &cases {
        assert_eq!(send(&mut store, raw), UploadOutcome::NotApplied(reason));
    }
    // Null wins (tag 0x14) is not a zero.
    let mut raw = body(team, sub, 1, 0, 0);
    let wins_at = 9 + 9 + 2 + 9 + 2 * 5;
    raw.splice(wins_at..wins_at + 5, [20]);
    assert_eq!(
        send(&mut store, &raw),
        UploadOutcome::NotApplied("null wins")
    );
    let receipts = store.upload_receipts(team).unwrap();
    assert_eq!(receipts.len(), 4);
    assert_eq!(receipts[0].raw, cases[0].0);
    assert_eq!(receipts[0].uploaded_wins, Some(2));
    assert_eq!(receipts[0].stored_wins, Some(0));
    assert_eq!(receipts[3].outcome, "null wins");
    for id in [team, other] {
        assert_eq!(store.stats_in_subdivision(&[id], sub).unwrap()[0].wins, 0);
    }
}

#[test]
fn ladder_orders_by_tracked_wins_and_closed_seasons_freeze() {
    let mut store = Store::open_in_memory().unwrap();
    open(&mut store);
    let (a, sub) = enrolled(&mut store, 1);
    let (b, _) = enrolled(&mut store, 2);
    send(&mut store, &body(b, sub, 1, 1, 0));
    let ladder = store.ladder(sub, 10).unwrap();
    assert_eq!(
        ladder
            .iter()
            .map(|r| (r.team_id, r.wins, r.rank))
            .collect::<Vec<_>>(),
        vec![(b, 1, 1), (a, 0, 2)]
    );
    store.close(7, 1, FEB).unwrap();
    assert_eq!(
        send(&mut store, &body(a, sub, 1, 1, 0)),
        UploadOutcome::NotApplied("season not open")
    );
    assert_eq!(store.stats_in_subdivision(&[a], sub).unwrap()[0].wins, 0);
}

#[test]
fn tracked_wins_and_receipts_survive_restart() {
    let path = std::env::temp_dir().join(format!(
        "private_league_results_{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let cleanup = PathBuf::from(&path);
    let (team, sub) = {
        let mut store = Store::open(&path).unwrap();
        open(&mut store);
        let (team, sub) = enrolled(&mut store, 1);
        send(&mut store, &body(team, sub, 1, 1, 0));
        (team, sub)
    };
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.stats_in_subdivision(&[team], sub).unwrap()[0].wins, 1);
    // The post-restart retry is still recognized as the same snapshot.
    assert_eq!(
        send(&mut store, &body(team, sub, 1, 1, 0)),
        UploadOutcome::Unchanged
    );
    assert_eq!(store.upload_receipts(team).unwrap().len(), 2);
    drop(store);
    let _ = std::fs::remove_file(cleanup);
}

#[test]
fn fresh_player_reaches_five_wins_through_dispatch_across_restarts() {
    use bo2_server::{
        league_service::{self, Caller},
        protocol,
    };
    let path = std::env::temp_dir().join(format!(
        "private_five_wins_{}_{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let task = |op: u8, body: &[u8]| {
        let mut data = vec![0x52, 3, op];
        data.extend_from_slice(body);
        data.push(0);
        data.extend(vec![0x35; protocol::task_padding(data.len())]);
        data
    };
    let caller = Caller { player_id: 42 };
    let (team, sub, mut expected) = {
        let mut store = Store::open(&path).unwrap();
        open(&mut store);
        let (team, sub) = enrolled(&mut store, caller.player_id);
        let row = store.stats_in_subdivision(&[team], sub).unwrap().remove(0);
        assert_eq!(row.wins, 0); // No operator seed or special new-player default.
        (team, sub, row)
    };
    let mut read_body = vec![10];
    read_body.extend(team.to_le_bytes());
    read_body.push(10);
    read_body.extend(sub.to_le_bytes());
    read_body.push(8);
    read_body.extend(1u32.to_le_bytes());
    for wins in 1..=5 {
        // Reopen persisted state before every match, rather than retaining a
        // client/server in-memory counter from the preceding match.
        let mut store = Store::open(&path).unwrap();
        assert_eq!(enrolled(&mut store, caller.player_id), (team, sub));
        let previous = store.stats_in_subdivision(&[team], sub).unwrap()[0].wins;
        assert_eq!(previous, wins - 1);
        for snapshot in [previous, wins, wins, previous] {
            // Unchanged/preloss, final win, duplicate final, stale old snapshot.
            let reply = league_service::handle_task(
                &mut store,
                &caller,
                1,
                &task(1, &body(team, sub, wins, snapshot, 0)),
                NOV + 600,
            )
            .unwrap();
            assert_eq!(reply, protocol::empty_success(1, 1));
        }
        expected.wins = wins;
        let reply =
            league_service::handle_task(&mut store, &caller, 2, &task(4, &read_body), NOV + 600)
                .unwrap();
        assert_eq!(
            reply,
            protocol::results_reply(2, 4, &[expected.encode().unwrap()]).unwrap()
        );
    }
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.stats_in_subdivision(&[team], sub).unwrap()[0].wins, 5);
    let receipts = store.upload_receipts(team).unwrap();
    assert_eq!(
        receipts
            .iter()
            .filter(|r| r.outcome == "win_recorded")
            .count(),
        5
    );
    assert_eq!(receipts.len(), 20);
    // A different new player still begins at zero after someone reaches five.
    let (other, other_sub) = enrolled(&mut store, 43);
    assert_eq!(
        store.stats_in_subdivision(&[other], other_sub).unwrap()[0].wins,
        0
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}
