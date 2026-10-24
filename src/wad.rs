//! Online WAD container (`configured publisher bundle`) as parsed by 0x55cc24.
//!
//! Big-endian header: magic 0x543377AB, timestamp, entry count, required PATCH
//! version (stored to 0xd867e4). Entries start at +0x10, 0x2c bytes each: a
//! NUL-padded 32-byte name, compressed size (+0x20), size (+0x24) and data offset
//! (+0x28). Each member is one zlib stream inflated into a zeroed 0x40000-byte
//! buffer and dispatched by name. The download buffer holds 0x10000 bytes.
//!
//! Members use zlib *stored* blocks: valid inflate input with no compressor
//! dependency. The parser only reads +0x20 and +0x28; +0x24 is filled with the
//! uncompressed size as the conventional value.
use crate::protocol::ProtocolError;

type Result<T> = std::result::Result<T, ProtocolError>;

pub const MAGIC: u32 = 0x5433_77ab;
/// Publisher-read capacity the client supplies for the WAD (0x55c6d8).
pub const MAX_WAD_BYTES: usize = 0x10000;
/// Inflate buffer; one byte is kept free so text members stay NUL-terminated.
pub const MAX_MEMBER_BYTES: usize = 0x40000 - 1;
const ENTRY_BYTES: usize = 0x2c;
const HEADER_BYTES: usize = 0x10;

pub struct Member<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= 65_521;
        b %= 65_521;
    }
    (b << 16) | a
}

/// zlib stream (RFC 1950) made of stored deflate blocks (RFC 1951 BTYPE=00).
pub fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut blocks = data.chunks(0xffff).peekable();
    if blocks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(block) = blocks.next() {
        out.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Build a WAD. `patch_version` is the minimum PATCH version the asset gate
/// requires; -1 matches a declined PATCH (see `publisher`).
pub fn build(timestamp: u32, patch_version: i32, members: &[Member]) -> Result<Vec<u8>> {
    let mut entries = Vec::with_capacity(members.len() * ENTRY_BYTES);
    let mut data = Vec::new();
    let mut offset = HEADER_BYTES + members.len() * ENTRY_BYTES;
    for member in members {
        let name = member.name.as_bytes();
        if name.is_empty() || name.len() > 31 || name.contains(&0) {
            return Err(ProtocolError("invalid WAD member name"));
        }
        if member.data.len() > MAX_MEMBER_BYTES {
            return Err(ProtocolError("WAD member exceeds inflate buffer"));
        }
        let stream = zlib_stored(member.data);
        let mut entry = [0u8; ENTRY_BYTES];
        entry[..name.len()].copy_from_slice(name);
        entry[0x20..0x24].copy_from_slice(&(stream.len() as u32).to_be_bytes());
        entry[0x24..0x28].copy_from_slice(&(member.data.len() as u32).to_be_bytes());
        entry[0x28..0x2c].copy_from_slice(&(offset as u32).to_be_bytes());
        entries.extend_from_slice(&entry);
        offset += stream.len();
        data.extend_from_slice(&stream);
    }
    let mut out = Vec::with_capacity(offset);
    for word in [MAGIC, timestamp, members.len() as u32, patch_version as u32] {
        out.extend_from_slice(&word.to_be_bytes());
    }
    out.extend_from_slice(&entries);
    out.extend_from_slice(&data);
    if out.len() > MAX_WAD_BYTES {
        return Err(ProtocolError("WAD exceeds client download buffer"));
    }
    Ok(out)
}
