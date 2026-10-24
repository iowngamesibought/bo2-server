//! Remaining online-startup services (LEAGUE06): geolocation 0x1b/3, public
//! profile get 8/1 and set 8/3, presence 0x44/1.
//!
//! Layouts from the client:
//! - 0x1b/3 (builder 0x818b14): no request fields. Result (deserializer
//!   0x809ee4 via 0x818d68): strings bounded 3, 0x41, 0x41, 0x81; f32, f32;
//!   u32 x4 (+0x114..+0x120). The four u32 later become the advertised
//!   geographic attributes, with -1 replaced by the client's null sentinel.
//! - 8/1 (builder 0x8236b4): tagged u64 identities until the body ends.
//!   Result per profile (0x1aa848): u64 owner, i32 version, blob <= 0x400.
//! - 8/3 (writer 0x1aa7ec): i32 version, blob <= 0x400; no result object.
//! - 0x44/1 (builder 0x825d38): u64, blob; no result object.
//!
//! Owned policy: all players share a synthetic private matchmaking region
//! (empty strings, zero coordinates and zero region IDs). These IDs describe
//! our private pool, not a real location. Unknown (-1) IDs are unsafe here:
//! 0x52d514 turns them into INT_MAX in advertisements, while 0x15b6b8 compares
//! them against the unchanged local -1 IDs. League's geographic minimum then
//! rejects otherwise compatible peers. Profiles are stored per caller
//! and readable by anyone; presence is acknowledged and not stored.
use crate::league_service::ServiceError;
use crate::league_store::Store;
use crate::protocol::{ProtocolError, empty_success, results_reply};

type Result<T> = std::result::Result<T, ServiceError>;

pub const MAX_PROFILE_BYTES: usize = 0x400;
/// Owned bound on identities per profile request.
pub const MAX_PROFILE_IDS: usize = 64;

/// Virtual region shared by this private player pool, not a country lookup.
/// Must survive the client's -1 -> INT_MAX advertisement conversion unchanged.
pub const PRIVATE_MATCHMAKING_REGION: u32 = 0;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> std::result::Result<&'a [u8], ProtocolError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(ProtocolError("startup length"))?;
        let v = self
            .bytes
            .get(self.at..end)
            .ok_or(ProtocolError("truncated startup request"))?;
        self.at = end;
        Ok(v)
    }
    fn tagged(&mut self, tag: u8, n: usize) -> std::result::Result<&'a [u8], ProtocolError> {
        if self.take(1)?[0] != tag {
            return Err(ProtocolError("wrong startup field tag"));
        }
        self.take(n)
    }
    fn blob(&mut self, max: usize) -> std::result::Result<&'a [u8], ProtocolError> {
        if self.take(1)?[0] != 19 {
            return Err(ProtocolError("wrong startup blob tag"));
        }
        let len = u32::from_le_bytes(self.tagged(8, 4)?.try_into().unwrap()) as usize;
        if len > max {
            return Err(ProtocolError("startup blob too long"));
        }
        self.take(len)
    }
}

/// Bytes after the grammar must be exactly the minimal CBC padding.
fn check_padding(data: &[u8], body_used: usize) -> std::result::Result<(), ProtocolError> {
    let used = 3 + body_used;
    if !crate::protocol::task_tail_valid(data, used) {
        return Err(ProtocolError("non-minimal startup task padding"));
    }
    Ok(())
}

fn string(out: &mut Vec<u8>, s: &str) {
    out.push(16);
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

/// Geolocation body with a shared private matchmaking region.
pub fn geolocation_result() -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..4 {
        string(&mut out, "");
    }
    for _ in 0..2 {
        out.push(13);
        out.extend_from_slice(&0f32.to_le_bytes());
    }
    for _ in 0..4 {
        out.push(8);
        out.extend_from_slice(&PRIVATE_MATCHMAKING_REGION.to_le_bytes());
    }
    out
}

/// One profile result body (deserializer 0x1aa848).
pub fn profile_result(
    owner: u64,
    version: i32,
    data: &[u8],
) -> std::result::Result<Vec<u8>, ProtocolError> {
    if data.len() > MAX_PROFILE_BYTES {
        return Err(ProtocolError("profile exceeds 0x400 bytes"));
    }
    let mut out = Vec::with_capacity(20 + data.len());
    out.push(10);
    out.extend_from_slice(&owner.to_le_bytes());
    out.push(7);
    out.extend_from_slice(&version.to_le_bytes());
    out.push(19);
    out.push(8);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    Ok(out)
}

/// Handle one task for services 0x1b, 8 and 0x44.
pub fn handle_task(
    store: &mut Store,
    caller: u64,
    transaction: u64,
    data: &[u8],
    now: i64,
) -> Result<Vec<u8>> {
    if data.len() < 3 || data[1] != 3 {
        return Err(ProtocolError("invalid startup task header").into());
    }
    let (service, op) = (data[0], data[2]);
    let mut r = Reader {
        bytes: &data[3..],
        at: 0,
    };
    match (service, op) {
        (0x1b, 3) => {
            check_padding(data, 0)?;
            Ok(results_reply(transaction, op, &[geolocation_result()])?)
        }
        (8, 1) => {
            // Tagged u64 IDs fill the body; padding (< 8 bytes) cannot hold one.
            let mut ids = Vec::new();
            while r.bytes.len() - r.at >= 9 && r.bytes[r.at] == 10 {
                ids.push(u64::from_le_bytes(r.tagged(10, 8)?.try_into().unwrap()));
                if ids.len() > MAX_PROFILE_IDS {
                    return Err(ProtocolError("too many profile identities").into());
                }
            }
            check_padding(data, r.at)?;
            let mut results = Vec::new();
            for id in ids {
                if let Some((version, blob)) = store.profile(id)? {
                    results.push(profile_result(id, version, &blob)?);
                }
            }
            Ok(results_reply(transaction, op, &results)?)
        }
        (8, 3) => {
            let version = i32::from_le_bytes(r.tagged(7, 4)?.try_into().unwrap());
            let blob = r.blob(MAX_PROFILE_BYTES)?.to_vec();
            check_padding(data, r.at)?;
            store.put_profile(caller, version, &blob, now)?;
            Ok(empty_success(transaction, op))
        }
        (0x44, 1) => {
            r.tagged(10, 8)?;
            r.blob(0x400)?;
            check_padding(data, r.at)?;
            Ok(empty_success(transaction, op))
        }
        // 0x26/2 and /4 (builders 0x812100, 0x8122b4) attach no result. Op 4 was
        // observed first on a real console (blob16, u32 x2, u64 x3, blob6 holding
        // the console MAC). Acknowledged and deliberately not stored.
        (0x26, 2 | 4) if data.len() <= 1024 => Ok(empty_success(transaction, op)),
        // linked-account / linked-account status queries seen on a real console before
        // "Connecting to Online Service" completes without the LAN workaround:
        // 3/27 (descriptor MembershipQuery, u64 own ID), 0x20/4 (linked-account
        // subscriber refresh), and empty-body 0x1f/3, 0x21/2, 0x23/6, 0x24/7.
        // Zero results means "no membership / nothing linked": truthful, and the
        // original parser leaves result objects untouched at count 0. Error
        // replies left the game waiting. Experimental until a console run confirms.
        (3, 27) | (0x20, 4) | (0x1f, 3) | (0x21, 2) | (0x23, 6) | (0x24, 7) if data.len() <= 64 => {
            Ok(empty_success(transaction, op))
        }
        // 0x43/5: client debug/event log line (seen on a real console in a
        // party: u32, u32, u8, string "Getting changed availableMapPackFlags
        // in Party_ReadMember..."). Acknowledged without storing.
        (0x43, 5) if data.len() <= 0x400 => Ok(empty_success(transaction, op)),
        _ => Err(ProtocolError("unsupported startup operation").into()),
    }
}
