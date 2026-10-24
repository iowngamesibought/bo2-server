//! Stats / leaderboard service 4 (bdStats), owned implementation.
//!
//! Request layouts come from the client builders at 0x826064..0x826750; all
//! fields are typed (u8=3, i32=7, u32=8, i64=9, u64=10, string=16):
//!   4/1  write      : rows of { u32 leaderboard, u64 entity, u8 write type,
//!                     i64 rating, i32 column x N } (serializers 0x826b40 base,
//!                     0x891178 columns; N is per leaderboard and not sent)
//!   4/3  by entity  : u32 leaderboard, u64 entity x N (count not sent)
//!   4/4  by rank    : u32 leaderboard, u64 first rank (1-based), u32 max
//!                     (caller 0x2c4ccc passes rank+1 and 0x65)
//!   4/5  by pivot   : u32 leaderboard, u64 entity, u32 max (caller 0x2c4a0c
//!                     passes its own identity and 0x65)
//!   4/11 multi      : u32 N, u64 entity x N, u32 M, u32 leaderboard x M
//!
//! Results (deserializers 0x826ba8 base, 0x89120c columns, 0x826dd0 wrapper):
//!   row     : u64 entity, i64 rating, u64 rank, string name (<= 64),
//!             u32 field_6c, i32 column x N
//!   wrapper : u32 leaderboard, u32 absent, then a row (absent = 0) or the
//!             u64 entity (absent != 0); used only by 4/11
//!
//! Owned policy, not a recovered official algorithm:
//! - A connection may write only rows for its own identity.
//! - Write types: 0 replace, 1 add (rating and columns), 2 keep the higher
//!   rating, 3 keep the lower rating. TODO(stats-writetype): the meaning of the
//!   type byte is server-side in the original service. Determine it from real
//!   match stats during console testing; the server logs every stats task in
//!   full (`stats.request`) for that.
//! - Leaderboard reads are left at this first version for now (user decision
//!   2026-10-24); correctness of the writes is the priority.
//! - Rank is 1-based by rating descending, then identity ascending.
//! - field_6c is not consumed by any traced reader; it is sent as 0.
//!   TODO(stats-field6c): identify it.
use crate::league_store::{LeaderboardRow, Store};
use crate::protocol::{ProtocolError, empty_success, results_reply, task_tail_valid};

type Result<T> = std::result::Result<T, crate::league_service::ServiceError>;

/// Column storage of the client row object: +0x70 .. +0x98, 4 bytes each.
pub const MAX_COLUMNS: usize = 10;
/// The client asks for at most 0x65 rows per page.
pub const MAX_PAGE: u32 = 0x65;
const MAX_WRITE_ROWS: usize = 64;
const MAX_IDS: usize = 100;
const MAX_LEADERBOARDS: usize = 64;
const MAX_NAME_BYTES: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub struct WriteRow {
    pub leaderboard: u32,
    pub entity: u64,
    pub write_type: u8,
    pub rating: i64,
    pub columns: Vec<i32>,
}

#[derive(Debug, PartialEq)]
pub enum Request {
    Write(Vec<WriteRow>),
    ByEntities {
        leaderboard: u32,
        entities: Vec<u64>,
    },
    ByRank {
        leaderboard: u32,
        first: u64,
        max: u32,
    },
    ByPivot {
        leaderboard: u32,
        entity: u64,
        max: u32,
    },
    Multi {
        entities: Vec<u64>,
        leaderboards: Vec<u32>,
    },
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> std::result::Result<&'a [u8], ProtocolError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(ProtocolError("stats length overflow"))?;
        let v = self
            .bytes
            .get(self.at..end)
            .ok_or(ProtocolError("truncated stats request"))?;
        self.at = end;
        Ok(v)
    }
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }
    fn tagged<const N: usize>(&mut self, tag: u8) -> std::result::Result<[u8; N], ProtocolError> {
        if self.take(1)?[0] != tag {
            return Err(ProtocolError("wrong stats field tag"));
        }
        Ok(self.take(N)?.try_into().unwrap())
    }
    fn u8(&mut self) -> std::result::Result<u8, ProtocolError> {
        Ok(self.tagged::<1>(3)?[0])
    }
    fn i32(&mut self) -> std::result::Result<i32, ProtocolError> {
        Ok(i32::from_le_bytes(self.tagged(7)?))
    }
    fn u32(&mut self) -> std::result::Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(self.tagged(8)?))
    }
    fn i64(&mut self) -> std::result::Result<i64, ProtocolError> {
        Ok(i64::from_le_bytes(self.tagged(9)?))
    }
    fn u64(&mut self) -> std::result::Result<u64, ProtocolError> {
        Ok(u64::from_le_bytes(self.tagged(10)?))
    }
    /// Tagged u64 values until the next field is not a u64.
    fn u64_list(&mut self, max: usize) -> std::result::Result<Vec<u64>, ProtocolError> {
        let mut out = vec![];
        while self.peek() == Some(10) {
            out.push(self.u64()?);
            if out.len() > max {
                return Err(ProtocolError("too many stats entities"));
            }
        }
        Ok(out)
    }
}

/// Parse a service 4 task (`4, 3, op, body, 0x00, padding`) after MAC checks.
pub fn parse_task_payload(data: &[u8]) -> std::result::Result<Request, ProtocolError> {
    if data.len() < 3 || data[0] != 4 || data[1] != 3 {
        return Err(ProtocolError("invalid stats task header"));
    }
    let mut r = Reader { bytes: data, at: 3 };
    let request = match data[2] {
        1 => {
            let mut rows = vec![];
            while r.peek() == Some(8) {
                let mut row = WriteRow {
                    leaderboard: r.u32()?,
                    entity: r.u64()?,
                    write_type: r.u8()?,
                    rating: r.i64()?,
                    columns: vec![],
                };
                while r.peek() == Some(7) {
                    row.columns.push(r.i32()?);
                    if row.columns.len() > MAX_COLUMNS {
                        return Err(ProtocolError("too many stats columns"));
                    }
                }
                rows.push(row);
                if rows.len() > MAX_WRITE_ROWS {
                    return Err(ProtocolError("too many stats rows"));
                }
            }
            if rows.is_empty() {
                return Err(ProtocolError("empty stats write"));
            }
            Request::Write(rows)
        }
        3 => {
            let leaderboard = r.u32()?;
            let entities = r.u64_list(MAX_IDS)?;
            if entities.is_empty() {
                return Err(ProtocolError("stats read without entities"));
            }
            Request::ByEntities {
                leaderboard,
                entities,
            }
        }
        4 => Request::ByRank {
            leaderboard: r.u32()?,
            first: r.u64()?,
            max: r.u32()?,
        },
        5 => Request::ByPivot {
            leaderboard: r.u32()?,
            entity: r.u64()?,
            max: r.u32()?,
        },
        11 => {
            let n = r.u32()? as usize;
            if n == 0 || n > MAX_IDS {
                return Err(ProtocolError("stats entity count outside budget"));
            }
            let entities = (0..n)
                .map(|_| r.u64())
                .collect::<std::result::Result<_, _>>()?;
            let m = r.u32()? as usize;
            if m == 0 || m > MAX_LEADERBOARDS {
                return Err(ProtocolError("stats leaderboard count outside budget"));
            }
            let leaderboards = (0..m)
                .map(|_| r.u32())
                .collect::<std::result::Result<_, _>>()?;
            Request::Multi {
                entities,
                leaderboards,
            }
        }
        _ => return Err(ProtocolError("unsupported stats operation")),
    };
    if !task_tail_valid(data, r.at) {
        return Err(ProtocolError("non-minimal stats padding"));
    }
    Ok(request)
}

/// One result row for deserializer 0x89120c.
pub fn row_result(
    row: &LeaderboardRow,
    name: &[u8],
) -> std::result::Result<Vec<u8>, ProtocolError> {
    if name.len() > MAX_NAME_BYTES || name.contains(&0) {
        return Err(ProtocolError("stats entity name"));
    }
    if row.columns.len() > MAX_COLUMNS {
        return Err(ProtocolError("too many stats columns"));
    }
    let mut out = Vec::with_capacity(48 + name.len() + 5 * row.columns.len());
    out.push(10);
    out.extend_from_slice(&row.entity.to_le_bytes());
    out.push(9);
    out.extend_from_slice(&row.rating.to_le_bytes());
    out.push(10);
    out.extend_from_slice(&row.rank.to_le_bytes());
    out.push(16);
    out.extend_from_slice(name);
    out.push(0);
    out.push(8);
    out.extend_from_slice(&0u32.to_le_bytes());
    for column in &row.columns {
        out.push(7);
        out.extend_from_slice(&column.to_le_bytes());
    }
    Ok(out)
}

/// One 4/11 result for wrapper deserializer 0x826dd0.
pub fn wrapper_result(
    leaderboard: u32,
    entity: u64,
    row: Option<(&LeaderboardRow, &[u8])>,
) -> std::result::Result<Vec<u8>, ProtocolError> {
    let mut out = vec![8];
    out.extend_from_slice(&leaderboard.to_le_bytes());
    out.push(8);
    match row {
        Some((row, name)) => {
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend(row_result(row, name)?);
        }
        None => {
            out.extend_from_slice(&1u32.to_le_bytes());
            out.push(10);
            out.extend_from_slice(&entity.to_le_bytes());
        }
    }
    Ok(out)
}

fn rows_reply(store: &Store, transaction: u64, op: u8, rows: &[LeaderboardRow]) -> Result<Vec<u8>> {
    let results = rows
        .iter()
        .map(|row| row_result(row, &store.player_name(row.entity)?).map_err(Into::into))
        .collect::<Result<Vec<_>>>()?;
    Ok(results_reply(transaction, op, &results)?)
}

/// Answer one service 4 task for the connection's bound identity.
pub fn handle_task(
    store: &mut Store,
    caller: u64,
    transaction: u64,
    data: &[u8],
    now: i64,
) -> Result<Vec<u8>> {
    let op = data[2];
    match parse_task_payload(data)? {
        Request::Write(rows) => {
            if rows.iter().any(|row| row.entity != caller) {
                return Err(crate::league_service::ServiceError::Forbidden(
                    "stats write for another identity",
                ));
            }
            for row in &rows {
                store.write_leaderboard_row(row, now)?;
            }
            Ok(empty_success(transaction, op))
        }
        Request::ByEntities {
            leaderboard,
            entities,
        } => {
            let mut rows = vec![];
            for entity in entities {
                if let Some(row) = store.leaderboard_row(leaderboard, entity)? {
                    rows.push(row);
                }
            }
            rows_reply(store, transaction, op, &rows)
        }
        Request::ByRank {
            leaderboard,
            first,
            max,
        } => {
            let rows = store.leaderboard_page(leaderboard, first.max(1), max.min(MAX_PAGE))?;
            rows_reply(store, transaction, op, &rows)
        }
        Request::ByPivot {
            leaderboard,
            entity,
            max,
        } => {
            let max = max.min(MAX_PAGE);
            let rows = match store.leaderboard_row(leaderboard, entity)? {
                // Center the page on the pivot's rank.
                Some(row) => {
                    let first = row.rank.saturating_sub(u64::from(max / 2)).max(1);
                    store.leaderboard_page(leaderboard, first, max)?
                }
                None => vec![],
            };
            rows_reply(store, transaction, op, &rows)
        }
        Request::Multi {
            entities,
            leaderboards,
        } => {
            let mut results = vec![];
            for &entity in &entities {
                for &leaderboard in &leaderboards {
                    let row = store.leaderboard_row(leaderboard, entity)?;
                    let name = store.player_name(entity)?;
                    results.push(wrapper_result(
                        leaderboard,
                        entity,
                        row.as_ref().map(|row| (row, name.as_slice())),
                    )?);
                }
            }
            Ok(results_reply(transaction, op, &results)?)
        }
    }
}
