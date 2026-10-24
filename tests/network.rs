use bo2_server::{
    protocol,
    server::{Config, serve},
};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
fn fixture(name: &str) -> Vec<u8> {
    let v: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/synthetic.json")).unwrap();
    v[name]
        .as_str()
        .unwrap()
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect()
}
struct TestServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl TestServer {
    fn new(time: bool) -> Self {
        Self::configured(time, |_| {})
    }
    fn configured(time: bool, configure: impl FnOnce(&mut Config)) -> Self {
        Self::bound("127.0.0.1:0", time, configure)
    }
    fn bound(bind: &str, time: bool, configure: impl FnOnce(&mut Config)) -> Self {
        let listener = TcpListener::bind(bind).unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let mut config = Config::diagnostic();
        config.serve_time = time;
        config.frame_timeout = Duration::from_secs(2);
        configure(&mut config);
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
impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}
fn read_frame(s: &mut TcpStream) -> Vec<u8> {
    loop {
        let mut h = [0; 4];
        s.read_exact(&mut h).unwrap();
        let n = u32::from_le_bytes(h) as usize;
        if n == 0 {
            continue;
        }
        assert!(n <= 65535);
        let mut b = vec![0; n];
        s.read_exact(&mut b).unwrap();
        return b;
    }
}
fn auth_token(server: &TestServer) -> [u8; 128] {
    let mut s = server.connect();
    let auth = fixture("auth_request");
    for chunk in auth.chunks(3) {
        s.write_all(chunk).unwrap();
    }
    let response = read_frame(&mut s);
    assert_eq!(&response[..2], &[0, 19]);
    // Independent test decoder: untyped bit flag + status32 + seed32 + cipher152.
    let bits = &response[2..];
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
fn login_bytes(token: &[u8; 128]) -> Vec<u8> {
    let mut login = fixture("login");
    // Replace the public fixture token in the independent bit-packed login.
    let start = 14 * 8 + 1 + 37 + 37;
    for (i, v) in token.iter().enumerate() {
        for b in 0..8 {
            let n = start + i * 8 + b;
            login[n / 8] = (login[n / 8] & !(1 << (n % 8))) | (((v >> b) & 1) << (n % 8));
        }
    }
    login
}
#[test]
fn clients_complete_handshakes_without_ip_enrollment() {
    let server = TestServer::bound("127.0.0.2:0", false, |_| {});
    let first_token = auth_token(&server);
    let second_token = auth_token(&server);
    assert_ne!(first_token, second_token);
    let mut first = server.connect();
    let mut second = server.connect();
    // No client address is supplied to Config, even for a different bind address.
    for (stream, token, connection_id) in [
        (&mut first, &first_token, 1),
        (&mut second, &second_token, 2),
    ] {
        stream.write_all(&login_bytes(token)).unwrap();
        assert_eq!(read_frame(stream), protocol::ready(connection_id)[4..]);
    }
}
#[test]
fn fragmented_auth_login_then_unknown_task_gets_error_not_success() {
    let server = TestServer::new(false);
    let token = auth_token(&server);
    let mut s = server.connect();
    for b in login_bytes(&token) {
        s.write_all(&[b]).unwrap();
    }
    assert_eq!(read_frame(&mut s), protocol::ready(1)[4..]);
    s.write_all(
        &protocol::encode_client_fixture(&[0x15, 3, 5, 0], &protocol::CANDIDATE_KEY, 0).unwrap(),
    )
    .unwrap();
    // Unserved tasks get an error reply (never success) and the session stays
    // open, so a real client completes the task as failed instead of looping.
    let reply =
        protocol::decode_server_fixture(&read_frame(&mut s), &protocol::CANDIDATE_KEY).unwrap();
    let expected = protocol::error_reply(1, 1000).unwrap();
    assert_eq!(&reply[..expected.len()], &expected[..]);
    let mut one = [0];
    let mut replay = server.connect();
    replay.write_all(&login_bytes(&token)).unwrap();
    assert_eq!(replay.read(&mut one).unwrap(), 0);
}
#[test]
fn coalesced_time_requests_receive_ordered_replies() {
    let server = TestServer::new(true);
    let token = auth_token(&server);
    let mut s = server.connect();
    s.write_all(&login_bytes(&token)).unwrap();
    assert_eq!(read_frame(&mut s)[1], 4);
    let mut both =
        protocol::encode_client_fixture(&[12, 3, 6, 0], &protocol::CANDIDATE_KEY, 0).unwrap();
    both.extend(
        protocol::encode_client_fixture(&[12, 3, 6, 0], &protocol::CANDIDATE_KEY, 1).unwrap(),
    );
    s.write_all(&both).unwrap();
    // Compare entire encrypted reply to independently anchored encoder; clock can tick.
    for index in 0..2u32 {
        let response = read_frame(&mut s);
        assert_eq!(response[0], 1);
        assert_eq!(
            u32::from_le_bytes(response[1..5].try_into().unwrap()),
            index
        );
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32;
        assert!((now - 2..=now).any(|t| {
            protocol::encode_server(
                &protocol::time_reply(u64::from(index) + 1, t),
                &protocol::CANDIDATE_KEY,
                index,
            )
            .unwrap()[4..]
                == response
        }));
    }
    drop(s);
}
#[test]
fn oversized_frame_is_closed_before_body_read() {
    let server = TestServer::new(false);
    let mut s = server.connect();
    s.write_all(&65536u32.to_le_bytes()).unwrap();
    let mut one = [0];
    assert_eq!(s.read(&mut one).unwrap(), 0);
}

#[test]
fn raw_measurement_failure_preserves_following_task_order() {
    let server = TestServer::new(true);
    let token = auth_token(&server);
    let mut s = server.connect();
    s.write_all(&login_bytes(&token)).unwrap();
    assert_eq!(read_frame(&mut s)[1], 4);
    // Synthetic raw service/op plus phase-zero payload, then a typed time task.
    // No captured account or console bytes.
    let mut raw = vec![0x12, 1];
    raw.extend([0; 16]);
    let mut both = protocol::encode_client_fixture(&raw, &protocol::CANDIDATE_KEY, 0).unwrap();
    both.extend(
        protocol::encode_client_fixture(&[12, 3, 6, 0], &protocol::CANDIDATE_KEY, 1).unwrap(),
    );
    s.write_all(&both).unwrap();
    let failed =
        protocol::decode_server_fixture(&read_frame(&mut s), &protocol::CANDIDATE_KEY).unwrap();
    assert_eq!(failed, [5, 1, 0, 0, 0, 0, 0, 0, 0, 1, 4, 0]);
    let time =
        protocol::decode_server_fixture(&read_frame(&mut s), &protocol::CANDIDATE_KEY).unwrap();
    assert_eq!(&time[..11], &[1, 10, 2, 0, 0, 0, 0, 0, 0, 0, 8]);
    assert_eq!(&time[11..18], &[0, 0, 0, 0, 3, 6, 8]);
    drop(s);
}

#[test]
fn repeated_and_fragmented_keepalives_preserve_task_and_seed_order() {
    let server = TestServer::configured(true, |c| c.max_tasks = 2);
    let token = auth_token(&server);
    let mut s = server.connect();
    s.write_all(&login_bytes(&token)).unwrap();
    assert_eq!(read_frame(&mut s)[1], 4);
    // Six heartbeats exceed the old four-empty-frame cap. The first header
    // is fragmented and the remainder are coalesced with a protected task.
    s.write_all(&[0, 0]).unwrap();
    let mut batch = vec![0; 22];
    batch.extend(
        protocol::encode_client_fixture(&[12, 3, 6, 0], &protocol::CANDIDATE_KEY, 0).unwrap(),
    );
    s.write_all(&batch).unwrap();
    let mut echoes = [255; 24];
    s.read_exact(&mut echoes).unwrap();
    assert_eq!(echoes, [0; 24]);
    let first = read_frame(&mut s);
    assert_eq!(u32::from_le_bytes(first[1..5].try_into().unwrap()), 0);
    let decoded = protocol::decode_server_fixture(&first, &protocol::CANDIDATE_KEY).unwrap();
    assert_eq!(&decoded[..10], &[1, 10, 1, 0, 0, 0, 0, 0, 0, 0]);
    s.write_all(&[0; 4]).unwrap();
    let mut echo = [255; 4];
    s.read_exact(&mut echo).unwrap();
    assert_eq!(echo, [0; 4]);
    s.write_all(
        &protocol::encode_client_fixture(&[12, 3, 6, 0], &protocol::CANDIDATE_KEY, 1).unwrap(),
    )
    .unwrap();
    let second = read_frame(&mut s);
    assert_eq!(u32::from_le_bytes(second[1..5].try_into().unwrap()), 1);
    let decoded = protocol::decode_server_fixture(&second, &protocol::CANDIDATE_KEY).unwrap();
    assert_eq!(&decoded[..10], &[1, 10, 2, 0, 0, 0, 0, 0, 0, 0]);
    drop(s);
}

#[test]
fn keepalives_renew_idle_wait_but_silence_still_expires() {
    let server = TestServer::configured(true, |c| {
        c.idle_timeout = Duration::from_millis(600);
    });
    let token = auth_token(&server);
    let mut s = server.connect();
    s.write_all(&login_bytes(&token)).unwrap();
    assert_eq!(read_frame(&mut s)[1], 4);
    // Total quiet-task time exceeds idle_timeout, but each complete heartbeat
    // is activity. After the last heartbeat, true silence must still close.
    for _ in 0..6 {
        thread::sleep(Duration::from_millis(150));
        s.write_all(&[0; 4]).unwrap();
        let mut echo = [255; 4];
        s.read_exact(&mut echo).unwrap();
        assert_eq!(echo, [0; 4]);
    }
    assert_eq!(s.read(&mut [0]).unwrap(), 0);
    drop(s);
}

#[test]
fn keepalives_do_not_extend_total_session_limit() {
    let server = TestServer::configured(true, |c| {
        c.session_limit = Duration::from_millis(600);
    });
    let token = auth_token(&server);
    let mut s = server.connect();
    s.write_all(&login_bytes(&token)).unwrap();
    assert_eq!(read_frame(&mut s)[1], 4);
    for _ in 0..3 {
        s.write_all(&[0; 4]).unwrap();
        let mut echo = [255; 4];
        s.read_exact(&mut echo).unwrap();
        assert_eq!(echo, [0; 4]);
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(s.read(&mut [0]).unwrap(), 0);
    drop(s);
}

#[test]
fn keepalive_does_not_remove_nonempty_frame_body_deadline() {
    let server = TestServer::configured(true, |c| {
        c.frame_timeout = Duration::from_millis(400);
    });
    let token = auth_token(&server);
    let mut s = server.connect();
    s.write_all(&login_bytes(&token)).unwrap();
    assert_eq!(read_frame(&mut s)[1], 4);
    s.write_all(&[0; 4]).unwrap();
    let mut echo = [255; 4];
    s.read_exact(&mut echo).unwrap();
    assert_eq!(echo, [0; 4]);
    s.write_all(&16u32.to_le_bytes()).unwrap();
    assert_eq!(s.read(&mut [0]).unwrap(), 0);
    drop(s);
}
