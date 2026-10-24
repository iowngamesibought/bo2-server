//! Persistent League authority for the supported client profile (Stage C, owned policy).
//!
//! Seasons, rules revisions, stable teams, enrollments, subdivisions, division
//! assignment and per-season statistics in one SQLite database. Every mutation
//! is a single immediate transaction. This module does not authenticate callers:
//! dispatch must bind a verified private identity before invoking it.
//!
//! Policies here (team canonicalization, season resolution, subdivision filling,
//! placement selection, ladder order, win tracking) are private-server choices,
//! not recovered official behavior. Of the uploaded results only the win count
//! is applied (see `record_upload`); every other statistic stays at its default.
use crate::league::{self, MAX_TEAM_MEMBERS, StatsResult};
use crate::league_requests::{StatsUpload, Value};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::{error::Error, fmt, path::Path};

/// Client configuration parser limits (League04 season report).
pub const MAX_LEAGUES: usize = 8;
pub const MAX_SEASONS_PER_LEAGUE: usize = 16;
pub const MAX_DIVISIONS: usize = 32;
/// Last whole minute before the client's signed 32-bit time overflows:
/// 2038-01-19 03:14 UTC.
pub const MAX_SEASON_END: i64 = 2_147_483_640;
/// 0x51/4 writer bound: 0x40 including its NUL.
pub const MAX_TEAM_NAME_BYTES: usize = 63;
const SCHEMA_VERSION: i64 = 3;

#[derive(Debug)]
pub enum StoreError {
    Db(rusqlite::Error),
    Invalid(&'static str),
    NotFound(&'static str),
    Conflict(&'static str),
}
impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Db(e) => write!(f, "database: {e}"),
            Self::Invalid(m) => write!(f, "invalid: {m}"),
            Self::NotFound(m) => write!(f, "not found: {m}"),
            Self::Conflict(m) => write!(f, "conflict: {m}"),
        }
    }
}
impl Error for StoreError {}
impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e)
    }
}
type Result<T> = std::result::Result<T, StoreError>;

/// Wire u64 identities are stored bit-for-bit in SQLite's signed INTEGER.
fn sql_id(value: u64) -> i64 {
    value as i64
}
fn wire_id(value: i64) -> u64 {
    value as u64
}

/// Division record, identical to the one published in `leagues.info`.
#[derive(Clone, Debug, PartialEq)]
pub struct Division {
    pub id: i32,
    pub skill_min: f32,
    pub skill_max: f32,
    pub subdivision_size: u32,
    pub can_place: bool,
}

/// How a team enters a season's divisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryPolicy {
    /// Every team goes straight into this nonzero division and one uncapped
    /// subdivision, so the whole private player pool shares a single ladder. A
    /// nonzero division means the client never shows placement or "awaiting
    /// placement". This is the starting policy for a small private population.
    SingleDivision(i32),
    /// Teams start in division 0 and move only through `assign_division` after
    /// the placement games.
    // TODO(league-divisions): make this the live policy once the population can
    // fill several divisions. It needs result ingestion to advance `played`, an
    // automatic placement step (e.g. `placement_division_v1`), and
    // promotion/relegation rules.
    Placement,
}

/// Immutable rules revision for one League. Values are operator configuration,
/// not recovered official defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Rules {
    pub league_id: i32,
    pub revision: u32,
    pub team_size: u32,
    pub teams_allowed: bool,
    pub randoms_allowed: bool,
    pub placement_games: u32,
    pub entry: EntryPolicy,
    pub divisions: Vec<Division>,
}

impl Rules {
    fn validate(&self) -> Result<()> {
        if self.league_id <= 0 || self.revision == 0 {
            return Err(StoreError::Invalid(
                "League and revision IDs must be positive",
            ));
        }
        if self.team_size == 0 || self.team_size as usize > MAX_TEAM_MEMBERS {
            return Err(StoreError::Invalid("team size outside 1..=15"));
        }
        if !self.teams_allowed && !self.randoms_allowed {
            return Err(StoreError::Invalid("League admits no team kind"));
        }
        if self.placement_games == 0 {
            return Err(StoreError::Invalid("placement games must be positive"));
        }
        if self.divisions.is_empty() || self.divisions.len() > MAX_DIVISIONS {
            return Err(StoreError::Invalid("division count outside 1..=32"));
        }
        let mut ids: Vec<i32> = self.divisions.iter().map(|d| d.id).collect();
        ids.sort_unstable();
        ids.dedup();
        if ids.len() != self.divisions.len() || ids.iter().any(|&id| id < 0) {
            return Err(StoreError::Invalid(
                "division IDs must be unique and nonnegative",
            ));
        }
        // The client resolves division 0 to show the pending-placement state.
        if ids[0] != 0 {
            return Err(StoreError::Invalid("division 0 is required"));
        }
        for d in self.divisions.iter().filter(|d| d.id != 0) {
            if !d.skill_min.is_finite() || !d.skill_max.is_finite() || d.skill_min >= d.skill_max {
                return Err(StoreError::Invalid("division skill range invalid"));
            }
            if d.subdivision_size == 0 {
                return Err(StoreError::Invalid("subdivision size must be positive"));
            }
        }
        if !self.divisions.iter().any(|d| d.id != 0 && d.can_place) {
            return Err(StoreError::Invalid("no placeable division"));
        }
        if let EntryPolicy::SingleDivision(id) = self.entry
            && (id == 0 || self.division(id).is_none())
        {
            return Err(StoreError::Invalid(
                "single division must be a configured nonzero division",
            ));
        }
        Ok(())
    }

    fn admits(&self, members: usize) -> bool {
        (members == 1 && self.randoms_allowed)
            || (members == self.team_size as usize && self.teams_allowed)
    }

    fn division(&self, id: i32) -> Option<&Division> {
        self.divisions.iter().find(|d| d.id == id)
    }

    /// Division a newly enrolled team starts in.
    fn entry_division(&self) -> i32 {
        match self.entry {
            EntryPolicy::SingleDivision(id) => id,
            EntryPolicy::Placement => 0,
        }
    }

    /// Placement (division 0) and the single shared division are uncapped.
    fn capacity(&self, division_id: i32) -> Result<Option<i64>> {
        if division_id == 0 || self.entry == EntryPolicy::SingleDivision(division_id) {
            return Ok(None);
        }
        let division = self
            .division(division_id)
            .ok_or(StoreError::NotFound("division"))?;
        Ok(Some(i64::from(division.subdivision_size)))
    }
}

/// Owned placement policy v1: the placeable nonzero division whose
/// `[skill_min, skill_max)` contains `placement_skill`, else the nearest end of
/// the placeable range. Ties pick the lowest ID. Not the official algorithm.
pub fn placement_division_v1(rules: &Rules, placement_skill: f32) -> Option<i32> {
    if !placement_skill.is_finite() {
        return None;
    }
    let mut placeable: Vec<&Division> = rules
        .divisions
        .iter()
        .filter(|d| d.id != 0 && d.can_place)
        .collect();
    placeable.sort_by(|a, b| a.skill_min.total_cmp(&b.skill_min).then(a.id.cmp(&b.id)));
    let first = placeable.first()?;
    if let Some(d) = placeable
        .iter()
        .find(|d| d.skill_min <= placement_skill && placement_skill < d.skill_max)
    {
        return Some(d.id);
    }
    if placement_skill < first.skill_min {
        return Some(first.id);
    }
    placeable
        .iter()
        .max_by(|a, b| a.skill_max.total_cmp(&b.skill_max).then(b.id.cmp(&a.id)))
        .map(|d| d.id)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeasonState {
    Draft,
    Published,
    Closed,
}
impl SeasonState {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "draft" => Ok(Self::Draft),
            "published" => Ok(Self::Published),
            "closed" => Ok(Self::Closed),
            _ => Err(StoreError::Invalid("unknown stored season state")),
        }
    }
}

/// Season window in Unix seconds, `[start, end)` as the client evaluates it.
/// The client calendar has minute resolution, so both bounds are whole minutes.
#[derive(Clone, Debug, PartialEq)]
pub struct Season {
    pub league_id: i32,
    pub season_id: i32,
    pub name: String,
    pub start: i64,
    pub end: i64,
    pub rules_revision: u32,
    pub state: SeasonState,
}

/// One calendar row for `leagues.info`, rendered from published/closed seasons.
#[derive(Clone, Debug, PartialEq)]
pub struct CalendarEntry {
    pub id: i32,
    pub name: String,
    pub start: String,
    pub end: String,
}

/// Current placement of a team in a season. `metadata` is the subdivision blob.
#[derive(Clone, Debug, PartialEq)]
pub struct Assignment {
    pub team_id: u64,
    pub league_id: i32,
    pub season_id: i32,
    pub subdivision_id: u64,
    pub division_id: i32,
}

impl Assignment {
    /// Version-1 metadata `1,<division>`: sets the division, leaves the native
    /// name index at -1 and the unnamed boolean true, as an empty blob would.
    pub fn metadata(&self) -> Vec<u8> {
        format!("1,{}", self.division_id).into_bytes()
    }

    /// 0x51/3 result body. The first u64 is filled with the team ID; that field's
    /// meaning is unproven (see `league::subdivision_result`).
    pub fn encode(&self) -> std::result::Result<Vec<u8>, crate::protocol::ProtocolError> {
        league::subdivision_result(
            self.team_id,
            self.league_id as u64,
            self.subdivision_id,
            &self.metadata(),
        )
    }
}

/// Stored per-season statistics. `field_58` is not stored: its meaning is open.
#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub rank_points: i64,
    pub real_points: i32,
    pub played: i32,
    pub wins: i32,
    pub losses: i32,
    pub bonus_used: i32,
    pub bonus_time: i32,
    pub streak: i32,
    pub career_wins: i32,
    pub skill: f32,
    pub variance: f32,
    pub placement_skill: f32,
}

impl Default for Stats {
    /// Mirrors the client's zero-result fallback (skill 0, variance 1).
    fn default() -> Self {
        Self {
            rank_points: 0,
            real_points: 0,
            played: 0,
            wins: 0,
            losses: 0,
            bonus_used: 0,
            bonus_time: 0,
            streak: 0,
            career_wins: 0,
            skill: 0.0,
            variance: 1.0,
            placement_skill: 0.0,
        }
    }
}

const SCHEMA: &str = "
CREATE TABLE rules(
  league_id INTEGER NOT NULL, revision INTEGER NOT NULL, team_size INTEGER NOT NULL,
  teams_allowed INTEGER NOT NULL, randoms_allowed INTEGER NOT NULL,
  placement_games INTEGER NOT NULL, entry_division INTEGER,
  PRIMARY KEY(league_id, revision));
CREATE TABLE divisions(
  league_id INTEGER NOT NULL, revision INTEGER NOT NULL, division_id INTEGER NOT NULL,
  skill_min REAL NOT NULL, skill_max REAL NOT NULL, subdivision_size INTEGER NOT NULL,
  can_place INTEGER NOT NULL, PRIMARY KEY(league_id, revision, division_id),
  FOREIGN KEY(league_id, revision) REFERENCES rules(league_id, revision));
CREATE TABLE seasons(
  league_id INTEGER NOT NULL, season_id INTEGER NOT NULL, name TEXT NOT NULL,
  start_s INTEGER NOT NULL, end_s INTEGER NOT NULL, rules_revision INTEGER NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('draft','published','closed')),
  PRIMARY KEY(league_id, season_id),
  FOREIGN KEY(league_id, rules_revision) REFERENCES rules(league_id, revision));
CREATE TABLE teams(
  team_id INTEGER PRIMARY KEY AUTOINCREMENT, members_key BLOB NOT NULL UNIQUE,
  member_count INTEGER NOT NULL, name BLOB NOT NULL);
CREATE TABLE team_members(
  team_id INTEGER NOT NULL REFERENCES teams(team_id), position INTEGER NOT NULL,
  member_id INTEGER NOT NULL, PRIMARY KEY(team_id, position));
CREATE TABLE subdivisions(
  subdivision_id INTEGER PRIMARY KEY AUTOINCREMENT, league_id INTEGER NOT NULL,
  season_id INTEGER NOT NULL, division_id INTEGER NOT NULL, ordinal INTEGER NOT NULL,
  capacity INTEGER, UNIQUE(league_id, season_id, division_id, ordinal),
  FOREIGN KEY(league_id, season_id) REFERENCES seasons(league_id, season_id));
CREATE TABLE enrollments(
  team_id INTEGER NOT NULL REFERENCES teams(team_id), league_id INTEGER NOT NULL,
  season_id INTEGER NOT NULL,
  subdivision_id INTEGER NOT NULL REFERENCES subdivisions(subdivision_id),
  division_id INTEGER NOT NULL, policy_revision INTEGER NOT NULL,
  PRIMARY KEY(team_id, league_id, season_id));
CREATE TABLE stats(
  team_id INTEGER NOT NULL, league_id INTEGER NOT NULL, season_id INTEGER NOT NULL,
  rank_points INTEGER NOT NULL, real_points INTEGER NOT NULL, played INTEGER NOT NULL,
  wins INTEGER NOT NULL, losses INTEGER NOT NULL, bonus_used INTEGER NOT NULL,
  bonus_time INTEGER NOT NULL, streak INTEGER NOT NULL, career_wins INTEGER NOT NULL,
  skill REAL NOT NULL, variance REAL NOT NULL, placement_skill REAL NOT NULL,
  PRIMARY KEY(team_id, league_id, season_id),
  FOREIGN KEY(team_id, league_id, season_id)
    REFERENCES enrollments(team_id, league_id, season_id));
CREATE TABLE upload_receipts(
  receipt_id INTEGER PRIMARY KEY AUTOINCREMENT, received_at INTEGER NOT NULL,
  caller INTEGER NOT NULL, team_id INTEGER, subdivision_id INTEGER,
  uploaded_wins INTEGER, stored_wins INTEGER, outcome TEXT NOT NULL, raw BLOB NOT NULL);
CREATE INDEX upload_receipts_team ON upload_receipts(team_id, receipt_id);
CREATE TABLE players(
  player_id INTEGER PRIMARY KEY, online_name BLOB NOT NULL, last_seen INTEGER NOT NULL);
CREATE TABLE profiles(
  owner INTEGER PRIMARY KEY, version INTEGER NOT NULL, data BLOB NOT NULL,
  updated INTEGER NOT NULL);
CREATE TABLE user_files(
  owner INTEGER NOT NULL, filename TEXT NOT NULL, data BLOB NOT NULL,
  updated INTEGER NOT NULL, PRIMARY KEY(owner, filename));
";

/// Version 2: leaderboard rows for stats service 4. Columns are i32 LE.
const SCHEMA_V2: &str = "
CREATE TABLE leaderboard_rows(
  leaderboard INTEGER NOT NULL, entity INTEGER NOT NULL, rating INTEGER NOT NULL,
  columns BLOB NOT NULL, updated INTEGER NOT NULL, PRIMARY KEY(leaderboard, entity));
CREATE INDEX leaderboard_order ON leaderboard_rows(leaderboard, rating DESC, entity);
";

/// Version 3: key archive 0x0f and counters 0x17.
const SCHEMA_V3: &str = "
CREATE TABLE key_archive(
  owner INTEGER NOT NULL, category INTEGER NOT NULL, key INTEGER NOT NULL,
  value INTEGER NOT NULL, updated INTEGER NOT NULL, PRIMARY KEY(owner, category, key));
CREATE TABLE counters(
  counter INTEGER PRIMARY KEY, total INTEGER NOT NULL, updated INTEGER NOT NULL);
";

/// One stored leaderboard row with its computed 1-based rank.
#[derive(Clone, Debug, PartialEq)]
pub struct LeaderboardRow {
    pub leaderboard: u32,
    pub entity: u64,
    pub rating: i64,
    pub rank: u64,
    pub columns: Vec<i32>,
}

fn pack_columns(columns: &[i32]) -> Vec<u8> {
    columns.iter().flat_map(|c| c.to_le_bytes()).collect()
}

fn unpack_columns(bytes: &[u8]) -> Vec<i32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| i32::from_le_bytes(*c))
        .collect()
}

/// Position of `wins` in `StatsUpload::counters` (realPoints, played, wins, ...).
const UPLOAD_WINS: usize = 2;

/// What `record_upload` did with one 0x52/1 upload. All outcomes are receipted
/// and all are acknowledged to the client; only `WinRecorded` changes state.
#[derive(Clone, Debug, PartialEq)]
pub enum UploadOutcome {
    /// Uploaded wins was stored+1: one win counted.
    WinRecorded { wins: i32 },
    /// Uploaded wins equals the stored value: a retry, loss, draw or preloss.
    Unchanged,
    /// Kept as a receipt only; the reason is stored with it.
    NotApplied(&'static str),
}

impl UploadOutcome {
    fn label(&self) -> &'static str {
        match self {
            Self::WinRecorded { .. } => "win_recorded",
            Self::Unchanged => "unchanged",
            Self::NotApplied(reason) => reason,
        }
    }
}

/// One stored upload, for diagnostics and future re-processing.
#[derive(Clone, Debug, PartialEq)]
pub struct UploadReceipt {
    pub received_at: i64,
    /// Name-derived identity of the connection that sent the upload.
    pub caller: u64,
    pub subdivision_id: Option<u64>,
    pub uploaded_wins: Option<i32>,
    pub stored_wins: Option<i32>,
    pub outcome: String,
    pub raw: Vec<u8>,
}

/// Policy revision recorded with each enrollment change.
pub const POLICY_REVISION: u32 = 1;

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        match version {
            0 => {
                let tx = conn.unchecked_transaction()?;
                tx.execute_batch(SCHEMA)?;
                tx.execute_batch(SCHEMA_V2)?;
                tx.execute_batch(SCHEMA_V3)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                tx.commit()?;
            }
            1 | 2 => {
                let tx = conn.unchecked_transaction()?;
                if version == 1 {
                    tx.execute_batch(SCHEMA_V2)?;
                }
                tx.execute_batch(SCHEMA_V3)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                tx.commit()?;
            }
            SCHEMA_VERSION => {}
            _ => return Err(StoreError::Invalid("unsupported database schema version")),
        }
        Ok(Self { conn })
    }

    fn write(&mut self) -> Result<Transaction<'_>> {
        Ok(self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?)
    }

    /// Insert an immutable rules revision. Re-inserting identical rules is a no-op.
    pub fn put_rules(&mut self, rules: &Rules) -> Result<()> {
        rules.validate()?;
        let tx = self.write()?;
        if let Some(existing) = load_rules(&tx, rules.league_id, rules.revision)? {
            return if existing == *rules {
                Ok(())
            } else {
                Err(StoreError::Conflict("rules revision is immutable"))
            };
        }
        tx.execute(
            "INSERT INTO rules VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                rules.league_id,
                rules.revision,
                rules.team_size,
                rules.teams_allowed,
                rules.randoms_allowed,
                rules.placement_games,
                match rules.entry {
                    EntryPolicy::SingleDivision(id) => Some(id),
                    EntryPolicy::Placement => None,
                }
            ],
        )?;
        for d in &rules.divisions {
            tx.execute(
                "INSERT INTO divisions VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    rules.league_id,
                    rules.revision,
                    d.id,
                    f64::from(d.skill_min),
                    f64::from(d.skill_max),
                    d.subdivision_size,
                    d.can_place
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn rules(&self, league_id: i32, revision: u32) -> Result<Option<Rules>> {
        load_rules(&self.conn, league_id, revision)
    }

    /// Create or replace a draft. Published and closed seasons are immutable.
    pub fn put_draft(&mut self, season: &Season) -> Result<()> {
        if season.state != SeasonState::Draft {
            return Err(StoreError::Invalid("only drafts can be written"));
        }
        validate_window(season)?;
        let tx = self.write()?;
        if load_rules(&tx, season.league_id, season.rules_revision)?.is_none() {
            return Err(StoreError::NotFound("rules revision"));
        }
        if let Some(existing) = load_season(&tx, season.league_id, season.season_id)?
            && existing.state != SeasonState::Draft
        {
            return Err(StoreError::Conflict("season already published"));
        }
        tx.execute(
            "INSERT OR REPLACE INTO seasons VALUES(?1,?2,?3,?4,?5,?6,'draft')",
            params![
                season.league_id,
                season.season_id,
                season.name,
                season.start,
                season.end,
                season.rules_revision
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Publish a draft after checking overlap and client configuration limits.
    pub fn publish(&mut self, league_id: i32, season_id: i32) -> Result<()> {
        let tx = self.write()?;
        let season =
            load_season(&tx, league_id, season_id)?.ok_or(StoreError::NotFound("season"))?;
        if season.state != SeasonState::Draft {
            return Err(StoreError::Conflict("season is not a draft"));
        }
        validate_window(&season)?;
        let overlapping: i64 = tx.query_row(
            "SELECT COUNT(*) FROM seasons WHERE league_id=?1 AND state!='draft'
             AND start_s < ?3 AND ?2 < end_s",
            params![league_id, season.start, season.end],
            |r| r.get(0),
        )?;
        if overlapping != 0 {
            return Err(StoreError::Conflict("season overlaps a published season"));
        }
        let leagues: i64 = tx.query_row(
            "SELECT COUNT(DISTINCT league_id) FROM seasons
             WHERE state!='draft' AND league_id!=?1",
            params![league_id],
            |r| r.get(0),
        )?;
        if leagues as usize >= MAX_LEAGUES {
            return Err(StoreError::Conflict("client supports at most 8 Leagues"));
        }
        tx.execute(
            "UPDATE seasons SET state='published' WHERE league_id=?1 AND season_id=?2",
            params![league_id, season_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Close a published season once its window has ended. Its enrollments and
    /// statistics remain as immutable history.
    pub fn close(&mut self, league_id: i32, season_id: i32, now: i64) -> Result<()> {
        let tx = self.write()?;
        let season =
            load_season(&tx, league_id, season_id)?.ok_or(StoreError::NotFound("season"))?;
        if season.state != SeasonState::Published {
            return Err(StoreError::Conflict("season is not published"));
        }
        if now < season.end {
            return Err(StoreError::Conflict("season window has not ended"));
        }
        tx.execute(
            "UPDATE seasons SET state='closed' WHERE league_id=?1 AND season_id=?2",
            params![league_id, season_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn season(&self, league_id: i32, season_id: i32) -> Result<Option<Season>> {
        load_season(&self.conn, league_id, season_id)
    }

    /// The published season whose `[start, end)` contains `now`, if any.
    pub fn active_season(&self, league_id: i32, now: i64) -> Result<Option<Season>> {
        active_season(&self.conn, league_id, now)
    }

    /// Calendar rows for `leagues.info`: the latest 16 published/closed seasons,
    /// oldest first, with UTC `YYYY-MM-DD HH:MM` bounds.
    pub fn calendar(&self, league_id: i32) -> Result<Vec<CalendarEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT season_id, name, start_s, end_s FROM seasons
             WHERE league_id=?1 AND state!='draft' ORDER BY start_s DESC LIMIT ?2",
        )?;
        let mut rows = stmt
            .query_map(params![league_id, MAX_SEASONS_PER_LEAGUE as i64], |r| {
                Ok(CalendarEntry {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    start: format_minute(r.get(2)?),
                    end: format_minute(r.get(3)?),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// 0x51/1: stable team for a member set. Order is not significant in this
    /// policy; duplicates, zero and the null sentinel are rejected. The caller
    /// must have verified that the requester belongs to `member_ids`.
    pub fn team_for_members(&mut self, member_ids: &[u64]) -> Result<u64> {
        let mut sorted = member_ids.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted.len() != member_ids.len() {
            return Err(StoreError::Invalid("duplicate team member"));
        }
        if sorted.is_empty() || sorted.len() > MAX_TEAM_MEMBERS {
            return Err(StoreError::Invalid("team member count outside 1..=15"));
        }
        if sorted.iter().any(|&id| id == 0 || id == u64::MAX) {
            return Err(StoreError::Invalid("invalid member identity"));
        }
        let key: Vec<u8> = sorted.iter().flat_map(|id| id.to_be_bytes()).collect();
        let tx = self.write()?;
        if let Some(id) = tx
            .query_row(
                "SELECT team_id FROM teams WHERE members_key=?1",
                params![key],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
        {
            return Ok(wire_id(id));
        }
        tx.execute(
            "INSERT INTO teams(members_key, member_count, name) VALUES(?1,?2,x'')",
            params![key, sorted.len() as i64],
        )?;
        let team_id = tx.last_insert_rowid();
        for (position, id) in sorted.iter().enumerate() {
            tx.execute(
                "INSERT INTO team_members VALUES(?1,?2,?3)",
                params![team_id, position as i64, sql_id(*id)],
            )?;
        }
        tx.commit()?;
        Ok(wire_id(team_id))
    }

    /// Members in canonical order.
    /// Teams a player belongs to, ordered by team ID.
    pub fn teams_of(&self, player_id: u64) -> Result<Vec<u64>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT team_id FROM team_members WHERE member_id=?1 ORDER BY team_id",
        )?;
        let ids = stmt
            .query_map(params![sql_id(player_id)], |r| r.get::<_, i64>(0))?
            .map(|id| id.map(wire_id))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    pub fn team_members(&self, team_id: u64) -> Result<Vec<u64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT member_id FROM team_members WHERE team_id=?1 ORDER BY position")?;
        let ids = stmt
            .query_map(params![sql_id(team_id)], |r| r.get::<_, i64>(0))?
            .map(|id| id.map(wire_id))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if ids.is_empty() {
            return Err(StoreError::NotFound("team"));
        }
        Ok(ids)
    }

    /// 0x51/4. Bytes are kept raw; no character-set policy is implied.
    pub fn rename_team(&mut self, team_id: u64, name: &[u8]) -> Result<()> {
        if name.len() > MAX_TEAM_NAME_BYTES || name.contains(&0) {
            return Err(StoreError::Invalid("team name"));
        }
        let tx = self.write()?;
        let changed = tx.execute(
            "UPDATE teams SET name=?2 WHERE team_id=?1",
            params![sql_id(team_id), name],
        )?;
        if changed != 1 {
            return Err(StoreError::NotFound("team"));
        }
        tx.commit()?;
        Ok(())
    }

    pub fn team_name(&self, team_id: u64) -> Result<Vec<u8>> {
        team_name(&self.conn, team_id)
    }

    /// 0x51/3 for one League: the team's assignment in the active season,
    /// enrolling it in the season's placement subdivision on first request.
    /// Returns `None` when no published season is active at `now`.
    pub fn current_assignment(
        &mut self,
        team_id: u64,
        league_id: u64,
        now: i64,
    ) -> Result<Option<Assignment>> {
        let league_id = i32::try_from(league_id).map_err(|_| StoreError::NotFound("League"))?;
        let tx = self.write()?;
        let Some(season) = active_season(&tx, league_id, now)? else {
            return Ok(None);
        };
        if let Some(found) = load_assignment(&tx, team_id, league_id, season.season_id)? {
            return Ok(Some(found));
        }
        let rules = load_rules(&tx, league_id, season.rules_revision)?
            .ok_or(StoreError::NotFound("rules revision"))?;
        let members: i64 = tx
            .query_row(
                "SELECT member_count FROM teams WHERE team_id=?1",
                params![sql_id(team_id)],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(StoreError::NotFound("team"))?;
        if !rules.admits(members as usize) {
            return Err(StoreError::Invalid(
                "team composition not allowed in League",
            ));
        }
        let division = rules.entry_division();
        let subdivision = subdivision_with_room(&tx, &rules, season.season_id, division)?;
        tx.execute(
            "INSERT INTO enrollments VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                sql_id(team_id),
                league_id,
                season.season_id,
                subdivision,
                division,
                POLICY_REVISION
            ],
        )?;
        let d = Stats::default();
        tx.execute(
            "INSERT INTO stats VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                sql_id(team_id),
                league_id,
                season.season_id,
                d.rank_points,
                d.real_points,
                d.played,
                d.wins,
                d.losses,
                d.bonus_used,
                d.bonus_time,
                d.streak,
                d.career_wins,
                f64::from(d.skill),
                f64::from(d.variance),
                f64::from(d.placement_skill)
            ],
        )?;
        let assignment = load_assignment(&tx, team_id, league_id, season.season_id)?
            .ok_or(StoreError::NotFound("enrollment"))?;
        tx.commit()?;
        Ok(Some(assignment))
    }

    /// Move an enrolled team into a placeable nonzero division of an open season.
    /// From division 0 the team must have completed the placement games. Only
    /// `EntryPolicy::Placement` Leagues reassign; a single-division League refuses.
    pub fn assign_division(
        &mut self,
        team_id: u64,
        league_id: i32,
        season_id: i32,
        division_id: i32,
    ) -> Result<Assignment> {
        let tx = self.write()?;
        let season =
            load_season(&tx, league_id, season_id)?.ok_or(StoreError::NotFound("season"))?;
        if season.state != SeasonState::Published {
            return Err(StoreError::Conflict("season is not open"));
        }
        let rules = load_rules(&tx, league_id, season.rules_revision)?
            .ok_or(StoreError::NotFound("rules revision"))?;
        if rules.entry != EntryPolicy::Placement {
            return Err(StoreError::Conflict("single-division League"));
        }
        let current = load_assignment(&tx, team_id, league_id, season_id)?
            .ok_or(StoreError::NotFound("enrollment"))?;
        let target = rules
            .division(division_id)
            .ok_or(StoreError::NotFound("division"))?;
        if target.id == 0 || !target.can_place {
            return Err(StoreError::Invalid("division cannot receive placements"));
        }
        if current.division_id == division_id {
            return Ok(current);
        }
        if current.division_id == 0 {
            let played = load_stats(&tx, team_id, league_id, season_id)?.played;
            if played < 0 || (played as u32) < rules.placement_games {
                return Err(StoreError::Conflict("placement games not completed"));
            }
        }
        let subdivision = subdivision_with_room(&tx, &rules, season_id, division_id)?;
        tx.execute(
            "UPDATE enrollments SET subdivision_id=?4, division_id=?5, policy_revision=?6
             WHERE team_id=?1 AND league_id=?2 AND season_id=?3",
            params![
                sql_id(team_id),
                league_id,
                season_id,
                subdivision,
                division_id,
                POLICY_REVISION
            ],
        )?;
        let assignment = load_assignment(&tx, team_id, league_id, season_id)?
            .ok_or(StoreError::NotFound("enrollment"))?;
        tx.commit()?;
        Ok(assignment)
    }

    /// Receipt a 0x52/1 upload and apply only its win count.
    ///
    /// An upload is a full client snapshot, not a delta. Retries resend it
    /// unchanged; a win snapshot carries the last-read wins plus one; loss, draw
    /// and preloss snapshots keep it. So uploaded == stored is a no-op, stored+1
    /// counts one win, and anything else is kept unapplied. Every other field is
    /// ignored on purpose. `raw` is the request body, kept for re-processing.
    ///
    /// Dispatch acknowledges every outcome to the client: an error reply makes
    /// it retry the same snapshot. `caller` is the connection's bound identity;
    /// uploads for a team it does not belong to are receipted, not applied.
    // TODO(league-results): a client could claim a win it did not earn. Bind wins
    // to server-admitted matches once the directory exists, and apply the other
    // statistics under a versioned rating policy.
    pub fn record_upload(
        &mut self,
        caller: u64,
        upload: &StatsUpload,
        raw: &[u8],
        now: i64,
    ) -> Result<UploadOutcome> {
        let tx = self.write()?;
        let (stored, outcome) = apply_upload(&tx, caller, upload)?;
        let present = |v: Value<u64>| match v {
            Value::Present(id) => Some(sql_id(id)),
            Value::Null => None,
        };
        let uploaded = match upload.counters[UPLOAD_WINS] {
            Value::Present(wins) => Some(wins),
            Value::Null => None,
        };
        tx.execute(
            "INSERT INTO upload_receipts(received_at, caller, team_id, subdivision_id,
                                         uploaded_wins, stored_wins, outcome, raw)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                now,
                sql_id(caller),
                present(upload.team_id),
                present(upload.subdivision_id),
                uploaded,
                stored,
                outcome.label(),
                raw
            ],
        )?;
        tx.commit()?;
        Ok(outcome)
    }

    /// Upload receipts for one team, oldest first.
    pub fn upload_receipts(&self, team_id: u64) -> Result<Vec<UploadReceipt>> {
        let mut stmt = self.conn.prepare(
            "SELECT received_at, caller, subdivision_id, uploaded_wins, stored_wins, outcome,
                    raw
             FROM upload_receipts WHERE team_id=?1 ORDER BY receipt_id",
        )?;
        let rows = stmt
            .query_map(params![sql_id(team_id)], |r| {
                Ok(UploadReceipt {
                    received_at: r.get(0)?,
                    caller: wire_id(r.get(1)?),
                    subdivision_id: r.get::<_, Option<i64>>(2)?.map(wire_id),
                    uploaded_wins: r.get(3)?,
                    stored_wins: r.get(4)?,
                    outcome: r.get(5)?,
                    raw: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Remember the online name last seen for an identity, for member lists.
    /// Only the name is kept; tickets and platform account numbers are not stored.
    pub fn record_player(&mut self, player_id: u64, online_name: &[u8], now: i64) -> Result<()> {
        if player_id == 0 || online_name.is_empty() || online_name.len() > 16 {
            return Err(StoreError::Invalid("player identity"));
        }
        let tx = self.write()?;
        tx.execute(
            "INSERT INTO players VALUES(?1,?2,?3)
             ON CONFLICT(player_id) DO UPDATE SET online_name=?2, last_seen=?3",
            params![sql_id(player_id), online_name, now],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Last seen online name, or empty when the identity never logged in here.
    /// Apply one stats write for its own entity. Write types are owned policy:
    /// 0 replace, 1 add, 2 keep higher rating, 3 keep lower rating; other
    /// values replace. See `stats` for the open question.
    pub fn write_leaderboard_row(&mut self, row: &crate::stats::WriteRow, now: i64) -> Result<()> {
        if row.columns.len() > crate::stats::MAX_COLUMNS {
            return Err(StoreError::Invalid("too many leaderboard columns"));
        }
        let tx = self.write()?;
        let existing: Option<(i64, Vec<u8>)> = tx
            .query_row(
                "SELECT rating, columns FROM leaderboard_rows WHERE leaderboard=?1 AND entity=?2",
                params![row.leaderboard, sql_id(row.entity)],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (rating, columns) = match (row.write_type, existing) {
            (1, Some((rating, columns))) => {
                let mut sums = unpack_columns(&columns);
                sums.resize(sums.len().max(row.columns.len()), 0);
                for (sum, add) in sums.iter_mut().zip(&row.columns) {
                    *sum = sum.wrapping_add(*add);
                }
                (rating.wrapping_add(row.rating), sums)
            }
            (2, Some((rating, columns))) if rating >= row.rating => {
                (rating, unpack_columns(&columns))
            }
            (3, Some((rating, columns))) if rating <= row.rating => {
                (rating, unpack_columns(&columns))
            }
            _ => (row.rating, row.columns.clone()),
        };
        tx.execute(
            "INSERT INTO leaderboard_rows VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(leaderboard, entity) DO UPDATE SET rating=?3, columns=?4, updated=?5",
            params![
                row.leaderboard,
                sql_id(row.entity),
                rating,
                pack_columns(&columns),
                now
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// One entity's row on a leaderboard, with its rank.
    pub fn leaderboard_row(&self, leaderboard: u32, entity: u64) -> Result<Option<LeaderboardRow>> {
        let row: Option<(i64, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT rating, columns FROM leaderboard_rows WHERE leaderboard=?1 AND entity=?2",
                params![leaderboard, sql_id(entity)],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((rating, columns)) = row else {
            return Ok(None);
        };
        // Rank by rating descending, then identity ascending (signed SQL order
        // of the stored identity, consistent with `leaderboard_page`).
        let ahead: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM leaderboard_rows WHERE leaderboard=?1
             AND (rating > ?2 OR (rating = ?2 AND entity < ?3))",
            params![leaderboard, rating, sql_id(entity)],
            |r| r.get(0),
        )?;
        Ok(Some(LeaderboardRow {
            leaderboard,
            entity,
            rating,
            rank: ahead as u64 + 1,
            columns: unpack_columns(&columns),
        }))
    }

    /// Rows from 1-based rank `first`, at most `max`.
    pub fn leaderboard_page(
        &self,
        leaderboard: u32,
        first: u64,
        max: u32,
    ) -> Result<Vec<LeaderboardRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT entity, rating, columns FROM leaderboard_rows WHERE leaderboard=?1
             ORDER BY rating DESC, entity ASC LIMIT ?2 OFFSET ?3",
        )?;
        let offset = i64::try_from(first.saturating_sub(1)).unwrap_or(i64::MAX);
        let rows = stmt
            .query_map(params![leaderboard, i64::from(max), offset], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(i, (entity, rating, columns))| LeaderboardRow {
                leaderboard,
                entity: wire_id(entity),
                rating,
                rank: first.max(1) + i as u64,
                columns: unpack_columns(&columns),
            })
            .collect())
    }

    /// Apply key archive writes for one owner. Write types are owned policy:
    /// 0 replace, 1 add, 2 keep higher, 3 keep lower; other values replace.
    pub fn write_archive(
        &mut self,
        owner: u64,
        category: u16,
        keys: &[crate::archive::KeyWrite],
        now: i64,
    ) -> Result<()> {
        let tx = self.write()?;
        for k in keys {
            let old: Option<i64> = tx
                .query_row(
                    "SELECT value FROM key_archive WHERE owner=?1 AND category=?2 AND key=?3",
                    params![sql_id(owner), category, k.key],
                    |r| r.get(0),
                )
                .optional()?;
            let value = match (k.write_type, old) {
                (1, Some(old)) => old.wrapping_add(k.value),
                (2, Some(old)) => old.max(k.value),
                (3, Some(old)) => old.min(k.value),
                _ => k.value,
            };
            tx.execute(
                "INSERT INTO key_archive VALUES(?1,?2,?3,?4,?5)
                 ON CONFLICT(owner, category, key) DO UPDATE SET value=?4, updated=?5",
                params![sql_id(owner), category, k.key, value, now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn archive_value(&self, owner: u64, category: u16, key: u16) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM key_archive WHERE owner=?1 AND category=?2 AND key=?3",
                params![sql_id(owner), category, key],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Every stored key of one category, by key.
    pub fn archive_category(&self, owner: u64, category: u16) -> Result<Vec<(u16, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, value FROM key_archive WHERE owner=?1 AND category=?2 ORDER BY key",
        )?;
        let rows = stmt
            .query_map(params![sql_id(owner), category], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn add_counters(&mut self, counters: &[(u32, i64)], now: i64) -> Result<()> {
        let tx = self.write()?;
        for &(counter, delta) in counters {
            tx.execute(
                "INSERT INTO counters VALUES(?1,?2,?3)
                 ON CONFLICT(counter) DO UPDATE SET total=total+?2, updated=?3",
                params![counter, delta, now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn counter(&self, counter: u32) -> Result<i64> {
        Ok(self
            .conn
            .query_row(
                "SELECT total FROM counters WHERE counter=?1",
                params![counter],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    pub fn player_name(&self, player_id: u64) -> Result<Vec<u8>> {
        Ok(self
            .conn
            .query_row(
                "SELECT online_name FROM players WHERE player_id=?1",
                params![sql_id(player_id)],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_default())
    }

    /// Per-identity user storage (service 0x0a), e.g. `player_state`.
    /// Bytes are opaque: stored and returned exactly as the client uploaded.
    pub fn user_file(&self, owner: u64, filename: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn
            .query_row(
                "SELECT data FROM user_files WHERE owner=?1 AND filename=?2",
                params![sql_id(owner), filename],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn put_user_file(
        &mut self,
        owner: u64,
        filename: &str,
        data: &[u8],
        now: i64,
    ) -> Result<()> {
        if owner == 0 || filename.is_empty() {
            return Err(StoreError::Invalid("user file identity"));
        }
        let tx = self.write()?;
        tx.execute(
            "INSERT INTO user_files VALUES(?1,?2,?3,?4)
             ON CONFLICT(owner, filename) DO UPDATE SET data=?3, updated=?4",
            params![sql_id(owner), filename, data, now],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn remove_user_file(&mut self, owner: u64, filename: &str) -> Result<bool> {
        let tx = self.write()?;
        let n = tx.execute(
            "DELETE FROM user_files WHERE owner=?1 AND filename=?2",
            params![sql_id(owner), filename],
        )?;
        tx.commit()?;
        Ok(n == 1)
    }

    /// Public profile (service 8): version and opaque blob as set by the owner.
    pub fn profile(&self, owner: u64) -> Result<Option<(i32, Vec<u8>)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT version, data FROM profiles WHERE owner=?1",
                params![sql_id(owner)],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    pub fn put_profile(&mut self, owner: u64, version: i32, data: &[u8], now: i64) -> Result<()> {
        if owner == 0 {
            return Err(StoreError::Invalid("profile owner"));
        }
        let tx = self.write()?;
        tx.execute(
            "INSERT INTO profiles VALUES(?1,?2,?3,?4)
             ON CONFLICT(owner) DO UPDATE SET version=?2, data=?3, updated=?4",
            params![sql_id(owner), version, data, now],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn is_member(&self, team_id: u64, player_id: u64) -> Result<bool> {
        Ok(is_member(&self.conn, team_id, player_id)?)
    }

    /// The team's subdivision in one season, for season-history lookups.
    pub fn season_subdivision(
        &self,
        team_id: u64,
        league_id: i32,
        season_id: i32,
    ) -> Result<Option<u64>> {
        Ok(load_assignment(&self.conn, team_id, league_id, season_id)?.map(|a| a.subdivision_id))
    }

    /// Replace one team's statistics in an open season. Operator/test use; the
    /// upload path changes only wins, through `record_upload`.
    pub fn put_stats(
        &mut self,
        team_id: u64,
        league_id: i32,
        season_id: i32,
        stats: &Stats,
    ) -> Result<()> {
        if [stats.skill, stats.variance, stats.placement_skill]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err(StoreError::Invalid("non-finite skill"));
        }
        let tx = self.write()?;
        let season =
            load_season(&tx, league_id, season_id)?.ok_or(StoreError::NotFound("season"))?;
        if season.state != SeasonState::Published {
            return Err(StoreError::Conflict("season is not open"));
        }
        let changed = tx.execute(
            "UPDATE stats SET rank_points=?4, real_points=?5, played=?6, wins=?7, losses=?8,
             bonus_used=?9, bonus_time=?10, streak=?11, career_wins=?12, skill=?13,
             variance=?14, placement_skill=?15
             WHERE team_id=?1 AND league_id=?2 AND season_id=?3",
            params![
                sql_id(team_id),
                league_id,
                season_id,
                stats.rank_points,
                stats.real_points,
                stats.played,
                stats.wins,
                stats.losses,
                stats.bonus_used,
                stats.bonus_time,
                stats.streak,
                stats.career_wins,
                f64::from(stats.skill),
                f64::from(stats.variance),
                f64::from(stats.placement_skill)
            ],
        )?;
        if changed != 1 {
            return Err(StoreError::NotFound("enrollment"));
        }
        tx.commit()?;
        Ok(())
    }

    /// Statistics rows for teams currently in `subdivision_id` (0x52/2 shape),
    /// with ranks computed within that subdivision. Unknown teams are omitted.
    pub fn stats_in_subdivision(
        &self,
        team_ids: &[u64],
        subdivision_id: u64,
    ) -> Result<Vec<StatsResult>> {
        let ladder = self.ladder(subdivision_id, u32::MAX)?;
        Ok(team_ids
            .iter()
            .filter_map(|id| ladder.iter().find(|row| row.team_id == *id).cloned())
            .collect())
    }

    /// Subdivision ladder ordered by owned policy v1: rank points descending,
    /// wins descending, team ID ascending. Uploads never set rank points, so in
    /// practice this orders by wins. The placement subdivision (division 0) is
    /// unranked and reports rank 0.
    pub fn ladder(&self, subdivision_id: u64, limit: u32) -> Result<Vec<StatsResult>> {
        let division: Option<i32> = self
            .conn
            .query_row(
                "SELECT division_id FROM subdivisions WHERE subdivision_id=?1",
                params![sql_id(subdivision_id)],
                |r| r.get(0),
            )
            .optional()?;
        let division = division.ok_or(StoreError::NotFound("subdivision"))?;
        let mut stmt = self.conn.prepare(
            "SELECT s.team_id, t.name, s.rank_points, s.real_points, s.played, s.wins,
                    s.losses, s.bonus_used, s.bonus_time, s.streak, s.career_wins,
                    s.skill, s.variance, s.placement_skill
             FROM enrollments e
             JOIN stats s USING(team_id, league_id, season_id)
             JOIN teams t ON t.team_id = e.team_id
             WHERE e.subdivision_id=?1
             ORDER BY s.rank_points DESC, s.wins DESC, s.team_id ASC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![sql_id(subdivision_id), i64::from(limit)], |r| {
                Ok(StatsResult {
                    team_id: wire_id(r.get(0)?),
                    subdivision_id,
                    name: r.get(1)?,
                    field_58: 0,
                    rank: 0,
                    rank_points: r.get(2)?,
                    real_points: r.get(3)?,
                    played: r.get(4)?,
                    wins: r.get(5)?,
                    losses: r.get(6)?,
                    bonus_used: r.get(7)?,
                    bonus_time: r.get(8)?,
                    streak: r.get(9)?,
                    career_wins: r.get(10)?,
                    skill: r.get::<_, f64>(11)? as f32,
                    variance: r.get::<_, f64>(12)? as f32,
                    placement_skill: r.get::<_, f64>(13)? as f32,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(index, mut row)| {
                if division != 0 {
                    row.rank = index as u32 + 1;
                }
                row
            })
            .collect())
    }
}

fn validate_window(season: &Season) -> Result<()> {
    if season.league_id <= 0 || season.season_id <= 0 {
        return Err(StoreError::Invalid(
            "League and season IDs must be positive",
        ));
    }
    if season.name.is_empty() || season.name.len() > 64 {
        return Err(StoreError::Invalid("season name length"));
    }
    if season.start % 60 != 0 || season.end % 60 != 0 {
        return Err(StoreError::Invalid(
            "season bounds must be whole UTC minutes",
        ));
    }
    // The client compares season times as signed 32-bit: the original loader
    // showed a season ending after 2038-01-19 03:14:07 UTC as already over
    // (LEAGUE06 calendar emulation).
    if season.start < 0 || season.end > MAX_SEASON_END || season.start >= season.end {
        return Err(StoreError::Invalid("season window"));
    }
    Ok(())
}

fn load_rules(conn: &Connection, league_id: i32, revision: u32) -> Result<Option<Rules>> {
    let Some(mut rules) = conn
        .query_row(
            "SELECT team_size, teams_allowed, randoms_allowed, placement_games, entry_division
             FROM rules WHERE league_id=?1 AND revision=?2",
            params![league_id, revision],
            |r| {
                Ok(Rules {
                    league_id,
                    revision,
                    team_size: r.get(0)?,
                    teams_allowed: r.get(1)?,
                    randoms_allowed: r.get(2)?,
                    placement_games: r.get(3)?,
                    entry: match r.get::<_, Option<i32>>(4)? {
                        Some(id) => EntryPolicy::SingleDivision(id),
                        None => EntryPolicy::Placement,
                    },
                    divisions: vec![],
                })
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let mut stmt = conn.prepare(
        "SELECT division_id, skill_min, skill_max, subdivision_size, can_place
         FROM divisions WHERE league_id=?1 AND revision=?2 ORDER BY division_id",
    )?;
    rules.divisions = stmt
        .query_map(params![league_id, revision], |r| {
            Ok(Division {
                id: r.get(0)?,
                skill_min: r.get::<_, f64>(1)? as f32,
                skill_max: r.get::<_, f64>(2)? as f32,
                subdivision_size: r.get(3)?,
                can_place: r.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(Some(rules))
}

fn season_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(Season, String)> {
    Ok((
        Season {
            league_id: r.get(0)?,
            season_id: r.get(1)?,
            name: r.get(2)?,
            start: r.get(3)?,
            end: r.get(4)?,
            rules_revision: r.get(5)?,
            state: SeasonState::Draft,
        },
        r.get(6)?,
    ))
}

fn with_state(row: Option<(Season, String)>) -> Result<Option<Season>> {
    row.map(|(mut season, state)| {
        season.state = SeasonState::parse(&state)?;
        Ok(season)
    })
    .transpose()
}

fn load_season(conn: &Connection, league_id: i32, season_id: i32) -> Result<Option<Season>> {
    with_state(
        conn.query_row(
            "SELECT * FROM seasons WHERE league_id=?1 AND season_id=?2",
            params![league_id, season_id],
            season_row,
        )
        .optional()?,
    )
}

fn active_season(conn: &Connection, league_id: i32, now: i64) -> Result<Option<Season>> {
    // Publication rejects overlap, so at most one row matches.
    with_state(
        conn.query_row(
            "SELECT * FROM seasons WHERE league_id=?1 AND state='published'
             AND start_s <= ?2 AND ?2 < end_s",
            params![league_id, now],
            season_row,
        )
        .optional()?,
    )
}

fn team_name(conn: &Connection, team_id: u64) -> Result<Vec<u8>> {
    conn.query_row(
        "SELECT name FROM teams WHERE team_id=?1",
        params![sql_id(team_id)],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(StoreError::NotFound("team"))
}

fn load_assignment(
    conn: &Connection,
    team_id: u64,
    league_id: i32,
    season_id: i32,
) -> Result<Option<Assignment>> {
    Ok(conn
        .query_row(
            "SELECT subdivision_id, division_id FROM enrollments
             WHERE team_id=?1 AND league_id=?2 AND season_id=?3",
            params![sql_id(team_id), league_id, season_id],
            |r| {
                Ok(Assignment {
                    team_id,
                    league_id,
                    season_id,
                    subdivision_id: wire_id(r.get(0)?),
                    division_id: r.get(1)?,
                })
            },
        )
        .optional()?)
}

fn load_stats(conn: &Connection, team_id: u64, league_id: i32, season_id: i32) -> Result<Stats> {
    conn.query_row(
        "SELECT rank_points, real_points, played, wins, losses, bonus_used, bonus_time,
                streak, career_wins, skill, variance, placement_skill
         FROM stats WHERE team_id=?1 AND league_id=?2 AND season_id=?3",
        params![sql_id(team_id), league_id, season_id],
        |r| {
            Ok(Stats {
                rank_points: r.get(0)?,
                real_points: r.get(1)?,
                played: r.get(2)?,
                wins: r.get(3)?,
                losses: r.get(4)?,
                bonus_used: r.get(5)?,
                bonus_time: r.get(6)?,
                streak: r.get(7)?,
                career_wins: r.get(8)?,
                skill: r.get::<_, f64>(9)? as f32,
                variance: r.get::<_, f64>(10)? as f32,
                placement_skill: r.get::<_, f64>(11)? as f32,
            })
        },
    )
    .optional()?
    .ok_or(StoreError::NotFound("statistics"))
}

/// Decide and apply one upload. Returns the stored wins seen before applying.
fn is_member(conn: &Connection, team_id: u64, player_id: u64) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM team_members WHERE team_id=?1 AND member_id=?2)",
        params![sql_id(team_id), sql_id(player_id)],
        |r| r.get(0),
    )
}

fn apply_upload(
    tx: &Connection,
    caller: u64,
    upload: &StatsUpload,
) -> Result<(Option<i32>, UploadOutcome)> {
    let (Value::Present(team_id), Value::Present(subdivision_id)) =
        (upload.team_id, upload.subdivision_id)
    else {
        return Ok((None, UploadOutcome::NotApplied("null team or subdivision")));
    };
    if !is_member(tx, team_id, caller)? {
        return Ok((None, UploadOutcome::NotApplied("caller not in team")));
    }
    let current: Option<(i32, i32, String, i32)> = tx
        .query_row(
            "SELECT e.league_id, e.season_id, s.state, st.wins
             FROM enrollments e
             JOIN seasons s USING(league_id, season_id)
             JOIN stats st USING(team_id, league_id, season_id)
             WHERE e.team_id=?1 AND e.subdivision_id=?2",
            params![sql_id(team_id), sql_id(subdivision_id)],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((league_id, season_id, state, stored)) = current else {
        return Ok((None, UploadOutcome::NotApplied("team not in subdivision")));
    };
    if SeasonState::parse(&state)? != SeasonState::Published {
        return Ok((Some(stored), UploadOutcome::NotApplied("season not open")));
    }
    let Value::Present(uploaded) = upload.counters[UPLOAD_WINS] else {
        return Ok((Some(stored), UploadOutcome::NotApplied("null wins")));
    };
    if uploaded == stored {
        return Ok((Some(stored), UploadOutcome::Unchanged));
    }
    if Some(uploaded) != stored.checked_add(1) {
        return Ok((
            Some(stored),
            UploadOutcome::NotApplied("wins not stored or stored+1"),
        ));
    }
    tx.execute(
        "UPDATE stats SET wins=?4 WHERE team_id=?1 AND league_id=?2 AND season_id=?3",
        params![sql_id(team_id), league_id, season_id, uploaded],
    )?;
    Ok((Some(stored), UploadOutcome::WinRecorded { wins: uploaded }))
}

/// Lowest-ordinal subdivision of the division with free capacity, creating the
/// next one when all are full. Uncapped divisions (placement, or the single
/// shared division) have exactly one subdivision per season.
fn subdivision_with_room(
    tx: &Connection,
    rules: &Rules,
    season_id: i32,
    division_id: i32,
) -> Result<i64> {
    let capacity = rules.capacity(division_id)?;
    let found: Option<i64> = tx
        .query_row(
            "SELECT d.subdivision_id FROM subdivisions d
             WHERE d.league_id=?1 AND d.season_id=?2 AND d.division_id=?3
             AND (d.capacity IS NULL OR
                  (SELECT COUNT(*) FROM enrollments e WHERE e.subdivision_id=d.subdivision_id)
                  < d.capacity)
             ORDER BY d.ordinal LIMIT 1",
            params![rules.league_id, season_id, division_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = found {
        return Ok(id);
    }
    let ordinal: i64 = tx.query_row(
        "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM subdivisions
         WHERE league_id=?1 AND season_id=?2 AND division_id=?3",
        params![rules.league_id, season_id, division_id],
        |r| r.get(0),
    )?;
    tx.execute(
        "INSERT INTO subdivisions(league_id, season_id, division_id, ordinal, capacity)
         VALUES(?1,?2,?3,?4,?5)",
        params![rules.league_id, season_id, division_id, ordinal, capacity],
    )?;
    Ok(tx.last_insert_rowid())
}

/// `YYYY-MM-DD HH:MM` in UTC, the client calendar's `%04d-%02d-%02d %02d:%02d`.
pub fn format_minute(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    // Civil-from-days (H. Hinnant), proleptic Gregorian.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        secs / 3600,
        secs % 3600 / 60
    )
}
