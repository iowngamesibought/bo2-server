//! Storage service 0x0a for the supported client profile startup (LEAGUE06).
//!
//! Request layouts come from the client builders (DEC08 startup, LEAGUE06):
//!   0x0a/7 publisher get  : string filename
//!   0x0a/6 publisher list : u32, u16, u16, string|null filename
//!   0x0a/3 user get       : string filename, u64 owner (wrapper passes 0)
//!   0x0a/5 user list      : u64 owner, u32, u16 max, u16, string|null
//!   0x0a/1 user upload    : string filename, bool, blob data, [u64]
//!   0x0a/2 user remove    : string filename, u64
//! Typed tags: u16=6, u32=8, u64=10, bool=1, string=16, blob=19, null=20.
//!
//! Replies (owned policy, each grounded in a traced client path):
//! - Online WAD: the owned WAD from `league_config`.
//! - PATCH (`<base>.NN`): 16 zero bytes. The success callback 0x55d5b8 reads a
//!   16-byte trailer; without its `SKID` marker it records PATCH version -1 and
//!   completes, which the WAD's required version -1 accepts. A shorter blob
//!   would make the client read before its buffer.
//! - Missing user file: error reply. The stats failure callback 0x55abe8 marks
//!   stats ready and installs defaults, which is the new-player path.
//! - Upload and both lists: zero-result success. The original parser leaves
//!   result objects untouched at count 0 (LEAGUE06 multi-result run); the
//!   upload's metadata result has no traced consumer on the stats-save path.
use crate::protocol::ProtocolError;

type Result<T> = std::result::Result<T, ProtocolError>;

/// No-file error: generic failure callback 0x551db8 specifically tests 1000
/// before invoking the per-file failure callback (including 0x55abe8).
pub const ERROR_NO_FILE: u32 = 1000;
/// Client publisher-read capacities: WAD 0x10000, PATCH up to 0x20000.
pub const MAX_FILENAME_BYTES: usize = 0x7f;
/// User blobs are bounded by the stats buffer the client allocates (0xc400
/// read capacity); larger uploads are refused rather than stored.
pub const MAX_USER_FILE_BYTES: usize = 0xc400;

#[derive(Debug, PartialEq)]
pub enum Request {
    PublisherGet { filename: String },
    PublisherList { filename: Option<String> },
    UserGet { filename: String, owner: u64 },
    UserList { owner: u64 },
    Upload { filename: String, data: Vec<u8> },
    Remove { filename: String },
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
            .ok_or(ProtocolError("storage length overflow"))?;
        let v = self
            .bytes
            .get(self.at..end)
            .ok_or(ProtocolError("truncated storage request"))?;
        self.at = end;
        Ok(v)
    }
    fn tag(&mut self, expected: u8) -> Result<()> {
        if self.take(1)?[0] != expected {
            return Err(ProtocolError("wrong storage field tag"));
        }
        Ok(())
    }
    fn u16(&mut self) -> Result<u16> {
        self.tag(6)?;
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        self.tag(8)?;
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64> {
        self.tag(10)?;
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn bool(&mut self) -> Result<bool> {
        self.tag(1)?;
        Ok(self.take(1)?[0] != 0)
    }
    fn blob(&mut self) -> Result<Vec<u8>> {
        self.tag(19)?;
        let len = self.u32()? as usize;
        if len > MAX_USER_FILE_BYTES {
            return Err(ProtocolError("storage blob exceeds owned budget"));
        }
        Ok(self.take(len)?.to_vec())
    }
    fn string(&mut self) -> Result<Option<String>> {
        match self.take(1)?[0] {
            20 => Ok(None),
            16 => {
                let rest = &self.bytes[self.at..];
                let len = rest
                    .iter()
                    .take(MAX_FILENAME_BYTES + 1)
                    .position(|&b| b == 0)
                    .ok_or(ProtocolError("unterminated or oversized filename"))?;
                let name = std::str::from_utf8(self.take(len)?)
                    .map_err(|_| ProtocolError("filename is not UTF-8"))?
                    .to_owned();
                self.take(1)?;
                if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
                    return Err(ProtocolError("invalid filename"));
                }
                Ok(Some(name))
            }
            _ => Err(ProtocolError("wrong storage string tag")),
        }
    }
    /// Optional trailing filename: the real client omits it entirely (the
    /// task terminator 0x00 follows), or may send null.
    fn optional_string(&mut self) -> Result<Option<String>> {
        if self.bytes.get(self.at) == Some(&0) {
            return Ok(None);
        }
        self.string()
    }
    fn required_string(&mut self) -> Result<String> {
        self.string()?
            .ok_or(ProtocolError("missing storage filename"))
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }
}

/// Parse a 0x0a task payload (`0x0a, 3, op, body, CBC padding`) after MAC
/// verification. Trailing bytes are accepted only as minimal CBC padding.
pub fn parse_task_payload(data: &[u8]) -> Result<Request> {
    if data.len() < 3 || data[0] != 0x0a || data[1] != 3 {
        return Err(ProtocolError("not a storage task"));
    }
    let mut r = Reader {
        bytes: &data[3..],
        at: 0,
    };
    let request = match data[2] {
        7 => Request::PublisherGet {
            filename: r.required_string()?,
        },
        6 => {
            r.u32()?;
            r.u16()?;
            r.u16()?;
            Request::PublisherList {
                filename: r.optional_string()?,
            }
        }
        3 => Request::UserGet {
            filename: r.required_string()?,
            owner: r.u64()?,
        },
        5 => {
            let owner = r.u64()?;
            r.u32()?;
            r.u16()?;
            r.u16()?;
            r.optional_string()?;
            Request::UserList { owner }
        }
        1 => {
            let filename = r.required_string()?;
            r.bool()?;
            let data = r.blob()?;
            // Optional trailing u64 (written only when nonzero by 0x82731c).
            if r.remaining() >= 9 && r.bytes[r.at] == 10 {
                r.u64()?;
            }
            Request::Upload { filename, data }
        }
        2 => {
            let filename = r.required_string()?;
            r.u64()?;
            Request::Remove { filename }
        }
        _ => return Err(ProtocolError("unsupported storage operation")),
    };
    let used = 3 + r.at;
    // Frame = 4 MAC + 1 inner type + payload, padded to 8 bytes.
    if !crate::protocol::task_tail_valid(data, used) {
        return Err(ProtocolError("non-minimal storage task padding"));
    }
    Ok(request)
}

/// PATCH file names are `%s.%02d` with a runtime base name.
pub fn is_patch_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() > 3 && b[b.len() - 3] == b'.' && b[b.len() - 2..].iter().all(u8::is_ascii_digit)
}

/// Declined PATCH: no `SKID` trailer marker, exactly the trailer length.
pub const PATCH_DECLINE: [u8; 16] = [0; 16];

/// Blob result body (decoder 0x82703c): tag19, tagged u32 length, bytes.
pub fn blob_result(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + data.len());
    out.push(19);
    out.push(8);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    out
}

/// Answer one storage task for the connection's bound identity. `wad` is the
/// owned online WAD. Unknown publisher files get the no-file error, so their
/// failure callbacks run instead of a fabricated success.
pub fn handle_task(
    store: &mut crate::league_store::Store,
    caller: u64,
    transaction: u64,
    data: &[u8],
    now: i64,
    publisher: &crate::profile::PublisherAssets,
) -> std::result::Result<Vec<u8>, crate::league_service::ServiceError> {
    use crate::protocol::{empty_success, error_reply, results_reply};
    let op = data[2];
    let reply = match parse_task_payload(data)? {
        Request::PublisherGet { filename } if publisher.is_bundle_name(&filename) => {
            results_reply(transaction, op, &[blob_result(&publisher.bundle)])?
        }
        Request::PublisherGet { filename } if is_patch_name(&filename) => {
            results_reply(transaction, op, &[blob_result(&PATCH_DECLINE)])?
        }
        Request::PublisherGet { .. } => error_reply(transaction, ERROR_NO_FILE)?,
        Request::PublisherList { .. } | Request::UserList { .. } => empty_success(transaction, op),
        Request::UserGet { filename, owner } => {
            let owner = if owner == 0 { caller } else { owner };
            match store.user_file(owner, &filename)? {
                Some(bytes) => results_reply(transaction, op, &[blob_result(&bytes)])?,
                None => error_reply(transaction, ERROR_NO_FILE)?,
            }
        }
        Request::Upload { filename, data } => {
            store.put_user_file(caller, &filename, &data, now)?;
            empty_success(transaction, op)
        }
        Request::Remove { filename } => {
            store.remove_user_file(caller, &filename)?;
            empty_success(transaction, op)
        }
    };
    Ok(reply)
}
