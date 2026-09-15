use super::*;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicUsize;

fn dual_port() -> u16 {
    loop {
        let u = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let p = u.local_addr().unwrap().port();
        if TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }
}
const UUID: &str = "00000000-0000-0000-0000-000000000001";
struct HttpFixture {
    port: u16,
    response: Arc<Mutex<(u16, String)>>,
    hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
}
impl HttpFixture {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let response = Arc::new(Mutex::new((200, "local-test-data".repeat(8192))));
        let hits = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (r, h, s) = (response.clone(), hits.clone(), stop.clone());
        thread::spawn(move || {
            while !s.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(3)))
                            .unwrap();
                        let mut buf = [0; 4096];
                        if stream.read(&mut buf).unwrap_or(0) > 0 {
                            h.fetch_add(1, Ordering::SeqCst);
                            let (status, body) = r.lock().unwrap().clone();
                            let _ = write!(stream, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                        }
                    }
                    Err(_) => thread::sleep(Duration::from_millis(10)),
                }
            }
        });
        Self {
            port,
            response,
            hits,
            stop,
        }
    }
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/fixture", self.port)
    }
}
impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

#[test]
fn real_cores_isolated_protocols_connections_subscriptions_and_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_path_buf();
    let service = Service::new(
        root.clone(),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"),
        true,
    )
    .unwrap();
    let http = HttpFixture::new();
    let s = Settings {
        system_proxy: false,
        proxy_mode: "global".into(),
        http_port: cores::unused_port().unwrap(),
        socks_port: dual_port(),
        test_url: http.url(),
        test_timeout: 5,
        test_retries: 0,
        download_bytes: 65536,
        ..Settings::default()
    };
    service.save_settings(s.clone()).unwrap();

    // Run the actual shipped executables against every supported protocol and transport.
    let samples = [
        format!("vless://{UUID}@127.0.0.1:443#vless"),
        format!("trojan://test-password@127.0.0.1:443#trojan"),
        "ss://YWVzLTEyOC1nY206dGVzdA@127.0.0.1:443#ss".into(),
        "socks://u:p@127.0.0.1:443#socks".into(),
        "http://u:p@127.0.0.1:443#http".into(),
        "hy2://p@127.0.0.1:443?obfs=salamander&obfs-password=fixture#hy2".into(),
        "anytls://p@127.0.0.1:443#anytls".into(),
        format!("tuic://{UUID}:p@127.0.0.1:443#tuic"),
    ];
    for sample in &samples {
        let (nodes, _) = parser::parse(sample).unwrap();
        let n = &nodes[0];
        let (kind, cfg) = if cores::needs_singbox(n) {
            ("singbox", cores::singbox_config(n, 54321).unwrap())
        } else {
            ("xray", config::build(n, &s).unwrap())
        };
        let path = root.join("validate.json");
        storage::atomic_write(&path, &serde_json::to_vec(&cfg).unwrap()).unwrap();
        cores::validate_file(&root, kind, &path)
            .unwrap_or_else(|e| panic!("{} rejected: {e}", n.protocol));
    }
    let base = parser::parse(&samples[0]).unwrap().0.remove(0);
    let mut vmess = base.clone();
    vmess.protocol = "vmess".into();
    vmess.vmess_security = "auto".into();
    let mut reality = base.clone();
    reality.security = "reality".into();
    reality.sni = "example.com".into();
    reality.fingerprint = "chrome".into();
    reality.public_key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7u8; 32]);
    for n in [vmess, reality] {
        let path = root.join("validate.json");
        storage::atomic_write(
            &path,
            &serde_json::to_vec(&config::build(&n, &s).unwrap()).unwrap(),
        )
        .unwrap();
        cores::validate_file(&root, "xray", &path).unwrap();
    }
    for transport in ["tcp", "ws", "grpc", "httpupgrade", "xhttp", "kcp"] {
        let mut n = base.clone();
        n.network = transport.into();
        n.path = "/test".into();
        n.service_name = "test".into();
        let cfg = config::build(&n, &s).unwrap();
        let path = root.join("validate.json");
        storage::atomic_write(&path, &serde_json::to_vec(&cfg).unwrap()).unwrap();
        cores::validate_file(&root, "xray", &path)
            .unwrap_or_else(|e| panic!("{transport} rejected: {e}"));
    }
    let mut dns = s.clone();
    dns.proxy_mode = "rule".into();
    dns.custom_dns = true;
    dns.fake_dns = true;
    let path = root.join("validate.json");
    storage::atomic_write(
        &path,
        &serde_json::to_vec(&config::build(&base, &dns).unwrap()).unwrap(),
    )
    .unwrap();
    cores::validate_file(&root, "xray", &path).unwrap();

    // HTTPS must use the configured proxy too. A dead proxy must never contact the target directly.
    let https_target = HttpFixture::new();
    let mut https_settings = s.clone();
    https_settings.test_url = https_target.url().replacen("http:", "https:", 1);
    https_settings.test_timeout = 2;
    let mut dead_node = base.clone();
    dead_node.port = cores::unused_port().unwrap();
    assert!(service
        .test_node(&dead_node, "http", &https_settings)
        .is_err());
    thread::sleep(Duration::from_millis(100));
    assert_eq!(
        https_target.hits.load(Ordering::SeqCst),
        0,
        "HTTPS bypassed the proxy"
    );

    // Local servers use only generated test credentials and a fixture certificate.
    let any_port = cores::unused_port().unwrap();
    let tuic_port = std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let hy_port = std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let ready_port = cores::unused_port().unwrap();
    let certs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures");
    let tls = json!({"enabled":true,"certificate_path":certs.join("localhost-cert.pem"),"key_path":certs.join("localhost-key.pem")});
    let server_config = json!({"log":{"level":"error"},"inbounds":[
        {"type":"socks","listen":"127.0.0.1","listen_port":ready_port},
        {"type":"anytls","listen":"127.0.0.1","listen_port":any_port,"users":[{"password":"fixture-password"}],"tls":tls},
        {"type":"tuic","listen":"127.0.0.1","listen_port":tuic_port,"users":[{"uuid":UUID,"password":"fixture-password"}],"tls":tls},
        {"type":"hysteria2","listen":"127.0.0.1","listen_port":hy_port,"users":[{"password":"fixture-password"}],"tls":tls}
    ],"outbounds":[{"type":"direct"}]});
    let mut servers = ProcessSet::new(&root).unwrap();
    servers
        .start(
            &root,
            "singbox",
            &server_config,
            ready_port,
            &service.child_job,
            |l| eprintln!("server: {l}"),
        )
        .unwrap();
    let vless_port = cores::unused_port().unwrap();
    let xray_server = json!({"log":{"loglevel":"warning"},"inbounds":[{"listen":"127.0.0.1","port":vless_port,"protocol":"vless","settings":{"clients":[{"id":UUID}],"decryption":"none"}}],"outbounds":[{"protocol":"freedom"}]});
    servers
        .start(
            &root,
            "xray",
            &xray_server,
            vless_port,
            &service.child_job,
            |l| eprintln!("server: {l}"),
        )
        .unwrap();
    let pem = fs::read_to_string(certs.join("localhost-cert.pem")).unwrap();
    use base64::Engine;
    use sha2::Digest;
    let der = base64::engine::general_purpose::STANDARD
        .decode(
            pem.lines()
                .filter(|l| !l.starts_with("---"))
                .collect::<String>(),
        )
        .unwrap();
    let cert_hash = format!("{:x}", sha2::Sha256::digest(&der));
    let links = [
        format!("vless://{UUID}@127.0.0.1:{vless_port}#local-vless"),
        format!("anytls://fixture-password@127.0.0.1:{any_port}?insecure=1#local-anytls"),
        format!("tuic://{UUID}:fixture-password@127.0.0.1:{tuic_port}?insecure=1#local-tuic"),
        format!("hy2://fixture-password@127.0.0.1:{hy_port}?pinSHA256={cert_hash}#local-hy2"),
    ];
    service.import(&links.join("\n")).unwrap();
    let hy_id = service
        .snapshot()
        .data
        .nodes
        .iter()
        .find(|n| n.protocol == "hysteria2")
        .unwrap()
        .id
        .clone();
    assert!(service.set_node_pin(&hy_id, "invalid").is_err());
    service.set_node_pin(&hy_id, &cert_hash).unwrap();
    for node in service.snapshot().data.nodes {
        let before = http.hits.load(Ordering::SeqCst);
        let (latency, _) = service.test_node(&node, "http", &s).unwrap_or_else(|e| {
            panic!(
                "{} HTTP: {e}; logs: {:?}",
                node.protocol,
                service.snapshot().logs
            )
        });
        assert!(latency < 5000);
        let (_, speed) = service.test_node(&node, "download", &s).unwrap();
        assert!(speed.unwrap() > 0.0);
        assert!(http.hits.load(Ordering::SeqCst) >= before + 2);
        service.connect(Some(node.id.clone())).unwrap_or_else(|e| {
            panic!(
                "connect {}: {e}; {:?}",
                node.protocol,
                service.snapshot().logs
            )
        });
        assert_eq!(
            service.snapshot().data.active_node_id,
            Some(node.id.clone())
        );
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{}", s.http_port)).unwrap())
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        assert!(client
            .get(http.url())
            .send()
            .unwrap()
            .text()
            .unwrap()
            .starts_with("local-test-data"));
        service.disconnect().unwrap();
        assert!(TcpStream::connect(("127.0.0.1", s.http_port)).is_err());
        assert!(!root.join("system-proxy-backup.json").exists());
        service.cancel.store(false, Ordering::SeqCst);
    }

    // A blocked port must not leave any child or a connected state behind.
    let held = TcpListener::bind(("127.0.0.1", s.http_port)).unwrap();
    assert!(service.connect(None).unwrap_err().contains("端口已占用"));
    assert!(service.runtime.lock().unwrap().processes.is_none());
    drop(held);
    service.background();
    service.connect(None).unwrap();
    service
        .runtime
        .lock()
        .unwrap()
        .processes
        .as_mut()
        .unwrap()
        .children[0]
        .kill()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while service.snapshot().connection.status != "disconnected" && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(service.snapshot().connection.status, "disconnected");

    let sub_http = HttpFixture::new();
    *sub_http.response.lock().unwrap() = (200, links[0].clone());
    service
        .save_subscription(Subscription {
            id: "fixture-sub".into(),
            name: "Fixture".into(),
            url: sub_http.url(),
            ..Subscription::default()
        })
        .unwrap();
    service.update_subscription("fixture-sub").unwrap();
    let nodes_before = service.snapshot().data.nodes.len();
    *sub_http.response.lock().unwrap() = (500, "failure".into());
    assert!(service.update_subscription("fixture-sub").is_err());
    assert_eq!(service.snapshot().data.nodes.len(), nodes_before);
    service.cancel();
    assert!(service.update_subscription("fixture-sub").is_err());
    assert_eq!(service.snapshot().data.nodes.len(), nodes_before);
    service.cancel.store(false, Ordering::SeqCst);
    let export = service.export_diagnostics().unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(export).unwrap()).unwrap();
    for i in 0..zip.len() {
        let mut text = String::new();
        zip.by_index(i).unwrap().read_to_string(&mut text).unwrap();
        assert!(!text.contains("fixture-password"));
        assert!(!text.contains(&sub_http.url()));
    }
    service.shutdown();
    assert!(storage::load(&root).is_ok());
}

