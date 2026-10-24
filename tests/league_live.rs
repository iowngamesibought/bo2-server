//! League mode end to end on loopback: synthetic platform ticket auth, token login and
//! encrypted 0x51/0x52 tasks through the shipped server. Invented values only;
//! this is not a console test.
use bo2_server::{
    identity::name_player_id,
    league,
    league_store::{Division, EntryPolicy, Rules, Season, SeasonState, Store},
    protocol::{self, CANDIDATE_KEY},
    server::{Config, serve},
};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
mod common;

const NAME: &[u8] = b"synthetic_player";

fn store() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let division = |id: i32| Division {
        id,
        skill_min: -10.0,
        skill_max: 10.0,
        subdivision_size: 50,
        can_place: id != 0,
    };
    store
        .put_rules(&Rules {
            league_id: 7,
            revision: 1,
            team_size: 4,
            teams_allowed: true,
            randoms_allowed: true,
            placement_games: 3,
            entry: EntryPolicy::SingleDivision(1),
            divisions: vec![division(0), division(1)],
        })
        .unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let start = now / 60 * 60 - 3600;
    store
        .put_draft(&Season {
            league_id: 7,
            season_id: 1,
            name: "Season 1".into(),
            start,
            end: start + 30 * 86_400,
            rules_revision: 1,
            state: SeasonState::Draft,
        })
        .unwrap();
    store.publish(7, 1).unwrap();
    store
}

struct LeagueServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl LeagueServer {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let mut config = Config::diagnostic();
        config.frame_timeout = Duration::from_secs(2);
        config.idle_timeout = Duration::from_secs(2);
        config.league = Some(Arc::new(Mutex::new(store())));
        config.publisher = Arc::new(
            bo2_server::profile::PublisherAssets::from_profile(&common::profile(), 1).unwrap(),
        );
        config.serve_time = true;
        let worker = thread::spawn(move || serve(listener, config, signal).unwrap());
        Self {
            address,
            stop,
            worker: Some(worker),
        }
    }
    fn connect(&self) -> TcpStream {
        let s = TcpStream::connect(self.address).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        s
    }
}
impl Drop for LeagueServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

/// LSB-first bit packing, as the auth/login codec reads it.
struct Bits {
    bytes: Vec<u8>,
    at: usize,
}
impl Bits {
    fn put(&mut self, value: u32, count: usize) {
        for i in 0..count {
            if self.at / 8 == self.bytes.len() {
                self.bytes.push(0);
            }
            self.bytes[self.at / 8] |= (((value >> i) & 1) as u8) << (self.at % 8);
            self.at += 1;
        }
    }
    fn u32(&mut self, value: u32) {
        self.put(8, 5);
        self.put(value, 32);
    }
}

fn auth_request(ticket: &[u8]) -> Vec<u8> {
    let mut b = Bits {
        bytes: vec![],
        at: 0,
    };
    b.put(1, 1);
    b.u32(0x1234); // seed
    b.u32(0x777); // title
    b.u32(ticket.len() as u32);
    for byte in ticket {
        b.put(u32::from(*byte), 8);
    }
    b.u32(8);
    for _ in 0..8 {
        b.put(0, 8);
    }
    let mut body = vec![0, 18];
    body.extend(b.bytes);
    protocol::frame(&body).unwrap()
}

fn login(token: &[u8; 128]) -> Vec<u8> {
    let mut b = Bits {
        bytes: vec![],
        at: 0,
    };
    b.put(1, 1);
    b.u32(0); // title
    b.u32(0); // seed
    for byte in token {
        b.put(u32::from(*byte), 8);
    }
    let mut body = vec![0, 7];
    body.extend(b.bytes);
    let mut out = 200u32.to_le_bytes().to_vec();
    out.extend(65535u32.to_le_bytes());
    out.extend(protocol::frame(&body).unwrap());
    out
}

fn read_frame(s: &mut TcpStream) -> Vec<u8> {
    loop {
        let mut h = [0; 4];
        s.read_exact(&mut h).unwrap();
        let n = u32::from_le_bytes(h) as usize;
        if n == 0 {
            continue;
        }
        let mut b = vec![0; n];
        s.read_exact(&mut b).unwrap();
        return b;
    }
}

fn token(server: &LeagueServer, name: &[u8]) -> [u8; 128] {
    let mut s = server.connect();
    s.write_all(&auth_request(&common::ticket(name, [8, 1, 7, 7, 2, 4])))
        .unwrap();
    let reply = read_frame(&mut s);
    let bits = &reply[2..];
    let start = 1 + 32 + 32 + 152 * 8;
    std::array::from_fn(|i| {
        let mut v = 0;
        for b in 0..8 {
            let n = start + i * 8 + b;
            v |= ((bits[n / 8] >> (n % 8)) & 1) << b;
        }
        v
    })
}

fn ids(values: &[u64]) -> Vec<u8> {
    let mut out = vec![110, 8];
    out.extend(((values.len() * 8) as u32).to_le_bytes());
    out.extend((values.len() as u32).to_le_bytes());
    for v in values {
        out.extend(v.to_le_bytes());
    }
    out
}
fn u64v(v: u64) -> Vec<u8> {
    let mut out = vec![10];
    out.extend(v.to_le_bytes());
    out
}

struct Lobby {
    stream: TcpStream,
    seed: u32,
}
impl Lobby {
    fn open(server: &LeagueServer) -> Self {
        let token = token(server, NAME);
        let mut stream = server.connect();
        stream.write_all(&login(&token)).unwrap();
        assert_eq!(read_frame(&mut stream), protocol::ready(1)[4..]);
        Self { stream, seed: 0 }
    }
    /// Log in as another identity; the connection ID is not fixed.
    fn open_as(server: &LeagueServer, name: &[u8]) -> Self {
        let token = token(server, name);
        let mut stream = server.connect();
        stream.write_all(&login(&token)).unwrap();
        let ready = read_frame(&mut stream);
        assert_eq!(&ready[..3], &[0, 4, 10]);
        Self { stream, seed: 0 }
    }
    fn recv(&mut self) -> Vec<u8> {
        protocol::decode_server_fixture(&read_frame(&mut self.stream), &CANDIDATE_KEY).unwrap()
    }
    fn send(&mut self, service: u8, op: u8, body: &[u8]) {
        let mut payload = vec![service, 3, op];
        payload.extend(body);
        payload.push(0);
        let frame = protocol::encode_client_fixture(&payload, &CANDIDATE_KEY, self.seed).unwrap();
        self.seed += 1;
        self.stream.write_all(&frame).unwrap();
    }
    /// Next reply, checked against the independently built expected envelope.
    fn expect(&mut self, expected: &[u8]) {
        let plain =
            protocol::decode_server_fixture(&read_frame(&mut self.stream), &CANDIDATE_KEY).unwrap();
        assert_eq!(&plain[..expected.len()], expected);
        assert!(plain[expected.len()..].iter().all(|&b| b == 0));
    }
}

#[test]
fn league_session_serves_team_subdivision_upload_and_stats() {
    let server = LeagueServer::new();
    let me = name_player_id(NAME);
    let mut lobby = Lobby::open(&server);

    lobby.send(0x51, 1, &ids(&[me]));
    let team = 1;
    lobby.expect(&protocol::results_reply(1, 1, &[league::team_id_result(team)]).unwrap());

    let mut body = u64v(team);
    body.extend(ids(&[7]));
    lobby.send(0x51, 3, &body);
    let sub = 1;
    let expected = league::subdivision_result(team, 7, sub, b"1,1").unwrap();
    lobby.expect(&protocol::results_reply(2, 3, &[expected]).unwrap());

    // Win snapshot: wins 0 -> 1. Acknowledged with an empty result.
    let mut upload = u64v(team);
    upload.extend(u64v(sub));
    upload.extend([3, 0]);
    upload.extend(u64v(0));
    for counter in [9, 1, 1, 0, 0, 0, 1, 1] {
        upload.push(7);
        upload.extend(i32::to_le_bytes(counter));
    }
    for skill in [1.5f32, 0.5, 0.0] {
        upload.push(13);
        upload.extend(skill.to_le_bytes());
    }
    lobby.send(0x52, 1, &upload);
    lobby.expect(&protocol::empty_success(3, 1));

    let mut ranked = u64v(team);
    ranked.extend(u64v(sub));
    ranked.extend([8, 1, 0, 0, 0]);
    lobby.send(0x52, 4, &ranked);
    let row = league::StatsResult {
        team_id: team,
        subdivision_id: sub,
        name: vec![],
        field_58: 0,
        rank: 1,
        rank_points: 0,
        real_points: 0,
        played: 0,
        wins: 1,
        losses: 0,
        bonus_used: 0,
        bonus_time: 0,
        streak: 0,
        career_wins: 0,
        skill: 0.0,
        variance: 1.0,
        placement_skill: 0.0,
    };
    lobby.expect(&protocol::results_reply(4, 4, &[row.encode().unwrap()]).unwrap());

    // Member list resolves the stored online name for this identity.
    lobby.send(0x51, 8, &ids(&[team]));
    let members = [league::TeamMember {
        id: me,
        name: NAME.to_vec(),
    }];
    lobby.expect(
        &protocol::results_reply(
            5,
            8,
            &[league::team_members_result(team, &members).unwrap()],
        )
        .unwrap(),
    );

    // Requesting a team ID for someone else is refused: an error reply, no
    // team created or returned.
    lobby.send(0x51, 1, &ids(&[me ^ 1]));
    lobby.expect(&protocol::error_reply(6, 1000).unwrap());

    // A party member who has signed in gets the leader's existing solo team
    // (console run: members look up each other's teams and retry on refusal).
    let mut mate = Lobby::open_as(&server, b"synthetic_mate");
    mate.send(0x51, 1, &ids(&[me]));
    mate.expect(&protocol::results_reply(1, 1, &[league::team_id_result(team)]).unwrap());
}

#[test]
fn league_mode_refuses_auth_without_ticket_online_id() {
    let server = LeagueServer::new();
    let mut s = server.connect();
    // Version 2.1 tickets are not parsed, so no identity can be bound.
    let mut ticket = common::ticket(NAME, [8, 1, 7, 7, 2, 4]);
    ticket[0] = 0x21;
    s.write_all(&auth_request(&ticket)).unwrap();
    let mut one = [0];
    assert!(matches!(s.read(&mut one), Ok(0) | Err(_)));
}

/// Original-serializer directory requests (see tests/directory.rs).
const CREATE_BODY: &str = concat!(
    "130825000000404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f60",
    "61626364082200000008120000000a0807060504030201130810000000a0a1a2a3a4a5a6a7a8a9aaabac",
    "adaeaf07e80300000701000000070700000014070b00000007e9030000070000000007030000000d0000",
    "c03f07050000001407060000000707000000"
);
const SEARCH_BODY: &str = concat!(
    "080200000008000000000832000000070100000007070000000703000000070b00000007e90300",
    "00070000000007020000000d0000c03f0d0000803e07050000000706000000070700000007080000000d",
    "0000003f0d0000803e0d0000003e0d0000803f"
);

fn hexb(v: &str) -> Vec<u8> {
    v.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}
fn strv(s: &str) -> Vec<u8> {
    let mut out = vec![16];
    out.extend(s.as_bytes());
    out.push(0);
    out
}
fn groups(values: &[u32]) -> Vec<u8> {
    let mut out = vec![108, 8];
    out.extend(((values.len() * 4) as u32).to_le_bytes());
    out.extend((values.len() as u32).to_le_bytes());
    for v in values {
        out.extend(v.to_le_bytes());
    }
    out
}

#[test]
fn two_consoles_start_up_host_and_find_each_other() {
    use bo2_server::{groups::count_result, startup, storage};
    let server = LeagueServer::new();
    let wad = bo2_server::league_config::online_wad(&common::profile(), 1).unwrap();
    let mut host = Lobby::open_as(&server, b"synthetic_host");

    // Startup order traced in DEC08: time, assets, stats, then optional services.
    host.send(0x0c, 6, &[]);
    let time = host.recv();
    assert_eq!(time[0], 1);
    host.send(0x0a, 7, &strv("example_online_english.bundle"));
    host.expect(&protocol::results_reply(2, 7, &[storage::blob_result(&wad)]).unwrap());
    host.send(0x0a, 7, &strv("optional_patch.00"));
    host.expect(&protocol::results_reply(3, 7, &[storage::blob_result(&[0; 16])]).unwrap());
    let mut stats_get = strv("player_state");
    stats_get.extend(u64v(0));
    host.send(0x0a, 3, &stats_get);
    host.expect(&protocol::error_reply(4, storage::ERROR_NO_FILE).unwrap());
    host.send(0x1b, 3, &[]);
    host.expect(&protocol::results_reply(5, 3, &[startup::geolocation_result()]).unwrap());
    host.send(0x1c, 1, &groups(&[1]));
    host.expect(&protocol::empty_success(6, 1));
    host.send(0x15, 1, &hexb(CREATE_BODY));
    let created = host.recv();
    assert_eq!(created[0], 1);

    let mut joiner = Lobby::open_as(&server, b"synthetic_joiner");
    joiner.send(0x1c, 4, &groups(&[0, 1]));
    joiner
        .expect(&protocol::results_reply(1, 4, &[count_result(0, 0), count_result(1, 1)]).unwrap());
    joiner.send(0x15, 5, &hexb(SEARCH_BODY));
    let found = joiner.recv();
    // One search result whose directory ID equals the one the host received.
    // Envelope header (27 bytes) then blob tag/length (6), then the 8-byte ID;
    // decrypted frames carry zero padding after it.
    let id = &created[33..41];
    assert_eq!(
        &created[..41],
        &protocol::results_reply(
            7,
            1,
            &[bo2_server::league_directory::create_id_result(
                id.try_into().unwrap()
            )]
        )
        .unwrap()[..]
    );
    assert!(found.windows(8).any(|w| w == id));

    // Host leaves: its session and group membership disappear.
    drop(host);
    std::thread::sleep(Duration::from_millis(300));
    joiner.send(0x15, 5, &hexb(SEARCH_BODY));
    joiner.expect(&protocol::empty_success(3, 5));
    joiner.send(0x1c, 4, &groups(&[1]));
    joiner.expect(&protocol::results_reply(4, 4, &[count_result(1, 0)]).unwrap());
}
