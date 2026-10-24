//! LAN diagnostic server: no official endpoint forwarding and no success stubs.
//! Optional League mode dispatches 0x51/0x52 tasks to the owned store.
use crate::archive;
use crate::directory::Directory;
use crate::groups::Groups;
use crate::identity;
use crate::league_service::{self, Caller};
use crate::league_store::Store;
use crate::presence::Presence;
use crate::protocol::{self, CANDIDATE_KEY, MAX_FRAME};
use crate::startup;
use crate::stats;
use crate::storage;
use rand::{RngCore, rngs::OsRng};
use std::{
    collections::HashMap,
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct Config {
    pub serve_time: bool,
    pub frame_timeout: Duration,
    pub token_ttl: Duration,
    pub max_connections: usize,
    /// Wait allowed between lobby frames, including transport keep-alives.
    pub idle_timeout: Duration,
    /// Total lifetime and task budget of one lobby connection.
    pub session_limit: Duration,
    pub max_tasks: u32,
    /// League mode: requires a ticket online ID at auth and serves 0x51/0x52
    /// plus storage 0x0a.
    pub league: Option<Arc<Mutex<Store>>>,
    /// Online WAD served for `configured client bundle` in League mode.
    pub publisher: Arc<crate::profile::PublisherAssets>,
    /// In-memory session directory (service 0x15) shared by League sessions.
    pub directory: Arc<Mutex<Directory>>,
    /// Live group membership for population counts (service 0x1c).
    pub groups: Arc<Mutex<Groups>>,
    /// Live presence blobs (service 0x44).
    pub presence: Arc<Mutex<Presence>>,
}
impl Config {
    pub fn diagnostic() -> Self {
        Self {
            serve_time: false,
            frame_timeout: Duration::from_secs(10),
            token_ttl: Duration::from_secs(60),
            max_connections: 8,
            idle_timeout: Duration::from_secs(10),
            session_limit: Duration::from_secs(120),
            max_tasks: 64,
            league: None,
            publisher: Arc::new(crate::profile::PublisherAssets::default()),
            directory: Arc::new(Mutex::new(Directory::default())),
            groups: Arc::new(Mutex::new(Groups::default())),
            presence: Arc::new(Mutex::new(Presence::default())),
        }
    }
}
/// Name-derived identity and the online name it came from.
type Player = (u64, Vec<u8>);
struct Pending {
    ip: IpAddr,
    created: Instant,
    title: u32,
    player: Option<Player>,
}
#[derive(Default)]
struct Sessions {
    pending: Mutex<HashMap<[u8; 24], Pending>>,
    next_id: AtomicU64,
}
impl Sessions {
    fn issue(
        &self,
        ip: IpAddr,
        title: u32,
        player: Option<Player>,
        ttl: Duration,
    ) -> io::Result<[u8; 128]> {
        let mut all = self
            .pending
            .lock()
            .map_err(|_| io::Error::other("session lock poisoned"))?;
        all.retain(|_, v| v.created.elapsed() < ttl);
        if all.len() >= 64 {
            return Err(io::Error::other("pending auth limit"));
        }
        let mut token = [0; 128];
        OsRng
            .try_fill_bytes(&mut token)
            .map_err(|_| io::Error::other("OS random source failed"))?;
        let digest = protocol::token_digest(&token);
        if all.contains_key(&digest) {
            return Err(io::Error::other("token collision"));
        }
        all.insert(
            digest,
            Pending {
                ip,
                title,
                player,
                created: Instant::now(),
            },
        );
        Ok(token)
    }
    fn consume(
        &self,
        token: &[u8; 128],
        ip: IpAddr,
        ttl: Duration,
    ) -> io::Result<(u32, Option<Player>)> {
        let mut all = self
            .pending
            .lock()
            .map_err(|_| io::Error::other("session lock poisoned"))?;
        all.retain(|_, v| v.created.elapsed() < ttl);
        let key = protocol::token_digest(token);
        let Some(pending) = all.get(&key) else {
            return Err(io::Error::other("unknown, expired or reused token"));
        };
        if pending.ip != ip {
            return Err(io::Error::other("token source mismatch"));
        }
        let pending = all.remove(&key).unwrap();
        Ok((pending.title, pending.player))
    }
}
fn invalid(e: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}
fn read_until(stream: &mut TcpStream, bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
    let mut pos = 0;
    while pos < bytes.len() {
        let left = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "frame deadline"))?;
        stream.set_read_timeout(Some(left))?;
        match stream.read(&mut bytes[pos..]) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "peer closed")),
            Ok(n) => pos += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
fn length(stream: &mut TcpStream, deadline: Instant) -> io::Result<usize> {
    let mut raw = [0; 4];
    read_until(stream, &mut raw, deadline)?;
    let n = u32::from_le_bytes(raw) as usize;
    if n > MAX_FRAME {
        return Err(invalid("frame length exceeds limit"));
    }
    Ok(n)
}
fn body(stream: &mut TcpStream, n: usize, prefix: &[u8], deadline: Instant) -> io::Result<Vec<u8>> {
    if n < prefix.len() || n > MAX_FRAME {
        return Err(invalid("invalid body length"));
    }
    let mut out = vec![0; n];
    out[..prefix.len()].copy_from_slice(prefix);
    read_until(stream, &mut out[prefix.len()..], deadline)?;
    Ok(out)
}
fn next_frame(stream: &mut TcpStream, deadline: Instant) -> io::Result<Vec<u8>> {
    for _ in 0..4 {
        let n = length(stream, deadline)?;
        if n != 0 {
            return body(stream, n, &[], deadline);
        }
    }
    Err(invalid("too many empty frames"))
}
/// The original client sends a zero-length frame after 40s without a send
/// (0x80b538) and disconnects after 120s without a receive (0x80b578).
/// A zero-length reply resets its receive timer (0x80c750/0x80c8a4) without
/// entering task dispatch. Keep these frames outside FIFO/seed/task budgets.
fn next_lobby_frame(
    stream: &mut TcpStream,
    config: &Config,
    connection_deadline: Instant,
    id: u64,
) -> io::Result<Vec<u8>> {
    loop {
        let deadline = (Instant::now() + config.idle_timeout).min(connection_deadline);
        let n = length(stream, deadline)?;
        if n != 0 {
            let deadline = (Instant::now() + config.frame_timeout).min(connection_deadline);
            return body(stream, n, &[], deadline);
        }
        stream.write_all(&0u32.to_le_bytes())?;
        println!(
            "lobby.keepalive_reply connection={id} unix_seconds={}",
            unix_now()?
        );
        // A complete heartbeat renews the idle wait, never the session limit.
    }
}
/// Error value sent for tasks the server cannot serve. The session stays open:
/// the client completes that one task as failed and runs its own failure path,
/// instead of reconnecting in a loop. Nothing is fabricated.
const UNSERVED_ERROR: u32 = 1000;
fn unserved(transaction: u64) -> io::Result<Vec<u8>> {
    protocol::error_reply(transaction, UNSERVED_ERROR).map_err(invalid)
}
fn unix_now() -> io::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("clock before Unix epoch"))?
        .as_secs())
}

/// Connection-scoped state removed on every exit path: advertised sessions
/// and group membership.
struct ConnectionLease<'a> {
    config: &'a Config,
    owner: u64,
    connection: u64,
}
impl Drop for ConnectionLease<'_> {
    fn drop(&mut self) {
        if let Ok(mut directory) = self.config.directory.lock() {
            directory.drop_owner(self.owner);
        }
        if let Ok(mut groups) = self.config.groups.lock() {
            groups.drop_connection(self.connection);
        }
        if let Ok(mut presence) = self.config.presence.lock() {
            presence.drop_connection(self.connection);
        }
    }
}

fn connection(
    mut stream: TcpStream,
    peer: SocketAddr,
    config: &Config,
    sessions: &Sessions,
) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_write_timeout(Some(config.frame_timeout))?;
    stream.set_nodelay(true)?;
    let deadline = Instant::now() + config.frame_timeout;
    let mut n = length(&mut stream, deadline)?;
    for _ in 0..3 {
        if n != 0 {
            break;
        }
        n = length(&mut stream, deadline)?;
    }
    if n == 0 {
        return Err(invalid("too many empty opening frames"));
    }
    // A length of 200 alone is ambiguous. Recognize the full observed preamble.
    let mut peek = Vec::new();
    if n == 200 {
        peek.resize(4, 0);
        read_until(&mut stream, &mut peek, deadline)?;
    }
    let lobby = n == 200 && peek == 65535u32.to_le_bytes();
    if !lobby {
        let request = body(&mut stream, n, &peek, deadline)?;
        let auth = protocol::parse_auth(&request).map_err(invalid)?;
        println!(
            "auth.request_parsed title={} ticket_length={} ticket_version={:08x} online_id_present={}",
            auth.title,
            auth.ticket_len,
            auth.ticket_version,
            auth.online_id.is_some()
        );
        let player = auth
            .online_id
            .map(|name| (identity::name_player_id(&name), name));
        if config.league.is_some() && player.is_none() {
            return Err(invalid("League mode requires a ticket online ID"));
        }
        let token = sessions.issue(peer.ip(), auth.title, player, config.token_ttl)?;
        stream.write_all(&protocol::candidate_auth_reply(&token))?;
        println!("auth.candidate_sent experimental=true");
        return Ok(());
    }
    let login = protocol::parse_login(&next_frame(&mut stream, deadline)?).map_err(invalid)?;
    if login.title != 0 || login.seed != 0 {
        return Err(invalid(
            "candidate record not zero; fresh game lifecycle required",
        ));
    }
    let (title, player) = sessions.consume(&login.token, peer.ip(), config.token_ttl)?;
    let id = sessions.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    let caller = match (&config.league, player) {
        (Some(store), Some((player_id, name))) => {
            let now = unix_now()? as i64;
            store
                .lock()
                .map_err(|_| io::Error::other("store lock poisoned"))?
                .record_player(player_id, &name, now)
                .map_err(invalid)?;
            Some(Caller { player_id })
        }
        (Some(_), None) => return Err(invalid("League login without bound identity")),
        (None, _) => None,
    };
    // Player IDs are logged as hashes only; online names are not logged.
    println!(
        "lobby.login_bound connection={id} requested_title={title} league_identity={}",
        caller.map_or("none".into(), |c| format!("{:016x}", c.player_id))
    );
    // Advertised sessions belong to this connection: removed on every exit path.
    let _lease = caller.map(|c| ConnectionLease {
        config,
        owner: c.player_id,
        connection: id,
    });
    stream.write_all(&protocol::ready(id))?;
    println!("lobby.ready_sent connection={id}");
    let connected_at = Instant::now();
    let connection_deadline = connected_at + config.session_limit;
    let mut previous_seed = None;
    let mut non_tasks = 0u32;
    for index in 0..config.max_tasks {
        let frame =
            next_lobby_frame(&mut stream, config, connection_deadline, id).inspect_err(|e| {
                eprintln!(
                    "lobby.read_closed connection={id} elapsed_seconds={} kind={:?} reason={e}",
                    connected_at.elapsed().as_secs(),
                    e.kind()
                );
            })?;
        let message = protocol::decode_client(&frame, &CANDIDATE_KEY).map_err(invalid)?;
        if previous_seed.is_some_and(|s: u32| message.seed <= s) {
            return Err(invalid("repeated or decreasing seed unsupported"));
        }
        previous_seed = Some(message.seed);
        // Task data is the service byte (plaintext byte 4) plus the payload.
        let mut task = Vec::with_capacity(1 + message.payload_with_padding.len());
        task.push(message.inner_type);
        task.extend_from_slice(&message.payload_with_padding);
        let data = &task;
        if data.first() == Some(&0x12) {
            // Initial measurement uses 0x81f6cc: seven u32, port, IPv4 and
            // token. Only the later result uses 0x81e9d8's five u32. The old
            // five-zero "success" truncated initialization and could reach
            // a zero-size allocation before the parser checked its failure.
            // TODO(raw-0x12): implement the actual UDP measurement exchange.
            // Until then report failure in the shared, traced error format.
            println!(
                "lobby.raw_request connection={id} fifo={index} type=0x12 bytes={}",
                data.len()
            );
            let reply = protocol::raw_measurement_failure(u64::from(index) + 1);
            stream.write_all(
                &protocol::encode_server(&reply, &CANDIDATE_KEY, index).map_err(invalid)?,
            )?;
            println!(
                "lobby.raw_reply_sent connection={id} fifo={index} type=5 status=1 error=4 reason=measurement_unimplemented"
            );
            continue;
        }
        if data.len() < 3 || data[1] != 3 {
            // Not a task: no FIFO reply slot. Record it and keep reading.
            non_tasks += 1;
            println!(
                "lobby.non_task connection={id} fifo={index} bytes={} hex={}",
                data.len(),
                data.iter()
                    .take(160)
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            );
            if non_tasks > 64 {
                return Err(invalid("too many non-task messages"));
            }
            continue;
        }
        let (service, operation) = (data[0], data[2]);
        println!(
            "lobby.task_verified connection={id} fifo={index} service={service} operation={operation} bytes_with_padding={} hmac_valid=true",
            data.len()
        );
        let transaction = u64::from(index) + 1;
        if service == 0x0a {
            // File names select client callbacks. Never print the file body,
            // owner identity, or upload bytes here. Debug escaping keeps a
            // client-supplied name from injecting extra log lines.
            if let Ok(request) = storage::parse_task_payload(data) {
                let filename = match &request {
                    storage::Request::PublisherGet { filename }
                    | storage::Request::UserGet { filename, .. }
                    | storage::Request::Upload { filename, .. }
                    | storage::Request::Remove { filename } => Some(filename.as_str()),
                    storage::Request::PublisherList { filename } => filename.as_deref(),
                    storage::Request::UserList { .. } => None,
                };
                println!(
                    "storage.request connection={id} fifo={index} operation={operation} filename={filename:?}"
                );
            }
        }
        let reply = match (&config.league, caller, service) {
            (Some(_), Some(caller), 0x44) => {
                let mut presence = config
                    .presence
                    .lock()
                    .map_err(|_| io::Error::other("presence lock poisoned"))?;
                match presence.handle_task(caller.player_id, id, transaction, data) {
                    Ok(reply) => reply,
                    Err(e) => {
                        println!(
                            "task.unserved connection={id} reason=task_refused service={service} operation={operation} detail=\"{e}\" hex={}",
                            data.iter()
                                .take(160)
                                .map(|b| format!("{b:02x}"))
                                .collect::<String>()
                        );
                        unserved(transaction)?
                    }
                }
            }
            (Some(_), Some(_), 0x1c) => {
                let mut groups = config
                    .groups
                    .lock()
                    .map_err(|_| io::Error::other("groups lock poisoned"))?;
                match groups.handle_task(id, transaction, data) {
                    Ok(reply) => reply,
                    Err(e) => {
                        println!(
                            "task.unserved connection={id} reason=task_refused service={service} operation={operation} detail=\"{e}\" hex={}",
                            data.iter()
                                .take(160)
                                .map(|b| format!("{b:02x}"))
                                .collect::<String>()
                        );
                        unserved(transaction)?
                    }
                }
            }
            (Some(_), Some(caller), 0x15) => {
                let mut directory = config
                    .directory
                    .lock()
                    .map_err(|_| io::Error::other("directory lock poisoned"))?;
                println!(
                    "directory.request connection={id} operation={operation} hex={}",
                    data.iter().map(|b| format!("{b:02x}")).collect::<String>()
                );
                match directory.handle_task(caller.player_id, transaction, data, Instant::now()) {
                    Ok(reply) => reply,
                    Err(e) => {
                        println!(
                            "task.unserved connection={id} reason=task_refused service={service} operation={operation} detail=\"{e}\" hex={}",
                            data.iter()
                                .take(160)
                                .map(|b| format!("{b:02x}"))
                                .collect::<String>()
                        );
                        unserved(transaction)?
                    }
                }
            }
            (
                Some(store),
                Some(caller),
                0x51 | 0x52 | 0x0a | 0x1b | 0x08 | 0x26 | 0x0f | 0x03 | 0x20 | 0x1f | 0x21 | 0x23
                | 0x24 | 0x43 | 0x04 | 0x17,
            ) => {
                let now = unix_now()? as i64;
                if service == 0x04 {
                    // Full stats tasks are kept in the log so the write types
                    // can be determined from real match stats
                    // (TODO(stats-writetype)).
                    println!(
                        "stats.request connection={id} operation={operation} hex={}",
                        data.iter().map(|b| format!("{b:02x}")).collect::<String>()
                    );
                }
                if matches!(service, 0x51 | 0x52) {
                    println!(
                        "league.request connection={id} operation={operation} hex={}",
                        data.iter()
                            .take(160)
                            .map(|b| format!("{b:02x}"))
                            .collect::<String>()
                    );
                }
                let mut store = store
                    .lock()
                    .map_err(|_| io::Error::other("store lock poisoned"))?;
                let handled = if matches!(
                    service,
                    0x1b | 0x08 | 0x26 | 0x03 | 0x20 | 0x1f | 0x21 | 0x23 | 0x24 | 0x43
                ) {
                    startup::handle_task(&mut store, caller.player_id, transaction, data, now)
                } else if matches!(service, 0x0f | 0x17) {
                    archive::handle_task(&mut store, caller.player_id, transaction, data, now)
                } else if service == 0x04 {
                    stats::handle_task(&mut store, caller.player_id, transaction, data, now)
                } else if service == 0x0a {
                    storage::handle_task(
                        &mut store,
                        caller.player_id,
                        transaction,
                        data,
                        now,
                        &config.publisher,
                    )
                } else {
                    league_service::handle_task(&mut store, &caller, transaction, data, now)
                };
                match handled {
                    Ok(reply) => reply,
                    Err(e) => {
                        println!(
                            "task.unserved connection={id} reason=task_refused service={service} operation={operation} detail=\"{e}\" hex={}",
                            data.iter()
                                .take(160)
                                .map(|b| format!("{b:02x}"))
                                .collect::<String>()
                        );
                        unserved(transaction)?
                    }
                }
            }
            _ if config.serve_time && (service, operation) == (0x0c, 6) => {
                // No request fields: service, tag, op plus minimal padding.
                if !protocol::task_tail_valid(data, 3) {
                    return Err(invalid("time request has unexpected fields"));
                }
                let now = u32::try_from(unix_now()?)
                    .map_err(|_| invalid("clock exceeds protocol range"))?;
                protocol::time_reply(transaction, now)
            }
            _ => {
                println!(
                    "task.unserved connection={id} reason=unimplemented service={service} operation={operation} bytes={} hex={}",
                    data.len(),
                    data.iter()
                        .take(160)
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                );
                unserved(transaction)?
            }
        };
        let summary = protocol::task_reply_summary(&reply).map_err(invalid)?;
        stream
            .write_all(&protocol::encode_server(&reply, &CANDIDATE_KEY, index).map_err(invalid)?)?;
        println!(
            "service.reply_sent connection={id} fifo={index} service={service} operation={operation} error={} results={} result_bytes={} reply_bytes={}",
            summary.error,
            summary.results,
            summary.result_bytes,
            reply.len()
        );
    }
    Err(invalid("diagnostic task limit"))
}

/// Stop flag supports bounded loopback integration tests. No DNS or firewall edits.
pub fn serve(listener: TcpListener, config: Config, stop: Arc<AtomicBool>) -> io::Result<()> {
    if config.max_connections == 0
        || config.max_connections > 64
        || config.frame_timeout.is_zero()
        || config.token_ttl.is_zero()
        || config.idle_timeout.is_zero()
        || config.session_limit.is_zero()
        || config.max_tasks == 0
    {
        return Err(invalid("invalid server limits"));
    }
    listener.set_nonblocking(true)?;
    let sessions = Arc::new(Sessions::default());
    let mut workers: Vec<thread::JoinHandle<()>> = vec![];
    while !stop.load(Ordering::Relaxed) {
        let mut i = 0;
        while i < workers.len() {
            if workers[i].is_finished() {
                let _ = workers.swap_remove(i).join();
            } else {
                i += 1;
            }
        }
        match listener.accept() {
            Ok((stream, peer)) => {
                if workers.len() >= config.max_connections {
                    eprintln!("connection.refused peer={} reason=capacity", peer.ip());
                    drop(stream);
                    continue;
                }
                let c = config.clone();
                let s = Arc::clone(&sessions);
                workers.push(thread::spawn(move || {
                    if let Err(e) = connection(stream, peer, &c, &s) {
                        eprintln!("connection.closed reason={e}");
                    }
                }));
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(e) => return Err(e),
        }
    }
    for worker in workers {
        let _ = worker.join();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_is_single_use_and_source_bound() {
        let s = Sessions::default();
        let a = "127.0.0.1".parse().unwrap();
        let b = "127.0.0.2".parse().unwrap();
        let player = Some((42, b"someone".to_vec()));
        let token = s
            .issue(a, 7, player.clone(), Duration::from_secs(60))
            .unwrap();
        assert!(s.consume(&token, b, Duration::from_secs(60)).is_err());
        assert_eq!(
            s.consume(&token, a, Duration::from_secs(60)).unwrap(),
            (7, player)
        );
        assert!(s.consume(&token, a, Duration::from_secs(60)).is_err());
    }
    #[test]
    fn expired_token_is_rejected() {
        let s = Sessions::default();
        let a = "127.0.0.1".parse().unwrap();
        let token = s.issue(a, 7, None, Duration::from_secs(60)).unwrap();
        assert!(s.consume(&token, a, Duration::ZERO).is_err());
    }
}
