//! Bounded League request bodies for the supported client profile.
//!
//! `parse_request` accepts an exact body and service/op separately. The optional
//! `parse_task_payload` adapter handles authenticated task data with CBC padding
//! by parsing the grammar first; it never trims zeros or guesses field lengths.
//! No dispatch, identity validation, enrollment, ranking or mutation is performed.
use crate::protocol::ProtocolError;

type Result<T> = std::result::Result<T, ProtocolError>;

/// Owned resource budgets, not recovered client array/ladder maxima.
pub const MAX_BODY_BYTES: usize = 65_535;
pub const MAX_ARRAY_ITEMS: usize = 1_024;
pub const MAX_REQUESTED_ROWS: u32 = 4_096;
/// Original rename writer's 0x40 bound includes its generated NUL.
pub const MAX_RENAME_BYTES: usize = 63;

/// Null tag0x14 is distinct from zero and from a present scalar's bit pattern.
/// In particular, `Null` IDs must not be treated as authenticated identities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value<T> {
    Present(T),
    Null,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StatsUpload {
    pub team_id: Value<u64>,
    pub subdivision_id: Value<u64>,
    /// Native object+0x18; no update-mode meaning has been established.
    pub field_18: u8,
    /// Upload tag10/u64 differs from the response's signed64/tag9.
    pub rank_points_bits: Value<u64>,
    /// realPoints, played, wins, losses, bonusUsed, bonusTime, streak, careerWins.
    pub counters: [Value<i32>; 8],
    pub skill: Value<f32>,
    pub variance: Value<f32>,
    pub placement_skill: Value<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// 0x51/1. Preserve caller order; canonical team membership is not established.
    TeamId {
        member_ids: Vec<u64>,
    },
    /// 0x51/2. Names are wire positions, not inferred pagination semantics.
    TeamsList {
        field_00: Value<u64>,
        field_01: u8,
        field_02: Value<u32>,
        field_03: Value<u32>,
    },
    Subdivisions {
        team_id: Value<u64>,
        league_ids: Vec<u64>,
    },
    /// 0x51/4. Bytes are not assumed to be UTF-8. Null name has no assigned policy.
    Rename {
        team_id: Value<u64>,
        name: Value<Vec<u8>>,
    },
    TeamInfo {
        team_ids: Vec<u64>,
    },
    TeamMembers {
        team_ids: Vec<u64>,
    },
    HistorySubdivisions {
        subdivision_ids: Vec<u64>,
    },
    SeasonHistory {
        team_id: Value<u64>,
        league_id: Value<u64>,
        season_ids: Vec<u64>,
    },
    UploadStats(StatsUpload),
    Stats {
        team_ids: Vec<u64>,
        subdivision_id: Value<u64>,
    },
    Ladder {
        subdivision_id: Value<u64>,
        field_01: Value<u32>,
        limit: Value<u32>,
    },
    RankedStats {
        team_id: Value<u64>,
        subdivision_id: Value<u64>,
        limit: Value<u32>,
    },
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(count)
            .ok_or(ProtocolError("League length overflow"))?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(ProtocolError("truncated League request"))?;
        self.at = end;
        Ok(value)
    }

    fn tag(&mut self, expected: u8) -> Result<()> {
        if self.take(1)?[0] != expected {
            return Err(ProtocolError("wrong League field tag"));
        }
        Ok(())
    }

    fn scalar<T, const N: usize>(&mut self, tag: u8, decode: fn([u8; N]) -> T) -> Result<Value<T>> {
        match self.take(1)?[0] {
            20 => Ok(Value::Null),
            actual if actual == tag => {
                Ok(Value::Present(decode(self.take(N)?.try_into().unwrap())))
            }
            _ => Err(ProtocolError("wrong League scalar tag")),
        }
    }

    fn u64(&mut self) -> Result<Value<u64>> {
        self.scalar(10, u64::from_le_bytes)
    }
    fn u32(&mut self) -> Result<Value<u32>> {
        self.scalar(8, u32::from_le_bytes)
    }
    fn i32(&mut self) -> Result<Value<i32>> {
        self.scalar(7, i32::from_le_bytes)
    }
    fn f32(&mut self) -> Result<Value<f32>> {
        let value = self.scalar(13, f32::from_le_bytes)?;
        if matches!(value, Value::Present(v) if !v.is_finite()) {
            return Err(ProtocolError("non-finite League skill"));
        }
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8> {
        self.tag(3)?;
        Ok(self.take(1)?[0])
    }

    fn ids(&mut self) -> Result<Vec<u64>> {
        self.tag(110)?;
        self.tag(8)?;
        let byte_len = u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as usize;
        let count = u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as usize;
        if count > MAX_ARRAY_ITEMS || count.checked_mul(8) != Some(byte_len) {
            return Err(ProtocolError("invalid League array count/length"));
        }
        let payload = self.take(byte_len)?;
        let mut values = Vec::with_capacity(count);
        for chunk in payload.as_chunks::<8>().0 {
            let value = u64::from_le_bytes(*chunk);
            // 0x7e71a0 cannot emit the all-ones/null sentinel with tagging off:
            // it returns failure without writing an element. Do not invent an
            // in-array null representation or accept it as a valid identity.
            if value == u64::MAX {
                return Err(ProtocolError("null sentinel in untagged League array"));
            }
            values.push(value);
        }
        Ok(values)
    }

    fn name(&mut self) -> Result<Value<Vec<u8>>> {
        match self.take(1)?[0] {
            20 => Ok(Value::Null),
            16 => {
                let tail = &self.bytes[self.at..];
                let len = tail
                    .iter()
                    .take(MAX_RENAME_BYTES + 1)
                    .position(|&v| v == 0)
                    .ok_or(ProtocolError("unterminated or oversized League name"))?;
                let name = self.take(len)?.to_vec();
                self.take(1)?;
                Ok(Value::Present(name))
            }
            _ => Err(ProtocolError("wrong League string tag")),
        }
    }

    fn limit(&mut self) -> Result<Value<u32>> {
        let value = self.u32()?;
        if matches!(value, Value::Present(n) if n > MAX_REQUESTED_ROWS) {
            return Err(ProtocolError("League row request exceeds owned budget"));
        }
        Ok(value)
    }
}

/// Parse an exact, unpadded body. Unknown operations and any trailing bytes fail.
/// Tagged present sentinel bit patterns stay present; only tag0x14 means Null.
/// Non-finite floats and resource overages are owned input-policy rejections.
pub fn parse_request(service: u8, operation: u8, body: &[u8]) -> Result<Request> {
    if body.len() > MAX_BODY_BYTES {
        return Err(ProtocolError("League request exceeds owned byte budget"));
    }
    let (request, used) = parse_prefix(service, operation, body)?;
    if used != body.len() {
        return Err(ProtocolError("trailing League request bytes"));
    }
    Ok(request)
}

/// Parse `decode_client(...).payload_with_padding` only after the caller has
/// successfully verified client-direction authentication and checked inner_type2.
/// This function performs neither authentication nor the inner-type check itself.
///
/// Header: raw service, tag3, operation. A complete decrypted frame is
/// 4 MAC bytes + 1 inner-type byte + 3 header bytes + body + minimal CBC padding.
/// Consequently padding is (-body_used) mod8 bytes. Its authenticated bytes may
/// be arbitrary, including nonzero seed bytes. No extra full block is accepted.
/// The grammar determines `body_used`; legitimate zeros in fields are retained.
pub fn parse_task_payload(data: &[u8]) -> Result<Request> {
    if data.len() < 3 || data.len() > MAX_BODY_BYTES + 3 + 7 {
        return Err(ProtocolError("invalid League task payload length"));
    }
    if data[1] != 3 {
        return Err(ProtocolError("wrong League operation tag"));
    }
    let (request, used) = parse_prefix(data[0], data[2], &data[3..])?;
    if !crate::protocol::task_tail_valid(data, 3 + used) {
        return Err(ProtocolError("non-minimal League task padding"));
    }
    Ok(request)
}

fn parse_prefix(service: u8, operation: u8, body: &[u8]) -> Result<(Request, usize)> {
    if body.len() > MAX_BODY_BYTES + 7 {
        return Err(ProtocolError("League request exceeds owned byte budget"));
    }
    let mut r = Reader { bytes: body, at: 0 };
    let request = match (service, operation) {
        (0x51, 1) => Request::TeamId {
            member_ids: r.ids()?,
        },
        (0x51, 2) => Request::TeamsList {
            field_00: r.u64()?,
            field_01: r.u8()?,
            field_02: r.u32()?,
            field_03: r.u32()?,
        },
        (0x51, 3) => Request::Subdivisions {
            team_id: r.u64()?,
            league_ids: r.ids()?,
        },
        (0x51, 4) => Request::Rename {
            team_id: r.u64()?,
            name: r.name()?,
        },
        (0x51, 6) => Request::TeamInfo { team_ids: r.ids()? },
        (0x51, 8) => Request::TeamMembers { team_ids: r.ids()? },
        (0x51, 0x14) => Request::HistorySubdivisions {
            subdivision_ids: r.ids()?,
        },
        (0x51, 0x15) => Request::SeasonHistory {
            team_id: r.u64()?,
            league_id: r.u64()?,
            season_ids: r.ids()?,
        },
        (0x52, 1) => {
            let team_id = r.u64()?;
            let subdivision_id = r.u64()?;
            let field_18 = r.u8()?;
            let rank_points_bits = r.u64()?;
            let mut counters = [Value::Null; 8];
            for value in &mut counters {
                *value = r.i32()?;
            }
            Request::UploadStats(StatsUpload {
                team_id,
                subdivision_id,
                field_18,
                rank_points_bits,
                counters,
                skill: r.f32()?,
                variance: r.f32()?,
                placement_skill: r.f32()?,
            })
        }
        (0x52, 2) => Request::Stats {
            team_ids: r.ids()?,
            subdivision_id: r.u64()?,
        },
        (0x52, 3) => Request::Ladder {
            subdivision_id: r.u64()?,
            field_01: r.u32()?,
            limit: r.limit()?,
        },
        (0x52, 4) => Request::RankedStats {
            team_id: r.u64()?,
            subdivision_id: r.u64()?,
            limit: r.limit()?,
        },
        _ => return Err(ProtocolError("unsupported League request")),
    };
    if r.at > MAX_BODY_BYTES {
        return Err(ProtocolError("League request exceeds owned byte budget"));
    }
    Ok((request, r.at))
}
