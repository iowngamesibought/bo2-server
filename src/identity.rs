//! Player identity as the client computes it (LEAGUE06).
//!
//! The solo League team request (0x2cf468) sends `0x532ae4(slot)`, which reads
//! slot+0x88 at 0x26c0658+slot*0x108. DEC05 traced that cache to 0x81e378: Tiger
//! of the lowercased ASCII online ID, first eight digest bytes little-endian. It
//! is not taken from the game-auth reply, so a private server can derive the same
//! value from the online ID in the platform ticket.
//!
//! The ticket's platform provider signature is NOT verified. The extracted online ID binds a
//! connection consistently to the identity the client will claim; it does not
//! prove account ownership. Raw tickets are never stored.
use crate::protocol::ProtocolError;
use tiger::{Digest, Tiger};

type Result<T> = std::result::Result<T, ProtocolError>;

/// Version word of the observed official ticket (RUN-client-20261023-01).
pub const TICKET_VERSION_3_1: u32 = 0x3100_0000;
/// platform online IDs are 3..=16 characters of `[A-Za-z0-9_-]`.
const ONLINE_ID_MIN: usize = 3;
const ONLINE_ID_MAX: usize = 16;

/// The client's name-derived 64-bit identity (0x81e378).
pub fn name_player_id(online_id: &[u8]) -> u64 {
    let lower = online_id.to_ascii_lowercase();
    let digest = Tiger::digest(&lower);
    u64::from_le_bytes(digest[..8].try_into().unwrap())
}

fn entry(ticket: &[u8], at: usize, end: usize) -> Result<(u16, &[u8], usize)> {
    let header = ticket
        .get(at..at + 4)
        .filter(|_| at + 4 <= end)
        .ok_or(ProtocolError("short ticket entry"))?;
    let kind = u16::from_be_bytes([header[0], header[1]]);
    let len = usize::from(u16::from_be_bytes([header[2], header[3]]));
    let next = at + 4 + len;
    if next > end {
        return Err(ProtocolError("ticket entry overruns section"));
    }
    Ok((kind, &ticket[at + 4..next], next))
}

/// Online ID from a v3.1 platform ticket body.
///
/// Layout checked against the structure of the one observed official ticket:
/// big-endian `version, size`, then body section 0x3000 whose first six entries
/// are serial (8), issuer (1), issued (7), expiry (7), user ID (2) and online ID
/// (4, 32 bytes NUL-padded). Anything else is rejected rather than guessed.
pub fn ticket_online_id(ticket: &[u8]) -> Result<Vec<u8>> {
    if ticket.len() < 12 {
        return Err(ProtocolError("short ticket"));
    }
    let version = u32::from_be_bytes(ticket[..4].try_into().unwrap());
    if version != TICKET_VERSION_3_1 {
        return Err(ProtocolError("unsupported ticket version"));
    }
    let size = u32::from_be_bytes(ticket[4..8].try_into().unwrap()) as usize;
    if size.checked_add(8) != Some(ticket.len()) {
        return Err(ProtocolError("ticket size mismatch"));
    }
    let (section, body, _) = entry(ticket, 8, ticket.len())?;
    if section != 0x3000 {
        return Err(ProtocolError("ticket body section missing"));
    }
    let body_start = 12;
    let body_end = body_start + body.len();
    let mut at = body_start;
    let mut field = &[][..];
    for expected in [8u16, 1, 7, 7, 2, 4] {
        let (kind, value, next) = entry(ticket, at, body_end)?;
        if kind != expected {
            return Err(ProtocolError("unexpected ticket field order"));
        }
        field = value;
        at = next;
    }
    if field.len() != 32 {
        return Err(ProtocolError("online ID field is not 32 bytes"));
    }
    let len = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    let (name, padding) = field.split_at(len);
    if padding.iter().any(|&b| b != 0)
        || !(ONLINE_ID_MIN..=ONLINE_ID_MAX).contains(&name.len())
        || !name
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
    {
        return Err(ProtocolError("invalid online ID"));
    }
    Ok(name.to_vec())
}
