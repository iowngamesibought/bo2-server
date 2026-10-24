//! Contracts reconstructed in DEC06-08 for the supported client profile.
//! Auth/login bit fields and lobby byte tags deliberately use different codecs.
use des::TdesEde3;
use des::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::{error::Error, fmt};
use tiger::{Digest, Tiger};

pub const MAX_FRAME: usize = 65535;
pub const CANDIDATE_KEY: [u8; 24] = [0; 24];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError(pub &'static str);
impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl Error for ProtocolError {}
type Result<T> = std::result::Result<T, ProtocolError>;

pub fn frame(body: &[u8]) -> Result<Vec<u8>> {
    if body.len() > MAX_FRAME {
        return Err(ProtocolError("frame exceeds limit"));
    }
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(body);
    Ok(out)
}

struct Bits<'a> {
    bytes: &'a [u8],
    pos: usize,
    typed: bool,
}
impl<'a> Bits<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self> {
        let mut b = Self {
            bytes,
            pos: 0,
            typed: false,
        };
        b.typed = b.take(1)? != 0;
        Ok(b)
    }
    fn take(&mut self, count: usize) -> Result<u32> {
        if count > 32
            || self
                .pos
                .checked_add(count)
                .is_none_or(|n| n > self.bytes.len() * 8)
        {
            return Err(ProtocolError("truncated bit buffer"));
        }
        let mut v = 0;
        for n in 0..count {
            v |= u32::from((self.bytes[self.pos / 8] >> (self.pos % 8)) & 1) << n;
            self.pos += 1;
        }
        Ok(v)
    }
    fn u32(&mut self) -> Result<u32> {
        if self.typed && self.take(5)? != 8 {
            return Err(ProtocolError("wrong bit u32 tag"));
        }
        self.take(32)
    }
    fn bytes(&mut self, count: usize) -> Result<Vec<u8>> {
        if count > MAX_FRAME || count > (self.bytes.len() * 8 - self.pos) / 8 {
            return Err(ProtocolError("invalid blob length"));
        }
        (0..count).map(|_| self.take(8).map(|v| v as u8)).collect()
    }
    fn end(&mut self) -> Result<()> {
        let left = self.bytes.len() * 8 - self.pos;
        if left > 7 || self.take(left)? != 0 {
            return Err(ProtocolError("trailing bit data"));
        }
        Ok(())
    }
}
struct BitWriter {
    bytes: Vec<u8>,
    pos: usize,
}
impl BitWriter {
    fn new(typed: bool) -> Self {
        let mut w = Self {
            bytes: vec![],
            pos: 0,
        };
        w.put(u32::from(typed), 1);
        w
    }
    fn put(&mut self, v: u32, n: usize) {
        for i in 0..n {
            if self.pos / 8 == self.bytes.len() {
                self.bytes.push(0);
            }
            self.bytes[self.pos / 8] |= (((v >> i) & 1) as u8) << (self.pos % 8);
            self.pos += 1;
        }
    }
    fn raw(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.put(u32::from(*b), 8);
        }
    }
}

/// Only non-secret metadata escapes the parser. The ticket/proof is NOT verified.
#[derive(Debug)]
pub struct AuthRequest {
    pub title: u32,
    pub ticket_len: usize,
    pub ticket_version: u32,
    /// Online ID read from a v3.1 platform ticket body, when it parses. Unverified:
    /// the platform provider signature is not checked (see `identity`).
    pub online_id: Option<Vec<u8>>,
}
pub fn parse_auth(body: &[u8]) -> Result<AuthRequest> {
    if !body.starts_with(&[0, 18]) {
        return Err(ProtocolError("expected clear auth type18"));
    }
    let mut b = Bits::new(&body[2..])?;
    if !b.typed {
        return Err(ProtocolError("untyped auth request unsupported"));
    }
    let _seed = b.u32()?;
    let title = b.u32()?;
    let ticket_len = b.u32()? as usize;
    let ticket = b.bytes(ticket_len)?;
    if ticket.len() < 4 {
        return Err(ProtocolError("ticket too short"));
    }
    let ticket_version = u32::from_be_bytes(ticket[..4].try_into().unwrap());
    if b.u32()? != 8 {
        return Err(ProtocolError("unexpected proof length"));
    }
    b.bytes(8)?;
    b.end()?;
    Ok(AuthRequest {
        title,
        ticket_len,
        ticket_version,
        online_id: crate::identity::ticket_online_id(&ticket).ok(),
    })
}

/// Experimental malformed-record response, not a valid official auth record.
pub fn candidate_auth_reply(token: &[u8; 128]) -> Vec<u8> {
    let mut b = BitWriter::new(false);
    b.put(700, 32);
    b.put(0, 32);
    b.raw(&[0; 152]);
    b.raw(token);
    let mut body = vec![0, 19];
    body.extend(b.bytes);
    let mut out = vec![0; 4]; // observed optional zero-length record
    out.extend(frame(&body).expect("fixed bounded auth reply"));
    out
}

pub struct Login {
    pub title: u32,
    pub seed: u32,
    pub token: [u8; 128],
}
pub fn parse_login(body: &[u8]) -> Result<Login> {
    if !body.starts_with(&[0, 7]) {
        return Err(ProtocolError("expected clear lobby type7"));
    }
    let mut b = Bits::new(&body[2..])?;
    if !b.typed {
        return Err(ProtocolError("untyped lobby login unsupported"));
    }
    let title = b.u32()?;
    let seed = b.u32()?;
    let token = b.bytes(128)?.try_into().unwrap();
    b.end()?;
    Ok(Login { title, seed, token })
}
pub fn token_digest(token: &[u8; 128]) -> [u8; 24] {
    Tiger::digest(token).into()
}

fn tagged_u32(out: &mut Vec<u8>, v: u32) {
    out.push(8);
    out.extend(v.to_le_bytes());
}
fn tagged_u64(out: &mut Vec<u8>, v: u64) {
    out.push(10);
    out.extend(v.to_le_bytes());
}
pub fn ready(connection_id: u64) -> Vec<u8> {
    let mut b = vec![0, 4];
    tagged_u64(&mut b, connection_id);
    frame(&b).unwrap()
}
pub fn time_reply(transaction: u64, unix_seconds: u32) -> Vec<u8> {
    let mut p = vec![1];
    tagged_u64(&mut p, transaction);
    tagged_u32(&mut p, 0);
    p.extend([3, 6]);
    tagged_u32(&mut p, 1);
    tagged_u32(&mut p, 1);
    tagged_u32(&mut p, unix_seconds);
    p
}
pub fn empty_success(transaction: u64, operation: u8) -> Vec<u8> {
    let mut p = vec![1];
    tagged_u64(&mut p, transaction);
    tagged_u32(&mut p, 0);
    p.extend([3, operation]);
    tagged_u32(&mut p, 0);
    p
}
/// Failed task: transaction and nonzero error only (DEC08 handler 0x8239e8).
/// Error 200 means "async pending" to the client and is never emitted here.
pub fn error_reply(transaction: u64, error: u32) -> Result<Vec<u8>> {
    if error == 0 || error == 200 {
        return Err(ProtocolError("error code must be nonzero and not 200"));
    }
    let mut p = vec![1];
    tagged_u64(&mut p, transaction);
    tagged_u32(&mut p, error);
    Ok(p)
}

/// Raw measurement failure, accepted by both 0x81f6cc (initialization) and
/// 0x81e9d8 (results). Type 5 has a raw transaction, then u8 status and LE u16
/// error; it must not use the tagged type-1 task envelope.
pub fn raw_measurement_failure(transaction: u64) -> Vec<u8> {
    let mut reply = vec![5];
    reply.extend(transaction.to_le_bytes());
    // 4 is the client's own default failed-response state. This reports an
    // incomplete measurement service, not a successful bandwidth result.
    reply.extend([1, 4, 0]);
    reply
}

/// Metadata only, for an unencrypted, unpadded type-1 reply produced here.
/// This does not validate result bodies or imply that a client consumed them.
#[derive(Debug, PartialEq, Eq)]
pub struct TaskReplySummary {
    pub error: u32,
    pub results: u32,
    pub result_bytes: usize,
}

pub fn task_reply_summary(reply: &[u8]) -> Result<TaskReplySummary> {
    if reply.len() < 15 || reply[..2] != [1, 10] || reply[10] != 8 {
        return Err(ProtocolError("invalid reply diagnostic header"));
    }
    let error = u32::from_le_bytes(reply[11..15].try_into().unwrap());
    if error != 0 {
        if reply.len() != 15 {
            return Err(ProtocolError("error reply has trailing fields"));
        }
        return Ok(TaskReplySummary {
            error,
            results: 0,
            result_bytes: 0,
        });
    }
    if reply.len() < 22 || reply[15] != 3 || reply[17] != 8 {
        return Err(ProtocolError("invalid success diagnostic header"));
    }
    let results = u32::from_le_bytes(reply[18..22].try_into().unwrap());
    let header = if results == 0 {
        if reply.len() != 22 {
            return Err(ProtocolError("empty reply has trailing fields"));
        }
        22
    } else {
        if reply.len() < 27 || reply[22] != 8 || reply[23..27] != reply[18..22] {
            return Err(ProtocolError("inconsistent reply diagnostic counts"));
        }
        27
    };
    Ok(TaskReplySummary {
        error,
        results,
        result_bytes: reply.len() - header,
    })
}
/// Success envelope carrying result bodies. Zero results is `empty_success`;
/// otherwise two equal counts precede the bodies, as `time_reply` does. The
/// original receiver accepted one-result replies (DEC08, LEAGUE04/06); replies
/// with several results have not yet been exercised against it.
pub fn results_reply(transaction: u64, operation: u8, results: &[Vec<u8>]) -> Result<Vec<u8>> {
    if results.is_empty() {
        return Ok(empty_success(transaction, operation));
    }
    let count = u32::try_from(results.len()).map_err(|_| ProtocolError("too many results"))?;
    let mut p = vec![1];
    tagged_u64(&mut p, transaction);
    tagged_u32(&mut p, 0);
    p.extend([3, operation]);
    tagged_u32(&mut p, count);
    tagged_u32(&mut p, count);
    for body in results {
        p.extend_from_slice(body);
    }
    if p.len() > MAX_FRAME - 16 {
        return Err(ProtocolError("results exceed frame"));
    }
    Ok(p)
}

fn iv(seed: u32) -> [u8; 8] {
    Tiger::digest(seed.to_le_bytes())[..8].try_into().unwrap()
}
fn cbc(key: &[u8; 24], seed: u32, bytes: &mut [u8], encrypt: bool) -> Result<()> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(8) {
        return Err(ProtocolError("invalid cipher block length"));
    }
    let cipher = TdesEde3::new(key.into());
    let mut prev = iv(seed);
    for chunk in bytes.as_chunks_mut::<8>().0 {
        let mut block = des::cipher::Block::<TdesEde3>::clone_from_slice(chunk);
        if encrypt {
            for (b, p) in block.iter_mut().zip(prev) {
                *b ^= p;
            }
            cipher.encrypt_block(&mut block);
            prev.copy_from_slice(&block);
        } else {
            let next = *chunk;
            cipher.decrypt_block(&mut block);
            for (b, p) in block.iter_mut().zip(prev) {
                *b ^= p;
            }
            prev = next;
        }
        chunk.copy_from_slice(&block);
    }
    Ok(())
}

pub struct ClientMessage {
    pub seed: u32,
    pub inner_type: u8,
    pub payload_with_padding: Vec<u8>,
}
/// Minimal CBC padding after a client task's `service, 0x03, op, body` (which
/// `used` counts): the decrypted frame is 4 MAC bytes plus that data, a
/// multiple of 8. Confirmed on a real console (LEAGUE06 first RUN).
pub fn task_padding(used: usize) -> usize {
    (8 - ((4 + used) % 8)) % 8
}
/// Real client tasks end with one 0x00 byte after the body, then padding
/// (observed on a console, LEAGUE06 first RUN). `used` counts service, tag, op
/// and body. Anything else after the grammar is rejected.
pub fn task_tail_valid(data: &[u8], used: usize) -> bool {
    data.get(used) == Some(&0) && data.len() == used + 1 + task_padding(used + 1)
}
/// Verifies client-direction HMAC. Does not strip padding without a task schema.
/// `inner_type` (plaintext byte 4, outside the MAC) is the task's service number
/// for client tasks; the task data is `[inner_type] + payload_with_padding`.
pub fn decode_client(body: &[u8], key: &[u8; 24]) -> Result<ClientMessage> {
    if body.len() < 13 || body[0] != 1 || body.len() > MAX_FRAME {
        return Err(ProtocolError("expected protected client frame"));
    }
    let seed = u32::from_le_bytes(body[1..5].try_into().unwrap());
    let mut plain = body[5..].to_vec();
    cbc(key, seed, &mut plain, false)?;
    let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(key).unwrap();
    mac.update(&plain[5..]);
    let expected = mac.finalize().into_bytes();
    if plain[..4]
        .iter()
        .zip(&expected[..4])
        .fold(0, |v, (a, b)| v | (a ^ b))
        != 0
    {
        return Err(ProtocolError("client HMAC mismatch"));
    }
    Ok(ClientMessage {
        seed,
        inner_type: plain[4],
        payload_with_padding: plain[5..].to_vec(),
    })
}
/// Server direction uses fixed magic, NOT the client's HMAC prefix.
pub fn encode_server(payload_with_type: &[u8], key: &[u8; 24], seed: u32) -> Result<Vec<u8>> {
    if payload_with_type.is_empty() || payload_with_type.len() > MAX_FRAME - 16 {
        return Err(ProtocolError("invalid server payload size"));
    }
    let n = (payload_with_type.len() + 11) & !7;
    let mut plain = 0xdeadbeefu32.to_le_bytes().to_vec();
    plain.extend(payload_with_type);
    plain.resize(n, 0);
    cbc(key, seed, &mut plain, true)?;
    let mut body = vec![1];
    body.extend(seed.to_le_bytes());
    body.extend(plain);
    frame(&body)
}
/// Useful for synthetic integration tests; not used to impersonate official sessions.
pub fn encode_client_fixture(
    payload_with_type: &[u8],
    key: &[u8; 24],
    seed: u32,
) -> Result<Vec<u8>> {
    if payload_with_type.is_empty() || payload_with_type.len() > MAX_FRAME - 16 {
        return Err(ProtocolError("invalid client payload size"));
    }
    let n = (payload_with_type.len() + 11) & !7;
    let mut plain = vec![0; 4];
    plain.extend(payload_with_type);
    plain.resize(n, seed as u8);
    let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(key).unwrap();
    mac.update(&plain[5..]);
    plain[..4].copy_from_slice(&mac.finalize().into_bytes()[..4]);
    cbc(key, seed, &mut plain, true)?;
    let mut body = vec![1];
    body.extend(seed.to_le_bytes());
    body.extend(plain);
    frame(&body)
}
/// Test counterpart of `encode_server`: decrypts one server body (without its
/// length prefix), checks the fixed magic, returns payload plus zero padding.
pub fn decode_server_fixture(body: &[u8], key: &[u8; 24]) -> Result<Vec<u8>> {
    if body.len() < 13 || body[0] != 1 {
        return Err(ProtocolError("expected protected server frame"));
    }
    let seed = u32::from_le_bytes(body[1..5].try_into().unwrap());
    let mut plain = body[5..].to_vec();
    cbc(key, seed, &mut plain, false)?;
    if plain[..4] != 0xdeadbeefu32.to_le_bytes() {
        return Err(ProtocolError("server magic mismatch"));
    }
    Ok(plain[4..].to_vec())
}
