//! League result-body codecs for the supported client profile (LEAGUE04/06).
//!
//! These encode one non-null result object, without a task envelope. They do not
//! implement enrollment, ranking policy, persistence, dispatch or authentication.
//! Synthetic vectors were accepted by the original client's result decoders.
use crate::protocol::ProtocolError;

type Result<T> = std::result::Result<T, ProtocolError>;

/// Native string buffers are 0x41 bytes. The reader truncates silently, so the
/// encoder enforces the bound instead of relying on the client.
pub const MAX_NAME_BYTES: usize = 64;
/// Team-record blob capacity (0x400). Oversize blobs desynchronize the reader.
pub const MAX_TEAM_DATA_BYTES: usize = 1024;
/// The 0x51/8 decoder requires each array count to be strictly less than 16.
pub const MAX_TEAM_MEMBERS: usize = 15;

fn field(out: &mut Vec<u8>, tag: u8, bytes: &[u8]) {
    out.push(tag);
    out.extend_from_slice(bytes);
}

fn check_name(name: &[u8]) -> Result<()> {
    if name.len() > MAX_NAME_BYTES || name.contains(&0) {
        return Err(ProtocolError("invalid League name"));
    }
    Ok(())
}

fn string(out: &mut Vec<u8>, name: &[u8]) {
    field(out, 16, name);
    out.push(0);
}

fn blob(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(19);
    field(out, 8, &(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// Array header read by 0x7e6c9c: marker `type+100`, the writer's tag8 byte (not
/// validated by the reader), raw LE32 payload byte length (not enforced by the
/// reader), raw LE32 count. The elements that follow are untagged.
fn array_header(out: &mut Vec<u8>, element_type: u8, byte_len: usize, count: usize) {
    out.extend_from_slice(&[element_type + 100, 8]);
    out.extend_from_slice(&(byte_len as u32).to_le_bytes());
    out.extend_from_slice(&(count as u32).to_le_bytes());
}

/// Result object for service 0x51/1; the caller owns stable team-ID allocation.
pub fn team_id_result(team_id: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    field(&mut out, 10, &team_id.to_le_bytes());
    out
}

/// One subdivision result for service 0x51/3.
///
/// `field_08` is the first u64 in the native result. It may be the team ID, but
/// that meaning has not been established by a direct consumer.
///
/// `metadata` is a bounded wire blob. Its versioned text grammar and relationship
/// to a configured division must be validated by the business layer before use.
/// An empty or syntactically valid blob does not establish placement success.
pub fn subdivision_result(
    field_08: u64,
    league_id: u64,
    subdivision_id: u64,
    metadata: &[u8],
) -> Result<Vec<u8>> {
    if metadata.len() > 64 {
        return Err(ProtocolError("League metadata exceeds 64 bytes"));
    }
    let mut out = Vec::with_capacity(33 + metadata.len());
    for value in [field_08, league_id, subdivision_id] {
        field(&mut out, 10, &value.to_le_bytes());
    }
    blob(&mut out, metadata);
    Ok(out)
}

/// Statistics result decoded by the original function at 0x81de10.
/// Field labels follow client consumers, not an assumed original ranking policy.
#[derive(Clone, Debug, PartialEq)]
pub struct StatsResult {
    pub team_id: u64,
    pub subdivision_id: u64,
    /// Raw non-NUL bytes, at most 64. The encoder appends the wire terminator.
    pub name: Vec<u8>,
    /// Meaning of native object +0x58 is not yet established.
    pub field_58: u32,
    pub rank: u32,
    /// Also exposed as "rating" in one UI path; distinct from float `skill`.
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

impl StatsResult {
    /// Encode a response, never the different 0x52/1 upload schema.
    /// Non-finite floats are rejected as a private implementation input policy.
    pub fn encode(&self) -> Result<Vec<u8>> {
        check_name(&self.name)?;
        let skills = [self.skill, self.variance, self.placement_skill];
        if skills.iter().any(|value| !value.is_finite()) {
            return Err(ProtocolError("non-finite League skill"));
        }
        let mut out = Vec::with_capacity(94 + self.name.len());
        for id in [self.team_id, self.subdivision_id] {
            field(&mut out, 10, &id.to_le_bytes());
        }
        string(&mut out, &self.name);
        for value in [self.field_58, self.rank] {
            field(&mut out, 8, &value.to_le_bytes());
        }
        field(&mut out, 9, &self.rank_points.to_le_bytes());
        for value in [
            self.real_points,
            self.played,
            self.wins,
            self.losses,
            self.bonus_used,
            self.bonus_time,
            self.streak,
            self.career_wins,
        ] {
            field(&mut out, 7, &value.to_le_bytes());
        }
        for value in skills {
            field(&mut out, 13, &value.to_le_bytes());
        }
        Ok(out)
    }
}

/// Team record shared by 0x51/2 (decoder 0x80a820) and 0x51/6 (0x80a350).
///
/// Both use the same wire order but store fields at different native offsets.
/// The u32 lands at +0x458 for 0x51/2 and +0x10 for 0x51/6; its meaning is not
/// established for either. The first u64 is plausibly a team ID for 0x51/6,
/// which is queried by team IDs, but no direct consumer has proven that.
#[derive(Clone, Debug, PartialEq)]
pub struct TeamRecord {
    pub field_08: u64,
    /// Raw non-NUL bytes, at most 64. The encoder appends the wire terminator.
    pub name: Vec<u8>,
    /// Opaque blob of at most 1024 bytes; its grammar is not recovered.
    pub data: Vec<u8>,
    pub field_u32: u32,
}

impl TeamRecord {
    pub fn encode(&self) -> Result<Vec<u8>> {
        check_name(&self.name)?;
        if self.data.len() > MAX_TEAM_DATA_BYTES {
            return Err(ProtocolError("League team data exceeds 1024 bytes"));
        }
        let mut out = Vec::with_capacity(25 + self.name.len() + self.data.len());
        field(&mut out, 10, &self.field_08.to_le_bytes());
        string(&mut out, &self.name);
        blob(&mut out, &self.data);
        field(&mut out, 8, &self.field_u32.to_le_bytes());
        Ok(out)
    }
}

/// One 0x51/8 member entry. `id` is an untagged u64 array element; it must come
/// from the business layer's verified identity mapping.
#[derive(Clone, Debug, PartialEq)]
pub struct TeamMember {
    pub id: u64,
    /// Raw non-NUL bytes, at most 64.
    pub name: Vec<u8>,
}

/// Team-members result for 0x51/8, read by 0x80a580: team ID, then parallel ID
/// and name arrays. The client rejects 16 or more entries and unequal counts;
/// taking one slice keeps the counts equal by construction.
pub fn team_members_result(team_id: u64, members: &[TeamMember]) -> Result<Vec<u8>> {
    if members.len() > MAX_TEAM_MEMBERS {
        return Err(ProtocolError("League team exceeds 15 members"));
    }
    for member in members {
        check_name(&member.name)?;
    }
    let names: usize = members.iter().map(|m| m.name.len() + 1).sum();
    let mut out = Vec::with_capacity(29 + 8 * members.len() + names);
    field(&mut out, 10, &team_id.to_le_bytes());
    array_header(&mut out, 10, 8 * members.len(), members.len());
    for member in members {
        out.extend_from_slice(&member.id.to_le_bytes());
    }
    // The string payload length is our choice; the original reader ignores it.
    array_header(&mut out, 16, names, members.len());
    for member in members {
        out.extend_from_slice(&member.name);
        out.push(0);
    }
    Ok(out)
}

/// Historical subdivision for 0x51/0x14, read by 0x80a144. No field here has a
/// proven semantic name; the metadata blob is not assumed to follow the current
/// subdivision's `%d,%d,%d,%d` grammar.
#[derive(Clone, Debug, PartialEq)]
pub struct HistorySubdivision {
    pub field_08: u64,
    pub field_10: u64,
    pub field_18: u64,
    pub flag_20: bool,
    pub field_24: u32,
    /// At most 64 bytes.
    pub metadata: Vec<u8>,
}

impl HistorySubdivision {
    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.metadata.len() > 64 {
            return Err(ProtocolError("League metadata exceeds 64 bytes"));
        }
        let mut out = Vec::with_capacity(43 + self.metadata.len());
        for value in [self.field_08, self.field_10, self.field_18] {
            field(&mut out, 10, &value.to_le_bytes());
        }
        field(&mut out, 1, &[u8::from(self.flag_20)]);
        field(&mut out, 8, &self.field_24.to_le_bytes());
        blob(&mut out, &self.metadata);
        Ok(out)
    }
}

/// Season-history entry for 0x51/0x15, read by 0x81e24c. The first u64 is
/// plausibly a season ID; the second is consumed as a subdivision query key.
pub fn season_history_result(field_08: u64, subdivision_id: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(18);
    field(&mut out, 10, &field_08.to_le_bytes());
    field(&mut out, 10, &subdivision_id.to_le_bytes());
    out
}
