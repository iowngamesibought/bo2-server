//! Key archive 0x0f (per-player key/value store) and counters 0x17 (global
//! totals), owned and persistent.
//!
//! Request layouts come from the client builders; all fields are typed
//! (bool=1, u8=3, u16=6, u32=8, i64=9, u64=10):
//!   0x0f/1 write : u64 owner, u16 category, then { u16 key, i64 value,
//!                  u8 write type } per key (0x81bbdc; a key whose value is 0
//!                  with type 1 is skipped by the client)
//!   0x0f/2 read  : u64 owner, u16 category, bool, u16 key x N (0x81bf20)
//!   0x17/1 add   : { u32 counter, i64 delta } per counter (0x818654 with
//!                  serializer 0x818a34)
//!   0x17/2 read  : u32 counter x N (0x818820)
//! Results: key archive { u16 key, i64 value } (0x81c1b0); counter
//! { u32 counter, i64 total } (0x818a80).
//!
//! Owned policy, not a recovered official algorithm:
//! - A connection writes only its own archive; anyone may read any archive.
//! - Key archive write types: 0 replace, 1 add (the client's default, see
//!   constructor 0x81c160), 2 keep the higher value, 3 keep the lower; other
//!   values replace. TODO(archive-writetype): confirm during console testing
//!   with the full task logs.
//! - Reads return every requested key in request order, with 0 for keys never
//!   written. A read with no keys returns all stored keys of the category.
//! - Counters are global sums; unknown counters read as 0.
use crate::league_store::Store;
use crate::protocol::{ProtocolError, empty_success, results_reply, task_tail_valid};

type Result<T> = std::result::Result<T, crate::league_service::ServiceError>;

const MAX_KEYS: usize = 512;
const MAX_COUNTERS: usize = 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct KeyWrite {
    pub key: u16,
    pub value: i64,
    pub write_type: u8,
}

#[derive(Debug, PartialEq)]
pub enum Request {
    ArchiveWrite {
        owner: u64,
        category: u16,
        keys: Vec<KeyWrite>,
    },
    ArchiveRead {
        owner: u64,
        category: u16,
        flag: bool,
        keys: Vec<u16>,
    },
    CounterAdd(Vec<(u32, i64)>),
    CounterRead(Vec<u32>),
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
            .ok_or(ProtocolError("archive length overflow"))?;
        let v = self
            .bytes
            .get(self.at..end)
            .ok_or(ProtocolError("truncated archive request"))?;
        self.at = end;
        Ok(v)
    }
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }
    fn tagged<const N: usize>(&mut self, tag: u8) -> std::result::Result<[u8; N], ProtocolError> {
        if self.take(1)?[0] != tag {
            return Err(ProtocolError("wrong archive field tag"));
        }
        Ok(self.take(N)?.try_into().unwrap())
    }
    fn bool(&mut self) -> std::result::Result<bool, ProtocolError> {
        Ok(self.tagged::<1>(1)?[0] != 0)
    }
    fn u8(&mut self) -> std::result::Result<u8, ProtocolError> {
        Ok(self.tagged::<1>(3)?[0])
    }
    fn u16(&mut self) -> std::result::Result<u16, ProtocolError> {
        Ok(u16::from_le_bytes(self.tagged(6)?))
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
}

/// Parse a 0x0f or 0x17 task after MAC checks.
pub fn parse_task_payload(data: &[u8]) -> std::result::Result<Request, ProtocolError> {
    if data.len() < 3 || data[1] != 3 {
        return Err(ProtocolError("invalid archive task header"));
    }
    let mut r = Reader { bytes: data, at: 3 };
    let request = match (data[0], data[2]) {
        (0x0f, 1) => {
            let owner = r.u64()?;
            let category = r.u16()?;
            let mut keys = vec![];
            while r.peek() == Some(6) {
                keys.push(KeyWrite {
                    key: r.u16()?,
                    value: r.i64()?,
                    write_type: r.u8()?,
                });
                if keys.len() > MAX_KEYS {
                    return Err(ProtocolError("too many archive keys"));
                }
            }
            Request::ArchiveWrite {
                owner,
                category,
                keys,
            }
        }
        (0x0f, 2) => {
            let owner = r.u64()?;
            let category = r.u16()?;
            let flag = r.bool()?;
            let mut keys = vec![];
            while r.peek() == Some(6) {
                keys.push(r.u16()?);
                if keys.len() > MAX_KEYS {
                    return Err(ProtocolError("too many archive keys"));
                }
            }
            Request::ArchiveRead {
                owner,
                category,
                flag,
                keys,
            }
        }
        (0x17, 1) => {
            let mut counters = vec![];
            while r.peek() == Some(8) {
                counters.push((r.u32()?, r.i64()?));
                if counters.len() > MAX_COUNTERS {
                    return Err(ProtocolError("too many counters"));
                }
            }
            Request::CounterAdd(counters)
        }
        (0x17, 2) => {
            let mut counters = vec![];
            while r.peek() == Some(8) {
                counters.push(r.u32()?);
                if counters.len() > MAX_COUNTERS {
                    return Err(ProtocolError("too many counters"));
                }
            }
            Request::CounterRead(counters)
        }
        _ => return Err(ProtocolError("unsupported archive/counter operation")),
    };
    if !task_tail_valid(data, r.at) {
        return Err(ProtocolError("non-minimal archive padding"));
    }
    Ok(request)
}

/// Key archive result for deserializer 0x81c1b0.
pub fn key_result(key: u16, value: i64) -> Vec<u8> {
    let mut out = vec![6];
    out.extend_from_slice(&key.to_le_bytes());
    out.push(9);
    out.extend_from_slice(&value.to_le_bytes());
    out
}

/// Counter result for deserializer 0x818a80.
pub fn counter_result(counter: u32, total: i64) -> Vec<u8> {
    let mut out = vec![8];
    out.extend_from_slice(&counter.to_le_bytes());
    out.push(9);
    out.extend_from_slice(&total.to_le_bytes());
    out
}

/// Answer one 0x0f or 0x17 task for the connection's bound identity.
pub fn handle_task(
    store: &mut Store,
    caller: u64,
    transaction: u64,
    data: &[u8],
    now: i64,
) -> Result<Vec<u8>> {
    let op = data[2];
    match parse_task_payload(data)? {
        Request::ArchiveWrite {
            owner,
            category,
            keys,
        } => {
            if owner != caller {
                return Err(crate::league_service::ServiceError::Forbidden(
                    "key archive write for another identity",
                ));
            }
            store.write_archive(owner, category, &keys, now)?;
            Ok(empty_success(transaction, op))
        }
        Request::ArchiveRead {
            owner,
            category,
            keys,
            ..
        } => {
            let values = if keys.is_empty() {
                store.archive_category(owner, category)?
            } else {
                keys.iter()
                    .map(|&key| Ok((key, store.archive_value(owner, category, key)?.unwrap_or(0))))
                    .collect::<Result<Vec<_>>>()?
            };
            let results: Vec<Vec<u8>> = values.into_iter().map(|(k, v)| key_result(k, v)).collect();
            Ok(results_reply(transaction, op, &results)?)
        }
        Request::CounterAdd(counters) => {
            store.add_counters(&counters, now)?;
            Ok(empty_success(transaction, op))
        }
        Request::CounterRead(counters) => {
            let results = counters
                .iter()
                .map(|&id| Ok(counter_result(id, store.counter(id)?)))
                .collect::<Result<Vec<_>>>()?;
            Ok(results_reply(transaction, op, &results)?)
        }
    }
}
