//! Group population counts, service 0x1c (LEAGUE06).
//!
//! setGroups 0x1c/1 (builder 0x819ee8) and getGroupCounts 0x1c/4 (0x81a0bc)
//! both send one u32 array of group IDs: marker 108, a tag8 byte, raw LE32 byte
//! length, raw LE32 count, untagged LE32 elements. Only 0x1c/4 attaches
//! results: 12-byte objects (constructor 0x81a2c0) whose deserializer 0x81a350
//! reads u32 group (+4) and u32 count (+8). The client asks for groups 0..N-1
//! and feeds the League/playlist selector's population display from them.
//!
//! Counts are truthful (owned policy): each live connection's latest setGroups
//! membership, removed when the connection closes. No players are fabricated.
use crate::protocol::{ProtocolError, empty_success, results_reply};
use std::collections::{BTreeSet, HashMap};

type Result<T> = std::result::Result<T, ProtocolError>;

/// Client result array holds 0x400 objects (initializer 0x5246a0).
pub const MAX_GROUPS: usize = 0x400;

fn parse_array(data: &[u8]) -> Result<Vec<u32>> {
    if data.len() < 3 || data[0] != 0x1c || data[1] != 3 {
        return Err(ProtocolError("not a group task"));
    }
    let body = &data[3..];
    if body.len() < 10 || body[0] != 108 {
        return Err(ProtocolError("expected u32 group array"));
    }
    let byte_len = u32::from_le_bytes(body[2..6].try_into().unwrap()) as usize;
    let count = u32::from_le_bytes(body[6..10].try_into().unwrap()) as usize;
    if count > MAX_GROUPS || byte_len != count * 4 {
        return Err(ProtocolError("invalid group array count/length"));
    }
    let end = 10 + byte_len;
    let elements = body
        .get(10..end)
        .ok_or(ProtocolError("truncated group array"))?;
    let used = 3 + end;
    if !crate::protocol::task_tail_valid(data, used) {
        return Err(ProtocolError("non-minimal group task padding"));
    }
    Ok(elements
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect())
}

/// One group-count result body.
pub fn count_result(group: u32, count: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(10);
    out.push(8);
    out.extend_from_slice(&group.to_le_bytes());
    out.push(8);
    out.extend_from_slice(&count.to_le_bytes());
    out
}

/// Live group membership keyed by lobby connection.
#[derive(Default)]
pub struct Groups {
    members: HashMap<u64, BTreeSet<u32>>,
}

impl Groups {
    pub fn drop_connection(&mut self, connection: u64) {
        self.members.remove(&connection);
    }

    pub fn count(&self, group: u32) -> u32 {
        self.members.values().filter(|g| g.contains(&group)).count() as u32
    }

    pub fn handle_task(
        &mut self,
        connection: u64,
        transaction: u64,
        data: &[u8],
    ) -> Result<Vec<u8>> {
        let op = data.get(2).copied().unwrap_or(0);
        let groups = parse_array(data)?;
        match op {
            1 => {
                self.members
                    .insert(connection, groups.into_iter().collect());
                Ok(empty_success(transaction, op))
            }
            4 => {
                let results: Vec<Vec<u8>> = groups
                    .into_iter()
                    .map(|g| count_result(g, self.count(g)))
                    .collect();
                results_reply(transaction, op, &results)
            }
            _ => Err(ProtocolError("unsupported group operation")),
        }
    }
}
