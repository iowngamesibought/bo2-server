//! League task dispatch (services 0x51/0x52) onto the owned store (LEAGUE06).
//!
//! Input is an authenticated inner-type2 task payload; output is the clear reply
//! envelope for `protocol::encode_server`. Any error means the connection should
//! fail closed: no invented error codes or success stubs are sent for requests
//! whose semantics are unknown. Membership checks use the connection's bound
//! name-derived identity (`identity`), which is consistent but not proof of platform
//! account ownership.
use crate::{
    league::{self, TeamMember, TeamRecord},
    league_requests::{Request, Value, parse_task_payload},
    league_store::{Store, StoreError},
    protocol::{self, ProtocolError},
};
use std::fmt;

/// The identity bound to a lobby connection at login.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caller {
    pub player_id: u64,
}

#[derive(Debug)]
pub enum ServiceError {
    Protocol(ProtocolError),
    Store(StoreError),
    Forbidden(&'static str),
    Unsupported(&'static str),
}
impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(e) => write!(f, "protocol: {e}"),
            Self::Store(e) => write!(f, "store: {e}"),
            Self::Forbidden(m) => write!(f, "forbidden: {m}"),
            Self::Unsupported(m) => write!(f, "unsupported: {m}"),
        }
    }
}
impl std::error::Error for ServiceError {}
impl From<ProtocolError> for ServiceError {
    fn from(e: ProtocolError) -> Self {
        Self::Protocol(e)
    }
}
impl From<StoreError> for ServiceError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}
type Result<T> = std::result::Result<T, ServiceError>;

fn present<T>(value: Value<T>) -> Result<T> {
    match value {
        Value::Present(v) => Ok(v),
        Value::Null => Err(ServiceError::Unsupported(
            "null field has no assigned policy",
        )),
    }
}

fn require_member(store: &Store, team_id: u64, caller: &Caller) -> Result<()> {
    if store.is_member(team_id, caller.player_id)? {
        Ok(())
    } else {
        Err(ServiceError::Forbidden("caller is not a team member"))
    }
}

/// Unknown IDs in read requests yield no result rather than closing the session.
fn omit_missing<T>(value: std::result::Result<T, StoreError>) -> Result<Option<T>> {
    match value {
        Ok(v) => Ok(Some(v)),
        Err(StoreError::NotFound(_)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Handle one task: `data` is `service, tag3, operation, body, CBC padding` after
/// MAC verification. Returns the reply envelope with `transaction`.
pub fn handle_task(
    store: &mut Store,
    caller: &Caller,
    transaction: u64,
    data: &[u8],
    now: i64,
) -> Result<Vec<u8>> {
    let request = parse_task_payload(data)?;
    let results = dispatch(store, caller, &request, data, now)?;
    Ok(protocol::results_reply(transaction, data[2], &results)?)
}

fn dispatch(
    store: &mut Store,
    caller: &Caller,
    request: &Request,
    raw: &[u8],
    now: i64,
) -> Result<Vec<Vec<u8>>> {
    match request {
        Request::TeamId { member_ids } => {
            // A party member also looks up other members' teams (console run
            // 2026-10-24: the member asked for the leader's solo team and
            // retried on refusal). Owned policy: a caller may get or create a
            // team for any member set whose players have all signed in to this
            // server; IDs are stable per member set, so this reveals nothing
            // the members could not request themselves.
            if !member_ids.contains(&caller.player_id) {
                for &id in member_ids.iter() {
                    if store.player_name(id)?.is_empty() {
                        return Err(ServiceError::Forbidden(
                            "team member unknown to this server",
                        ));
                    }
                }
            }
            let team = store.team_for_members(member_ids)?;
            Ok(vec![league::team_id_result(team)])
        }
        Request::Subdivisions {
            team_id,
            league_ids,
        } => {
            let team = present(*team_id)?;
            require_member(store, team, caller)?;
            let mut results = vec![];
            for &league_id in league_ids {
                // TODO(league-preseason): with no active season nothing is
                // returned; the client's handling of an empty reply is unverified.
                if let Some(assignment) = store.current_assignment(team, league_id, now)? {
                    results.push(assignment.encode()?);
                }
            }
            Ok(results)
        }
        Request::Rename { team_id, name } => {
            let team = present(*team_id)?;
            require_member(store, team, caller)?;
            let name = match name {
                Value::Present(name) => name,
                Value::Null => return Err(ServiceError::Unsupported("null team name")),
            };
            store.rename_team(team, name)?;
            Ok(vec![])
        }
        Request::TeamInfo { team_ids } => {
            let mut results = vec![];
            for &team in team_ids {
                let Some(members) = omit_missing(store.team_members(team))? else {
                    continue;
                };
                // The u32 lands at native +0x10, matching the slot's memberCount
                // relative to teamID and teamName; that pairing is an inference.
                let record = TeamRecord {
                    field_08: team,
                    name: store.team_name(team)?,
                    data: vec![],
                    field_u32: members.len() as u32,
                };
                results.push(record.encode()?);
            }
            Ok(results)
        }
        Request::TeamMembers { team_ids } => {
            let mut results = vec![];
            for &team in team_ids {
                let Some(ids) = omit_missing(store.team_members(team))? else {
                    continue;
                };
                let members = ids
                    .into_iter()
                    .map(|id| {
                        Ok(TeamMember {
                            id,
                            name: store.player_name(id)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                results.push(league::team_members_result(team, &members)?);
            }
            Ok(results)
        }
        Request::SeasonHistory {
            team_id,
            league_id,
            season_ids,
        } => {
            let team = present(*team_id)?;
            let Ok(league_id) = i32::try_from(present(*league_id)?) else {
                return Ok(vec![]);
            };
            let mut results = vec![];
            for &season in season_ids {
                let Ok(season_id) = i32::try_from(season) else {
                    continue;
                };
                if let Some(sub) = store.season_subdivision(team, league_id, season_id)? {
                    results.push(league::season_history_result(season, sub));
                }
            }
            Ok(results)
        }
        Request::UploadStats(upload) => {
            // Always acknowledged: an error makes the client retry the snapshot.
            store.record_upload(caller.player_id, upload, raw, now)?;
            Ok(vec![])
        }
        Request::Stats {
            team_ids,
            subdivision_id,
        } => {
            let sub = present(*subdivision_id)?;
            let rows = omit_missing(store.stats_in_subdivision(team_ids, sub))?;
            rows.unwrap_or_default()
                .iter()
                .map(|row| Ok(row.encode()?))
                .collect()
        }
        Request::Ladder {
            subdivision_id,
            field_01,
            limit,
        } => {
            if present(*field_01)? != 0 {
                return Err(ServiceError::Unsupported("nonzero ladder argument"));
            }
            let rows = omit_missing(store.ladder(present(*subdivision_id)?, present(*limit)?))?;
            rows.unwrap_or_default()
                .iter()
                .map(|row| Ok(row.encode()?))
                .collect()
        }
        Request::RankedStats {
            team_id,
            subdivision_id,
            limit,
        } => {
            let limit = present(*limit)? as usize;
            let rows = omit_missing(
                store.stats_in_subdivision(&[present(*team_id)?], present(*subdivision_id)?),
            )?;
            rows.unwrap_or_default()
                .iter()
                .take(limit)
                .map(|row| Ok(row.encode()?))
                .collect()
        }
        // 0x51/2 observed on a real console as (own ID, 1, 0, 11): this player's
        // teams, offset 0, at most 11 (client result capacity 12). The u8 is
        // not interpreted. The trailing result u32 carries the member count, the
        // same inference as 0x51/6.
        Request::TeamsList {
            field_00,
            field_02,
            field_03,
            ..
        } => {
            let player = present(*field_00)?;
            let offset = present(*field_02)? as usize;
            let limit = (present(*field_03)? as usize).min(12);
            let mut results = vec![];
            for team in store.teams_of(player)?.into_iter().skip(offset).take(limit) {
                let members = store.team_members(team)?;
                let record = TeamRecord {
                    field_08: team,
                    name: store.team_name(team)?,
                    data: vec![],
                    field_u32: members.len() as u32,
                };
                results.push(record.encode()?);
            }
            Ok(results)
        }
        Request::HistorySubdivisions { .. } => {
            Err(ServiceError::Unsupported("0x51/0x14 result fields unknown"))
        }
    }
}
