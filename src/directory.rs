//! Session directory, service 0x15 (LEAGUE06).
//!
//! Request layouts from the client (LEAGUE04 objects, LEAGUE06 serializers):
//!   create 1  : advertisement
//!   modify 12 : blob8 directory ID, u32 (+0x114), advertisement
//!   delete 3  : blob8 directory ID
//!   search 5  : u32 discriminator, u32, u32 limit, query(discriminator)
//! Advertisement (0x820dc4 then 0x1a23b4): blob address (+0x0c), u32 +0x118,
//! u32 +0x110, u64 peer ID +0x120, blob16 peer key +0x128, 8 i32 +0x13c..0x158,
//! f32 +0x15c, 4 i32 +0x160..0x16c. Query discriminator 2 (0x1a2a94): 7 i32
//! +0x10..0x28, 2 f32, 4 i32, 4 f32. i32/f32 sentinels travel as null (tag 20).
//! Create returns one blob8 directory ID; modify and delete attach no result.
//!
//! Directory policy here is owned, not a recovered official algorithm: leases
//! live in memory only, records are owned by the creating identity, and search
//! matches the advertisement fields the client fills from the same sources as
//! the query (+0x144/+0x14, +0x14c/+0x1c, playlist +0x150/+0x20).
use crate::league_directory::{SearchResult, create_id_result};
use crate::protocol::{ProtocolError, empty_success, results_reply};
use rand::{RngCore, rngs::OsRng};
use std::collections::HashMap;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, ProtocolError>;

/// Native i32/f32 null sentinels (writers 0x7e6f70, 0x7e72b0).
const I32_NULL: i32 = 0x7fff_ffff;
const F32_NULL: f32 = 2_147_483_648.0;
/// The client's normal address producer (0x820bc0) emits 37 bytes, and the
/// peer consumer requires exactly that length.
pub const ADDRESS_BYTES: usize = 37;
/// Search capacity the client passes (50) and our hard cap.
pub const MAX_RESULTS: usize = 50;
/// Owned lease: a session not refreshed for this long is dropped.
pub const LEASE: Duration = Duration::from_secs(600);
/// Owned bound on live sessions per identity and overall.
const MAX_SESSIONS_PER_OWNER: usize = 4;
const MAX_SESSIONS: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub struct Advertisement {
    pub address: [u8; ADDRESS_BYTES],
    pub field_118: u32,
    pub field_110: u32,
    pub peer_id: u64,
    pub peer_key: [u8; 16],
    /// +0x13c..+0x158. +0x144 game mode source d707e4, +0x14c from 0x383a5c,
    /// +0x150 transformed playlist (LEAGUE04 producers).
    pub attributes: [i32; 8],
    pub party_aggregate: f32,
    pub geo: [i32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    /// +0x10..+0x28: 1, mode d707e4, map-pack mask, 0x383a5c, playlist,
    /// searchSessionIsEmpty, requested slots.
    pub ints: [i32; 7],
    pub floats: [f32; 2],
    pub geo: [i32; 4],
    pub geo_weights: [f32; 4],
}

#[derive(Debug, PartialEq)]
pub enum Request {
    Create(Advertisement),
    Modify {
        id: [u8; 8],
        field_114: u32,
        advertisement: Advertisement,
    },
    Delete {
        id: [u8; 8],
    },
    Search {
        limit: u32,
        query: Query,
    },
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(ProtocolError("directory length"))?;
        let v = self
            .bytes
            .get(self.at..end)
            .ok_or(ProtocolError("truncated directory request"))?;
        self.at = end;
        Ok(v)
    }
    fn tag(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        match self.tag()? {
            8 => Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())),
            20 => Ok(u32::MAX),
            _ => Err(ProtocolError("wrong directory u32 tag")),
        }
    }
    fn i32(&mut self) -> Result<i32> {
        match self.tag()? {
            7 => Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap())),
            20 => Ok(I32_NULL),
            _ => Err(ProtocolError("wrong directory i32 tag")),
        }
    }
    fn f32(&mut self) -> Result<f32> {
        let v = match self.tag()? {
            13 => f32::from_le_bytes(self.take(4)?.try_into().unwrap()),
            20 => F32_NULL,
            _ => return Err(ProtocolError("wrong directory f32 tag")),
        };
        if !v.is_finite() {
            return Err(ProtocolError("non-finite directory float"));
        }
        Ok(v)
    }
    fn u64(&mut self) -> Result<u64> {
        if self.tag()? != 10 {
            return Err(ProtocolError("wrong directory u64 tag"));
        }
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn blob(&mut self, max: usize) -> Result<&'a [u8]> {
        if self.tag()? != 19 {
            return Err(ProtocolError("wrong directory blob tag"));
        }
        let len = self.u32()? as usize;
        if len > max {
            return Err(ProtocolError("directory blob too long"));
        }
        self.take(len)
    }
    fn id(&mut self) -> Result<[u8; 8]> {
        self.blob(8)?
            .try_into()
            .map_err(|_| ProtocolError("directory ID must be 8 bytes"))
    }
    fn advertisement(&mut self) -> Result<Advertisement> {
        let address = self
            .blob(255)?
            .try_into()
            .map_err(|_| ProtocolError("advertised address must be 37 bytes"))?;
        let field_118 = self.u32()?;
        let field_110 = self.u32()?;
        let peer_id = self.u64()?;
        let peer_key = self
            .blob(16)?
            .try_into()
            .map_err(|_| ProtocolError("peer key must be 16 bytes"))?;
        let mut attributes = [0; 8];
        for v in &mut attributes {
            *v = self.i32()?;
        }
        let party_aggregate = self.f32()?;
        let mut geo = [0; 4];
        for v in &mut geo {
            *v = self.i32()?;
        }
        if peer_id == 0 {
            return Err(ProtocolError("zero peer identifier"));
        }
        Ok(Advertisement {
            address,
            field_118,
            field_110,
            peer_id,
            peer_key,
            attributes,
            party_aggregate,
            geo,
        })
    }
    fn query(&mut self) -> Result<Query> {
        let mut q = Query {
            ints: [0; 7],
            floats: [0.0; 2],
            geo: [0; 4],
            geo_weights: [0.0; 4],
        };
        for v in &mut q.ints {
            *v = self.i32()?;
        }
        for v in &mut q.floats {
            *v = self.f32()?;
        }
        for v in &mut q.geo {
            *v = self.i32()?;
        }
        for v in &mut q.geo_weights {
            *v = self.f32()?;
        }
        Ok(q)
    }
}

/// Parse a 0x15 task payload after MAC verification, allowing only minimal
/// CBC padding after the grammar.
pub fn parse_task_payload(data: &[u8]) -> Result<Request> {
    if data.len() < 3 || data[0] != 0x15 || data[1] != 3 {
        return Err(ProtocolError("not a directory task"));
    }
    let mut r = Reader {
        bytes: &data[3..],
        at: 0,
    };
    let request = match data[2] {
        1 => Request::Create(r.advertisement()?),
        12 => Request::Modify {
            id: r.id()?,
            field_114: r.u32()?,
            advertisement: r.advertisement()?,
        },
        3 => Request::Delete { id: r.id()? },
        5 => {
            let discriminator = r.u32()?;
            r.u32()?;
            let limit = r.u32()?;
            if discriminator != 2 {
                return Err(ProtocolError("unsupported search discriminator"));
            }
            Request::Search {
                limit,
                query: r.query()?,
            }
        }
        _ => return Err(ProtocolError("unsupported directory operation")),
    };
    let used = 3 + r.at;
    if !crate::protocol::task_tail_valid(data, used) {
        return Err(ProtocolError("non-minimal directory task padding"));
    }
    Ok(request)
}

struct Session {
    owner: u64,
    advertisement: Advertisement,
    field_114: u32,
    refreshed: Instant,
}

/// In-memory directory. Records are leases: never persisted, dropped when the
/// lease lapses or the owner's lobby connection closes.
#[derive(Default)]
pub struct Directory {
    sessions: HashMap<[u8; 8], Session>,
}

impl Directory {
    fn expire(&mut self, now: Instant) {
        self.sessions
            .retain(|_, s| now.duration_since(s.refreshed) < LEASE);
    }

    /// Remove every session owned by `owner` (lobby disconnect).
    pub fn drop_owner(&mut self, owner: u64) {
        self.sessions.retain(|_, s| s.owner != owner);
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Handle one 0x15 task for `caller`, returning the reply envelope.
    pub fn handle_task(
        &mut self,
        caller: u64,
        transaction: u64,
        data: &[u8],
        now: Instant,
    ) -> Result<Vec<u8>> {
        self.expire(now);
        let op = data[2];
        match parse_task_payload(data)? {
            Request::Create(advertisement) => {
                let owned = self.sessions.values().filter(|s| s.owner == caller).count();
                if owned >= MAX_SESSIONS_PER_OWNER || self.sessions.len() >= MAX_SESSIONS {
                    return Err(ProtocolError("directory session budget"));
                }
                let mut id = [0u8; 8];
                loop {
                    OsRng
                        .try_fill_bytes(&mut id)
                        .map_err(|_| ProtocolError("OS random source failed"))?;
                    if id != [0; 8] && !self.sessions.contains_key(&id) {
                        break;
                    }
                }
                self.sessions.insert(
                    id,
                    Session {
                        owner: caller,
                        advertisement,
                        field_114: 0,
                        refreshed: now,
                    },
                );
                results_reply(transaction, op, &[create_id_result(&id)])
            }
            Request::Modify {
                id,
                field_114,
                advertisement,
            } => {
                let session = self
                    .sessions
                    .get_mut(&id)
                    .filter(|s| s.owner == caller)
                    .ok_or(ProtocolError("unknown or foreign directory record"))?;
                session.advertisement = advertisement;
                session.field_114 = field_114;
                session.refreshed = now;
                Ok(empty_success(transaction, op))
            }
            Request::Delete { id } => {
                if self.sessions.get(&id).is_some_and(|s| s.owner != caller) {
                    return Err(ProtocolError("foreign directory record"));
                }
                self.sessions.remove(&id);
                Ok(empty_success(transaction, op))
            }
            Request::Search { limit, query } => {
                let limit = (limit as usize).min(MAX_RESULTS);
                let mut matches: Vec<(&[u8; 8], &Session)> = self
                    .sessions
                    .iter()
                    .filter(|(_, s)| s.owner != caller && matches_query(s, &query))
                    .collect();
                // Owned ordering: most recently refreshed first, then ID.
                matches.sort_by(|a, b| b.1.refreshed.cmp(&a.1.refreshed).then(a.0.cmp(b.0)));
                let results = matches
                    .into_iter()
                    .take(limit)
                    .map(|(id, s)| search_result(id, s).encode())
                    .collect::<Result<Vec<_>>>()?;
                results_reply(transaction, op, &results)
            }
        }
    }
}

/// Owned matching policy. Same mode (+0x144/+0x14), same 0x383a5c value
/// (+0x14c/+0x1c), same playlist (+0x150/+0x20), and enough free slots when
/// +0x110 (capacity) and +0x114 (occupancy, from modify) are both known.
fn matches_query(session: &Session, query: &Query) -> bool {
    let a = &session.advertisement.attributes;
    if a[2] != query.ints[1] || a[4] != query.ints[3] || a[5] != query.ints[4] {
        return false;
    }
    // +0x110 as capacity is an inference; never filter on 0 or the null value.
    let capacity = session.advertisement.field_110;
    let requested = query.ints[6].max(1) as u32;
    capacity == 0 || capacity == u32::MAX || session.field_114.saturating_add(requested) <= capacity
}

fn search_result(id: &[u8; 8], s: &Session) -> SearchResult {
    let a = &s.advertisement;
    SearchResult {
        address: a.address,
        directory_id: *id,
        field_118: a.field_118,
        field_110: a.field_110,
        field_114: s.field_114,
        peer_id: a.peer_id,
        peer_key: a.peer_key,
        fields_13c_through_158: a.attributes,
        field_15c: a.party_aggregate,
        fields_160_through_16c: a.geo,
    }
}
