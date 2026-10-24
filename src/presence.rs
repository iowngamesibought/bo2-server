//! Presence 0x44 (LEAGUE06, first console run). Counters 0x17 are in
//! `archive`.
//!
//! 0x44/1 (builder 0x825d38): u64, blob; no result. 0x44/2 (0x825e90): tagged
//! u64 IDs; results (deserializer 0x825c94): bool online (+4), blob (+5).
//!
//! Owned policy: presence blobs live in memory per connection and disappear on
//! disconnect; a player is online while a connection holds their presence.
use crate::protocol::{ProtocolError, empty_success, results_reply, task_tail_valid};
use std::collections::HashMap;

type Result<T> = std::result::Result<T, ProtocolError>;

const MAX_PRESENCE_BYTES: usize = 0x400;
const MAX_IDS: usize = 1024;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(ProtocolError("presence length"))?;
        let v = self
            .bytes
            .get(self.at..end)
            .ok_or(ProtocolError("truncated presence/counter request"))?;
        self.at = end;
        Ok(v)
    }
    fn tagged(&mut self, tag: u8, n: usize) -> Result<&'a [u8]> {
        if self.take(1)?[0] != tag {
            return Err(ProtocolError("wrong presence/counter tag"));
        }
        self.take(n)
    }
    /// Tagged scalars until the 0x00 terminator.
    fn list(&mut self, tag: u8, n: usize) -> Result<Vec<&'a [u8]>> {
        let mut out = vec![];
        while self.bytes.get(self.at) == Some(&tag) {
            out.push(self.tagged(tag, n)?);
            if out.len() > MAX_IDS {
                return Err(ProtocolError("too many IDs"));
            }
        }
        Ok(out)
    }
}

pub fn presence_result(online: bool, blob: &[u8]) -> Vec<u8> {
    let mut out = vec![1, u8::from(online), 19, 8];
    out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
    out.extend_from_slice(blob);
    out
}

#[derive(Default)]
pub struct Presence {
    /// player -> (connection, presence blob)
    entries: HashMap<u64, (u64, Vec<u8>)>,
}

impl Presence {
    pub fn drop_connection(&mut self, connection: u64) {
        self.entries.retain(|_, (c, _)| *c != connection);
    }

    pub fn handle_task(
        &mut self,
        caller: u64,
        connection: u64,
        transaction: u64,
        data: &[u8],
    ) -> Result<Vec<u8>> {
        if data.len() < 3 || data[1] != 3 {
            return Err(ProtocolError("invalid task header"));
        }
        let (service, op) = (data[0], data[2]);
        let mut r = Reader { bytes: data, at: 3 };
        let reply = match (service, op) {
            (0x44, 1) => {
                r.tagged(10, 8)?;
                if r.take(1)?[0] != 19 {
                    return Err(ProtocolError("wrong presence blob tag"));
                }
                let len = u32::from_le_bytes(r.tagged(8, 4)?.try_into().unwrap()) as usize;
                if len > MAX_PRESENCE_BYTES {
                    return Err(ProtocolError("presence blob too long"));
                }
                let blob = r.take(len)?.to_vec();
                self.entries.insert(caller, (connection, blob));
                empty_success(transaction, op)
            }
            (0x44, 2) => {
                let results: Vec<Vec<u8>> = r
                    .list(10, 8)?
                    .into_iter()
                    .map(|id| {
                        let id = u64::from_le_bytes(id.try_into().unwrap());
                        match self.entries.get(&id) {
                            Some((_, blob)) => presence_result(true, blob),
                            None => presence_result(false, &[]),
                        }
                    })
                    .collect();
                results_reply(transaction, op, &results)?
            }
            _ => return Err(ProtocolError("unsupported presence operation")),
        };
        if !task_tail_valid(data, r.at) {
            return Err(ProtocolError("non-minimal presence/counter padding"));
        }
        Ok(reply)
    }
}
