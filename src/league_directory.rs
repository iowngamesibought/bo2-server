//! Outbound directory result bodies for the supported client profile (LEAGUE04/05).
//!
//! These are single non-null objects, without task envelopes or live handlers.
//! The incoming search-result layout differs from the client's advertisement.
//! Opaque peer address/key bytes must come from a coherent host advertisement;
//! encoding them does not establish peer connectivity or a playable match.
use crate::protocol::ProtocolError;

fn field(out: &mut Vec<u8>, tag: u8, bytes: &[u8]) {
    out.push(tag);
    out.extend_from_slice(bytes);
}

fn blob(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(19);
    field(out, 8, &(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// Create-session result for service 0x15/1: an opaque directory record ID.
/// This is separate from the host's peer identifier and peer key.
pub fn create_id_result(directory_id: &[u8; 8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(14);
    blob(&mut out, directory_id);
    out
}

/// One search result for service 0x15/5, read by native function 0x1a24d8.
///
/// Names containing offsets intentionally leave unproven business meanings open.
/// Do not interpret these integers as season or division IDs. Byte arrays have
/// fixed compatible lengths; the peer consumer requires exactly 37 address bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchResult {
    pub address: [u8; 37],
    pub directory_id: [u8; 8],
    pub field_118: u32,
    pub field_110: u32,
    pub field_114: u32,
    /// Native +0x120. Its eight in-memory bytes enter the peer candidate bundle.
    pub peer_id: u64,
    pub peer_key: [u8; 16],
    /// Native +0x13c, +0x140, ... +0x158, in that order.
    pub fields_13c_through_158: [i32; 8],
    /// Party aggregate used alongside search skill weighting, not a division ID.
    pub field_15c: f32,
    /// Native +0x160, +0x164, +0x168, +0x16c, in that order.
    pub fields_160_through_16c: [i32; 4],
}

impl SearchResult {
    /// Encode one full result body. The directory ID is copied as opaque bytes;
    /// the peer identifier is a typed little-endian u64 on this wire path.
    ///
    /// A zero peer ID would be discarded by the known candidate consumer.
    /// Rejecting nonfinite aggregates is our input policy, not native validation.
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        if self.peer_id == 0 {
            return Err(ProtocolError("zero directory-result peer identifier"));
        }
        if !self.field_15c.is_finite() {
            return Err(ProtocolError("non-finite directory-result aggregate"));
        }
        let mut out = Vec::with_capacity(168);
        blob(&mut out, &self.address);
        blob(&mut out, &self.directory_id);
        for value in [self.field_118, self.field_110, self.field_114] {
            field(&mut out, 8, &value.to_le_bytes());
        }
        field(&mut out, 10, &self.peer_id.to_le_bytes());
        blob(&mut out, &self.peer_key);
        for value in self.fields_13c_through_158 {
            field(&mut out, 7, &value.to_le_bytes());
        }
        field(&mut out, 13, &self.field_15c.to_le_bytes());
        for value in self.fields_160_through_16c {
            field(&mut out, 7, &value.to_le_bytes());
        }
        Ok(out)
    }
}
