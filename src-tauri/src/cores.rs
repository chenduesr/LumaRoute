use crate::{
    config,
    models::{Node, Settings},
    windows::ChildJob,
};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
pub const NO_WINDOW: u32 = 0x08000000;
pub fn exe(root: &Path, kind: &str) -> PathBuf {
    root.join("cores").join(kind).join(if kind == "singbox" {
        "sing-box.exe"
    } else {
        "xray.exe"
    })
}
pub fn needs_singbox(n: &Node) -> bool {
    matches!(n.protocol.as_str(), "anytls" | "tuic")
}
pub fn bridge(port: u16) -> Node {
    Node {
        protocol: "socks".into(),
        address: "127.0.0.1".into(),
        port,
        network: "tcp".into(),
        security: "none".into(),
        ..Node::default()
    }
}
pub fn singbox_config(n: &Node, port: u16) -> Result<Value, String> {
    if !needs_singbox(n) {
        return Err("该节点无需 sing-box".into());
    }
    let mut out = json!({"type":n.protocol,"tag":"proxy","server":n.address,"server_port":n.port,"password":n.password,"tls":{"enabled":true,"server_name":if n.sni.is_empty(){&n.address}else{&n.sni},"insecure":n.allow_insecure}});
    if !n.alpn.is_empty() {
        out["tls"]["alpn"] = json!(config::list(&n.alpn));
    }
    if n.protocol == "tuic" {
        out["uuid"] = json!(n.user_id);
        out["congestion_control"] = json!("cubic");
    }
    Ok(
        json!({"log":{"level":"warn","timestamp":true},"dns":{"servers":[{"type":"local","tag":"local"}]},"inbounds":[{"type":"socks","tag":"bridge","listen":"127.0.0.1","listen_port":port}],"outbounds":[out],"route":{"final":"proxy","default_domain_resolver":"local","auto_detect_interface":true}}),
    )
}
pub fn command(exe: &Path) -> Command {
    let mut c = Command::new(exe);
    c.creation_flags(NO_WINDOW)
        .current_dir(exe.parent().unwrap_or(Path::new(".")));
    c
}
pub fn output(exe: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let file = tempfile::tempfile().map_err(|e| e.to_string())?;
    let err = file.try_clone().map_err(|e| e.to_string())?;
    let read = file.try_clone().map_err(|e| e.to_string())?;
    let mut child = command(exe)
        .args(args)
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err("核心检查超时".into());
        }
        thread::sleep(Duration::from_millis(30));
    };
    use std::io::{Seek, SeekFrom};
    let mut read = read;
    read.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut text = String::new();
    read.take(65536)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(text)
    } else {
        Err(crate::service::redact(&text))
    }
}
pub fn validate_file(root: &Path, kind: &str, path: &Path) -> Result<(), String> {
    let path = path.to_str().ok_or("核心配置路径无效")?;
    let args = if kind == "singbox" {
        vec!["check", "-c", path]
    } else {
        vec!["run", "-test", "-config", path]
    };
    output(&exe(root, kind), &args, Duration::from_secs(15)).map(|_| ())
}
pub struct ProcessSet {
    pub children: Vec<Child>,
    pub configs: tempfile::TempDir,
}
impl Drop for ProcessSet {
    fn drop(&mut self) {
        for c in &mut self.children {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}
impl ProcessSet {
    pub fn new(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root.join("runtime")).map_err(|e| e.to_string())?;
        Ok(Self {
            children: vec![],
            configs: tempfile::tempdir_in(root.join("runtime")).map_err(|e| e.to_string())?,
        })
    }
    pub fn start(
        &mut self,
        root: &Path,
        kind: &str,
        c: &Value,
        port: u16,
        job: &ChildJob,
        log: impl Fn(String) + Send + Sync + 'static,
    ) -> Result<(), String> {
        let path = self.configs.path().join(format!("{kind}.json"));
        crate::storage::atomic_write(&path, &serde_json::to_vec(c).map_err(|e| e.to_string())?)?;
        validate_file(root, kind, &path)?;
        let mut cmd = command(&exe(root, kind));
        if kind == "singbox" {
            cmd.args(["run", "-c"]);
        } else {
            cmd.args(["run", "-config"]);
        }
        cmd.arg(&path).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("无法启动 {kind}：{e}"))?;
        if let Err(e) = job.assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("无法为核心设置退出清理：{e}"));
        }
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let log = std::sync::Arc::new(log);
        for pipe in [
            stdout.map(|s| Box::new(s) as Box<dyn Read + Send>),
            stderr.map(|s| Box::new(s) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let log = log.clone();
            thread::spawn(move || {
                use std::io::{BufRead, BufReader};
                let mut reader = BufReader::new(pipe);
                loop {
                    let mut line = String::new();
                    match reader.by_ref().take(4096).read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        _ => log(line),
                    }
                }
            });
        }
        self.children.push(child);
        let start = Instant::now();
        loop {
            if self
                .children
                .last_mut()
                .unwrap()
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_some()
            {
                return Err(format!("{kind} 启动后退出，请查看核心日志"));
            }
            if std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_millis(100),
            )
            .is_ok()
            {
                return Ok(());
            }
            if start.elapsed() > Duration::from_secs(5) {
                return Err(format!("{kind} 监听端口启动超时"));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}
pub fn unused_port() -> Result<u16, String> {
    let socket = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    Ok(socket.local_addr().map_err(|e| e.to_string())?.port())
}
pub fn config_for(n: &Node, s: &Settings, helper_port: Option<u16>) -> Result<Value, String> {
    match helper_port {
        Some(p) => config::build(&bridge(p), s),
        None => config::build(n, s),
    }
}
