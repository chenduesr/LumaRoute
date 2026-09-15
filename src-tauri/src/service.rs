use crate::{
    config,
    cores::{self, ProcessSet},
    models::*,
    parser, storage, windows,
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub fn redact(text: &str) -> String {
    static URL: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)(?:https?|vmess|vless|trojan|ss|socks|anytls|tuic|hy2|hysteria2)://[^\s]+|[0-9a-f]{8}-[0-9a-f-]{27,}|\b(?:\d{1,3}\.){3}\d{1,3}\b").unwrap()
    });
    URL.replace_all(text, "[已脱敏]")
        .chars()
        .take(2000)
        .collect()
}
struct Runtime {
    processes: Option<ProcessSet>,
    connection: Connection,
}
pub struct Service {
    pub root: PathBuf,
    pub data: Mutex<Data>,
    runtime: Mutex<Runtime>,
    operation: Mutex<()>,
    pub job: Mutex<Job>,
    cancel: AtomicBool,
    pub exit: AtomicBool,
    logs: Mutex<VecDeque<LogEntry>>,
    core: Mutex<CoreInfo>,
    traffic: Mutex<Traffic>,
    storage_error: Mutex<Option<String>>,
    pub child_job: windows::ChildJob,
    pub isolated: bool,
}
impl Service {
    pub fn new(root: PathBuf, resources: PathBuf, isolated: bool) -> Result<Arc<Self>, String> {
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let (data, error) = match storage::load(&root) {
            Ok(d) => (d, None),
            Err(e) => (Data::default(), Some(e)),
        };
        for kind in ["xray", "singbox"] {
            let target = root.join("cores").join(kind);
            if !cores::exe(&root, kind).exists() {
                fs::create_dir_all(&target).map_err(|e| e.to_string())?;
                let source = resources.join(kind);
                if source.exists() {
                    for f in fs::read_dir(source).map_err(|e| e.to_string())?.flatten() {
                        if f.file_type().map(|t| t.is_file()).unwrap_or(false) {
                            fs::copy(f.path(), target.join(f.file_name()))
                                .map_err(|e| e.to_string())?;
                        }
                    }
                }
            }
        }
        let service = Arc::new(Self {
            root,
            data: Mutex::new(data),
            runtime: Mutex::new(Runtime {
                processes: None,
                connection: Connection {
                    status: "disconnected".into(),
                    ..Connection::default()
                },
            }),
            operation: Mutex::new(()),
            job: Mutex::new(Job::default()),
            cancel: AtomicBool::new(false),
            exit: AtomicBool::new(false),
            logs: Mutex::new(VecDeque::new()),
            core: Mutex::new(CoreInfo::default()),
            traffic: Mutex::new(Traffic::default()),
            storage_error: Mutex::new(error),
            child_job: windows::ChildJob::new()?,
            isolated,
        });
        service.refresh_core();
        if !isolated {
            match windows::restore(&service.root) {
                Ok(true) => service.log("WARN", "system", "已恢复上次异常退出遗留的系统代理"),
                Err(e) => service.log("ERROR", "system", &e),
                _ => {}
            }
        }
        service.log("INFO", "app", "MyRay Lite 原生业务服务已启动");
        Ok(service)
    }
    pub fn log(&self, level: &str, source: &str, message: &str) {
        let entry = LogEntry {
            timestamp: now(),
            level: level.into(),
            source: source.into(),
            message: redact(message),
        };
        let mut logs = self.logs.lock().unwrap();
        logs.push_back(entry.clone());
        while logs.len() > 1000 {
            logs.pop_front();
        }
        let path = self.root.join("app.log");
        if fs::metadata(&path)
            .map(|m| m.len() > 2 * 1024 * 1024)
            .unwrap_or(false)
        {
            let previous = self.root.join("app.previous.log");
            if previous.exists() {
                let _ = fs::remove_file(&previous);
            }
            let _ = fs::rename(&path, previous);
        }
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(
                file,
                "{}",
                serde_json::to_string(&entry).unwrap_or_default()
            );
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        let data = self.data.lock().unwrap().clone();
        let connection = self.runtime.lock().unwrap().connection.clone();
        let job = self.job.lock().unwrap().clone();
        let traffic = self.traffic.lock().unwrap().clone();
        let core = self.core.lock().unwrap().clone();
        let logs = self.logs.lock().unwrap().iter().cloned().collect();
        let storage_error = self.storage_error.lock().unwrap().clone();
        Snapshot {
            isolated: self.isolated,
            data,
            connection,
            job,
            traffic,
            core,
            logs,
            storage_error,
        }
    }
    fn change(&self, f: impl FnOnce(&mut Data) -> Result<(), String>) -> Result<(), String> {
        if self.storage_error.lock().unwrap().is_some() {
            return Err("当前配置读取失败，请先在设置中恢复备份，避免覆盖原文件".into());
        }
        let mut data = self.data.lock().unwrap();
        let mut next = data.clone();
        f(&mut next)?;
        storage::save(&self.root, &next)?;
        *data = next;
        Ok(())
    }
    pub fn restore_backup(&self) -> Result<(), String> {
        let _op = self.operation.lock().unwrap();
        if self.runtime.lock().unwrap().processes.is_some() || self.job.lock().unwrap().running {
            return Err("请先断开连接并等待后台任务结束".into());
        }
        let bytes =
            fs::read(self.root.join("profile.backup.json")).map_err(|_| "没有可恢复的备份")?;
        let d: Data = serde_json::from_slice(&bytes).map_err(|_| "备份文件损坏")?;
        if d.version != 1 {
            return Err("备份版本不支持".into());
        }
        config::validate(&d.settings)?;
        let original = self.root.join("profile.json");
        if original.exists() {
            fs::copy(
                &original,
                self.root.join(format!(
                    "profile.damaged-{}.json",
                    chrono::Utc::now().timestamp()
                )),
            )
            .map_err(|e| e.to_string())?;
        }
        storage::atomic_write(&original, &bytes)?;
        *self.data.lock().unwrap() = d;
        *self.storage_error.lock().unwrap() = None;
        Ok(())
    }
    pub fn save_settings(&self, s: Settings) -> Result<(), String> {
        config::validate(&s)?;
        let _op = self.operation.lock().unwrap();
        if self.runtime.lock().unwrap().processes.is_some() {
            return Err("请先断开连接再修改网络设置".into());
        }
        let old = self.data.lock().unwrap().settings.clone();
        if s.start_on_boot != old.start_on_boot && !self.isolated {
            windows::autostart(s.start_on_boot)?;
        }
        let result = self.change(|d| {
            d.settings = s;
            Ok(())
        });
        if result.is_err() && !self.isolated {
            let _ = windows::autostart(old.start_on_boot);
        }
        result
    }
    pub fn import(&self, payload: &str) -> Result<Value, String> {
        let (nodes, rejected) = parser::parse(payload)?;
        let mut added = 0;
        self.change(|d| {
            for n in nodes {
                if !d.nodes.iter().any(|old| old.id == n.id) {
                    d.nodes.push(n);
                    added += 1;
                }
            }
            Ok(())
        })?;
        self.log(
            "INFO",
            "subscription",
            &format!("导入 {added} 个节点，跳过 {rejected} 个无效条目"),
        );
        Ok(json!({"added":added,"rejected":rejected}))
    }
    pub fn select(&self, id: &str) -> Result<(), String> {
        self.change(|d| {
            if !d.nodes.iter().any(|n| n.id == id) {
                return Err("节点不存在".into());
            }
            d.active_node_id = Some(id.into());
            Ok(())
        })
    }
    pub fn rename_node(&self, id: &str, name: &str) -> Result<(), String> {
        if name.trim().is_empty() || name.chars().count() > 100 {
            return Err("名称应为 1–100 字".into());
        }
        self.change(|d| {
            let n = d
                .nodes
                .iter_mut()
                .find(|n| n.id == id)
                .ok_or("节点不存在")?;
            n.name = name.trim().into();
            Ok(())
        })
    }
    pub fn delete_node(&self, id: &str) -> Result<(), String> {
        if self.runtime.lock().unwrap().connection.node_id.as_deref() == Some(id) {
            return Err("请先断开该节点".into());
        }
        self.change(|d| {
            d.nodes.retain(|n| n.id != id);
            if d.active_node_id.as_deref() == Some(id) {
                d.active_node_id = None;
            }
            Ok(())
        })
    }
    pub fn set_node_pin(&self, id: &str, fingerprint: &str) -> Result<(), String> {
        let pin = fingerprint.trim().replace(':', "").to_lowercase();
        if !pin.is_empty() && (pin.len() != 64 || !pin.chars().all(|c| c.is_ascii_hexdigit())) {
            return Err("请输入 64 位十六进制 SHA-256 证书指纹，可含冒号".into());
        }
        self.change(|d| {
            let n = d
                .nodes
                .iter_mut()
                .find(|n| n.id == id)
                .ok_or("节点不存在")?;
            if cores::needs_singbox(n) {
                return Err("此项证书指纹用于 Xray TLS 节点".into());
            }
            n.cert_sha256 = pin;
            if !n.cert_sha256.is_empty() {
                n.allow_insecure = false;
            }
            Ok(())
        })
    }
    pub fn save_subscription(&self, mut sub: Subscription) -> Result<(), String> {
        {
            let job = self.job.lock().unwrap();
            if job.running && job.kind == "subscription" {
                return Err("请等待订阅更新结束后再编辑".into());
            }
        }
        config::validate_http(&sub.url)?;
        if sub.name.trim().is_empty() || sub.name.len() > 200 || sub.interval_hours > 720 {
            return Err("订阅名称或更新间隔无效".into());
        }
        if sub.id.is_empty() {
            sub.id = id(&format!("{}{}", sub.url, now()));
        }
        self.change(|d| {
            if let Some(old) = d.subscriptions.iter_mut().find(|s| s.id == sub.id) {
                if sub.url == old.url {
                    sub.last_updated = old.last_updated.clone();
                    sub.last_attempt = old.last_attempt.clone();
                    sub.error = old.error.clone();
                } else {
                    sub.last_updated = None;
                    sub.last_attempt = None;
                    sub.error = None;
                }
                *old = sub;
            } else {
                d.subscriptions.push(sub);
            }
            Ok(())
        })
    }
    pub fn delete_subscription(&self, id: &str) -> Result<(), String> {
        if self.job.lock().unwrap().running {
            return Err("请等待当前任务结束".into());
        }
        let current = self.runtime.lock().unwrap().connection.node_id.clone();
        self.change(|d| {
            if d.nodes.iter().any(|n| {
                n.subscription_id.as_deref() == Some(id) && current.as_deref() == Some(&n.id)
            }) {
                return Err("请先断开该订阅节点".into());
            }
            d.subscriptions.retain(|s| s.id != id);
            d.nodes.retain(|n| n.subscription_id.as_deref() != Some(id));
            if !d
                .nodes
                .iter()
                .any(|n| Some(&n.id) == d.active_node_id.as_ref())
            {
                d.active_node_id = None;
            }
            Ok(())
        })
    }
    pub fn start_job(
        self: &Arc<Self>,
        kind: &str,
        total: usize,
        work: impl FnOnce(Arc<Self>) -> Result<String, String> + Send + 'static,
    ) -> Result<(), String> {
        let mut j = self.job.lock().unwrap();
        if j.running {
            return Err("已有后台任务，请等待或取消".into());
        }
        *j = Job {
            running: true,
            kind: kind.into(),
            total,
            completed: 0,
            message: "正在处理…".into(),
        };
        self.cancel.store(false, Ordering::SeqCst);
        let s = self.clone();
        drop(j);
        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(s.clone())))
                .unwrap_or_else(|_| Err("后台任务异常中断，已清理核心进程".into()));
            let mut job = s.job.lock().unwrap();
            job.running = false;
            job.message = match result {
                Ok(m) => m,
                Err(e) => {
                    s.log("ERROR", "task", &e);
                    redact(&e)
                }
            };
        });
        Ok(())
    }
    pub fn cancelled(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::SeqCst) || self.exit.load(Ordering::SeqCst) {
            Err("任务已取消".into())
        } else {
            Ok(())
        }
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
    fn client(timeout: u64) -> Result<reqwest::blocking::Client, String> {
        reqwest::blocking::Client::builder()
            .no_proxy()
            .user_agent(concat!("MyRayLite/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(timeout))
            .build()
            .map_err(|e| e.to_string())
    }
    pub fn fetch_limited(&self, url: &str, limit: usize) -> Result<Vec<u8>, String> {
        self.cancelled()?;
        let mut response = Self::client(90)?
            .get(url)
            .send()
            .map_err(|e| format!("网络请求失败：{}", redact(&e.to_string())))?
            .error_for_status()
            .map_err(|e| {
                format!(
                    "服务器返回错误：{}",
                    e.status().map(|s| s.as_u16()).unwrap_or(0)
                )
            })?;
        if response.content_length().unwrap_or(0) > limit as u64 {
            return Err("下载内容超过大小限制".into());
        }
        let mut result = Vec::new();
        let mut buf = [0; 65536];
        loop {
            self.cancelled()?;
            let n = response.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            if result.len() + n > limit {
                return Err("下载内容超过大小限制".into());
            }
            result.extend_from_slice(&buf[..n]);
        }
        Ok(result)
    }
    pub fn update_subscription(&self, id: &str) -> Result<String, String> {
        let sub = self
            .data
            .lock()
            .unwrap()
            .subscriptions
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or("订阅不存在")?;
        self.change(|d| {
            if let Some(s) = d.subscriptions.iter_mut().find(|s| s.id == id) {
                s.last_attempt = Some(now());
            }
            Ok(())
        })?;
        let result: Result<String, String> = (|| {
            let bytes = self.fetch_limited(&sub.url, 8 * 1024 * 1024)?;
            let (nodes, skipped) =
                parser::parse(&String::from_utf8(bytes).map_err(|_| "订阅不是有效 UTF-8")?)?;
            self.cancelled()?;
            let count = nodes.len();
            self.change(|d| {
                let active_was_from_subscription = d
                    .active_node_id
                    .as_ref()
                    .and_then(|active_id| d.nodes.iter().find(|node| &node.id == active_id))
                    .map(|node| node.subscription_id.as_deref() == Some(id))
                    .unwrap_or(false);
                let old: Vec<_> = d
                    .nodes
                    .iter()
                    .filter(|n| n.subscription_id.as_deref() == Some(id))
                    .cloned()
                    .collect();
                d.nodes.retain(|n| n.subscription_id.as_deref() != Some(id));
                for mut n in nodes {
                    n.id = crate::models::id(&format!("{id}:{}", n.raw));
                    n.subscription_id = Some(id.into());
                    if let Some(prev) = old.iter().find(|o| o.id == n.id) {
                        n.delay_ms = prev.delay_ms;
                        n.download_mbps = prev.download_mbps;
                        n.last_tested = prev.last_tested.clone();
                        n.last_error = prev.last_error.clone();
                        n.last_test_mode = prev.last_test_mode.clone();
                        if !prev.cert_sha256.is_empty() {
                            n.cert_sha256 = prev.cert_sha256.clone();
                            n.allow_insecure = false;
                        }
                    }
                    d.nodes.push(n);
                }
                if !d
                    .nodes
                    .iter()
                    .any(|n| Some(&n.id) == d.active_node_id.as_ref())
                {
                    d.active_node_id = if active_was_from_subscription {
                        d.nodes
                            .iter()
                            .find(|node| node.subscription_id.as_deref() == Some(id))
                            .map(|node| node.id.clone())
                    } else {
                        None
                    };
                }
                if let Some(s) = d.subscriptions.iter_mut().find(|s| s.id == id) {
                    s.last_updated = Some(now());
                    s.error = None;
                }
                Ok(())
            })?;
            Ok(format!("已更新 {count} 个节点；跳过 {skipped} 个无效条目"))
        })();
        if let Err(e) = &result {
            let _ = self.change(|d| {
                if let Some(s) = d.subscriptions.iter_mut().find(|s| s.id == id) {
                    s.error = Some(redact(e));
                }
                Ok(())
            });
        }
        result
    }
    pub fn update_subscriptions(&self, ids: Vec<String>) -> Result<String, String> {
        if ids.is_empty() {
            return Ok("没有可更新的订阅".into());
        }
        {
            let mut job = self.job.lock().unwrap();
            job.total = ids.len();
            job.completed = 0;
            job.message = format!("正在更新订阅 0 / {}", ids.len());
        }
        let mut succeeded = 0usize;
        let mut failed = 0usize;
        let mut first_error = None;
        for (index, id) in ids.iter().enumerate() {
            self.cancelled()?;
            match self.update_subscription(id) {
                Ok(_) => succeeded += 1,
                Err(error) => {
                    failed += 1;
                    if first_error.is_none() {
                        first_error = Some(error.clone());
                    }
                    self.log("ERROR", "subscription", &error);
                }
            }
            let mut job = self.job.lock().unwrap();
            job.completed = index + 1;
            job.message = format!("正在更新订阅 {} / {}", job.completed, job.total);
        }
        if succeeded == 0 && failed > 0 {
            return Err(format!(
                "全部 {failed} 个订阅更新失败：{}",
                first_error.unwrap_or_else(|| "未知错误".into())
            ));
        }
        Ok(format!("订阅更新完成：{succeeded} 个成功，{failed} 个失败"))
    }
    pub fn connect(self: &Arc<Self>, id: Option<String>) -> Result<String, String> {
        let _op = self.operation.lock().unwrap();
        self.cancelled()?;
        self.stop_inner()?;
        let data = self.data.lock().unwrap().clone();
        let selected = id.or(data.active_node_id.clone());
        let node = if data.settings.proxy_mode == "direct" {
            Node::default()
        } else {
            data.nodes
                .iter()
                .find(|n| Some(&n.id) == selected.as_ref())
                .cloned()
                .ok_or("请先选择节点")?
        };
        if self.isolated && data.settings.system_proxy {
            return Err("隔离测试模式禁止修改系统代理".into());
        }
        self.runtime.lock().unwrap().connection = Connection {
            status: "connecting".into(),
            node_id: selected.clone(),
            ..Connection::default()
        };
        let result: Result<(), String> = (|| {
            let guards = vec![
                std::net::TcpListener::bind(("127.0.0.1", data.settings.http_port))
                    .map_err(|_| "HTTP 端口已占用")?,
                std::net::TcpListener::bind(("127.0.0.1", data.settings.socks_port))
                    .map_err(|_| "SOCKS 端口已占用")?,
            ];
            let udp_guard = std::net::UdpSocket::bind(("127.0.0.1", data.settings.socks_port))
                .map_err(|_| "SOCKS UDP 端口不可用（已占用或受系统限制），请更换端口")?;
            drop(guards);
            drop(udp_guard);
            let mut set = ProcessSet::new(&self.root)?;
            let mut helper = None;
            if cores::needs_singbox(&node) && data.settings.proxy_mode != "direct" {
                let p = cores::unused_port()?;
                let cfg = cores::singbox_config(&node, p)?;
                let service = self.clone();
                set.start(&self.root, "singbox", &cfg, p, &self.child_job, move |l| {
                    service.log("INFO", "singbox", &l)
                })?;
                helper = Some(p);
            }
            let cfg = cores::config_for(&node, &data.settings, helper)?;
            let service = self.clone();
            set.start(
                &self.root,
                "xray",
                &cfg,
                data.settings.http_port,
                &self.child_job,
                move |l| service.log("INFO", "xray", &l),
            )?;
            self.cancelled()?;
            if let Some(id) = selected.as_ref() {
                self.select(id)?;
            }
            if data.settings.system_proxy {
                windows::enable(
                    &self.root,
                    data.settings.http_port,
                    data.settings.socks_port,
                )?;
            }
            let mut runtime = self.runtime.lock().unwrap();
            runtime.processes = Some(set);
            runtime.connection = Connection {
                status: "connected".into(),
                node_id: selected.clone(),
                since: Some(now()),
                system_proxy: data.settings.system_proxy,
                error: None,
            };
            Ok(())
        })();
        if let Err(e) = result {
            if !self.isolated {
                let _ = windows::restore(&self.root);
            }
            self.runtime.lock().unwrap().connection = Connection {
                status: "disconnected".into(),
                error: Some(redact(&e)),
                ..Connection::default()
            };
            return Err(e);
        }
        self.log(
            "INFO",
            "connection",
            "核心已启动，本地代理监听就绪；远端连通性请通过 HTTP 测速确认",
        );
        Ok("本地代理已启动".into())
    }
    fn stop_inner(&self) -> Result<(), String> {
        let mut runtime = self.runtime.lock().unwrap();
        let had = runtime.processes.take().is_some();
        drop(runtime);
        let restore = if self.isolated {
            Ok(false)
        } else {
            windows::restore(&self.root)
        };
        let mut runtime = self.runtime.lock().unwrap();
        runtime.connection = Connection {
            status: "disconnected".into(),
            error: restore.as_ref().err().cloned(),
            ..Connection::default()
        };
        if had {
            self.log("INFO", "connection", "核心进程已停止");
        }
        restore.map(|_| ())
    }
    pub fn disconnect(&self) -> Result<(), String> {
        self.cancel();
        let _op = self.operation.lock().unwrap();
        self.stop_inner()
    }
    pub fn shutdown(&self) {
        self.exit.store(true, Ordering::SeqCst);
        self.cancel();
        let _op = self.operation.lock().unwrap();
        if let Err(e) = self.stop_inner() {
            self.log("ERROR", "system", &e);
        }
        let d = self.data.lock().unwrap();
        if self.storage_error.lock().unwrap().is_none() {
            let _ = storage::save(&self.root, &d);
        }
    }
    pub fn refresh_core(&self) {
        let version = |kind| {
            cores::output(
                &cores::exe(&self.root, kind),
                &["version"],
                Duration::from_secs(5),
            )
            .ok()
            .and_then(|s| s.lines().next().map(str::to_string))
            .unwrap_or_else(|| "未安装或不可运行".into())
        };
        *self.core.lock().unwrap() = CoreInfo {
            installed: cores::exe(&self.root, "xray").exists(),
            version: version("xray"),
            geo_ready: self.root.join("cores/xray/geoip.dat").exists()
                && self.root.join("cores/xray/geosite.dat").exists(),
            singbox_installed: cores::exe(&self.root, "singbox").exists(),
            singbox_version: version("singbox"),
        };
    }
}

impl Service {
    fn test_node(
        self: &Arc<Self>,
        node: &Node,
        mode: &str,
        s: &Settings,
    ) -> Result<(u64, Option<f64>), String> {
        self.cancelled()?;
        if mode == "tcp" {
            use std::net::ToSocketAddrs;
            let addresses = (node.address.as_str(), node.port)
                .to_socket_addrs()
                .map_err(|_| "节点域名解析失败")?;
            let start = Instant::now();
            for address in addresses {
                self.cancelled()?;
                if std::net::TcpStream::connect_timeout(
                    &address,
                    Duration::from_secs(s.test_timeout),
                )
                .is_ok()
                {
                    return Ok((start.elapsed().as_millis() as u64, None));
                }
            }
            return Err("TCP 连接失败或超时".into());
        }
        let port = cores::unused_port()?;
        let mut set = ProcessSet::new(&self.root)?;
        let mut target = node.clone();
        if cores::needs_singbox(node) {
            let p = cores::unused_port()?;
            let cfg = cores::singbox_config(node, p)?;
            let service = self.clone();
            set.start(&self.root, "singbox", &cfg, p, &self.child_job, move |l| {
                service.log("INFO", "probe", &l)
            })?;
            target = cores::bridge(p);
        }
        let cfg = config::probe(&target, port)?;
        let service = self.clone();
        set.start(&self.root, "xray", &cfg, port, &self.child_job, move |l| {
            service.log("INFO", "probe", &l)
        })?;
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .proxy(
                reqwest::Proxy::all(format!("http://127.0.0.1:{port}"))
                    .map_err(|e| e.to_string())?,
            )
            .timeout(Duration::from_secs(s.test_timeout))
            .build()
            .map_err(|e| e.to_string())?;
        let start = Instant::now();
        let mut response = client
            .get(&s.test_url)
            .send()
            .map_err(|_| "HTTP 请求失败或超时")?
            .error_for_status()
            .map_err(|e| {
                format!(
                    "测速地址返回非成功状态：{}",
                    e.status().map(|s| s.as_u16()).unwrap_or(0)
                )
            })?;
        let latency = start.elapsed().as_millis() as u64;
        if mode == "http" {
            return Ok((latency, None));
        }
        let mut received = 0;
        let mut buf = [0; 16384];
        while received < s.download_bytes {
            self.cancelled()?;
            let limit = buf.len().min(s.download_bytes - received);
            let n = response
                .read(&mut buf[..limit])
                .map_err(|_| "下载读取失败或超时")?;
            if n == 0 {
                break;
            }
            received += n;
        }
        if received < 1024 {
            return Err("下载测速响应不足 1 KiB，请设置可下载文件的测速 URL".into());
        }
        Ok((
            latency,
            Some(received as f64 / start.elapsed().as_secs_f64().max(0.001) / 1048576.0),
        ))
    }
    pub fn test_nodes(self: &Arc<Self>, ids: Vec<String>, mode: String) -> Result<String, String> {
        if !["tcp", "http", "download"].contains(&mode.as_str()) {
            return Err("测速模式无效".into());
        }
        let _op = self.operation.lock().unwrap();
        let data = self.data.lock().unwrap().clone();
        let nodes: Vec<_> = data
            .nodes
            .into_iter()
            .filter(|n| ids.is_empty() || ids.contains(&n.id))
            .collect();
        if nodes.is_empty() {
            return Err("没有待测试节点".into());
        }
        self.job.lock().unwrap().total = nodes.len();
        let mut succeeded = 0;
        for batch in nodes.chunks(data.settings.test_concurrency) {
            self.cancelled()?;
            let results = thread::scope(|scope| {
                let handles: Vec<_> = batch
                    .iter()
                    .map(|node| {
                        let service = self.clone();
                        let settings = &data.settings;
                        let mode = &mode;
                        scope.spawn(move || {
                            let mut result = Err("未测试".into());
                            for _ in 0..=settings.test_retries {
                                if service.cancelled().is_err() {
                                    break;
                                }
                                result = service.test_node(node, mode, settings);
                                if result.is_ok() {
                                    break;
                                }
                            }
                            (node.id.clone(), result)
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| {
                        h.join()
                            .unwrap_or_else(|_| (String::new(), Err("测速任务异常".into())))
                    })
                    .collect::<Vec<_>>()
            });
            self.cancelled()?;
            for (id, result) in results {
                if result.is_ok() {
                    succeeded += 1;
                }
                self.change(|d| {
                    if let Some(n) = d.nodes.iter_mut().find(|n| n.id == id) {
                        n.last_tested = Some(now());
                        n.last_test_mode = mode.clone();
                        match result {
                            Ok((ms, speed)) => {
                                n.delay_ms = Some(ms);
                                n.download_mbps = speed;
                                n.last_error = None;
                            }
                            Err(e) => {
                                n.delay_ms = None;
                                n.download_mbps = None;
                                n.last_error = Some(redact(&e));
                            }
                        }
                    }
                    Ok(())
                })?;
                let mut j = self.job.lock().unwrap();
                j.completed += 1;
                j.message = format!("已测试 {} / {}", j.completed, j.total);
            }
        }
        Ok(format!("测速完成：{succeeded} / {} 可用", nodes.len()))
    }
    pub fn check_release(&self, kind: &str) -> Result<Value, String> {
        let repo = match kind {
            "xray" => "XTLS/Xray-core".to_string(),
            "singbox" => "SagerNet/sing-box".to_string(),
            "app" => self.data.lock().unwrap().settings.update_repo.clone(),
            _ => return Err("更新类型无效".into()),
        };
        if repo.is_empty() {
            return Err("请先配置新版 GitHub 发布源（owner/repo）；不会使用旧 WPF 更新源".into());
        }
        let body = self.fetch_limited(
            &format!("https://api.github.com/repos/{repo}/releases/latest"),
            4 * 1024 * 1024,
        )?;
        serde_json::from_slice(&body).map_err(|_| "发布信息无效".into())
    }
    pub fn update_core(&self, kind: &str, geo_only: bool) -> Result<String, String> {
        let _op = self.operation.lock().unwrap();
        if self.runtime.lock().unwrap().processes.is_some() {
            return Err("请先断开连接再更新核心或规则数据".into());
        }
        let release = self.check_release(kind)?;
        let assets = release["assets"].as_array().ok_or("缺少发布文件")?;
        let asset = assets
            .iter()
            .find(|a| {
                let name = a["name"].as_str().unwrap_or_default();
                if kind == "xray" {
                    name == "Xray-windows-64.zip"
                } else {
                    name.contains("windows-amd64") && name.ends_with(".zip")
                }
            })
            .ok_or("未找到 Windows x64 核心压缩包")?;
        let url = asset["browser_download_url"]
            .as_str()
            .ok_or("下载地址缺失")?;
        if !url.starts_with("https://github.com/") {
            return Err("核心下载地址不属于官方 GitHub 发布".into());
        }
        let bytes = self.fetch_limited(url, 150 * 1024 * 1024)?;
        use sha2::{Digest, Sha256};
        let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
        if asset["digest"].as_str() != Some(&digest) {
            return Err("核心发布缺少可信 SHA256 或校验不匹配，保留当前核心".into());
        }
        let stage = tempfile::tempdir_in(self.root.join("cores")).map_err(|e| e.to_string())?;
        let current = self.root.join("cores").join(kind);
        if geo_only && current.exists() {
            for f in fs::read_dir(&current).map_err(|e| e.to_string())?.flatten() {
                if f.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    fs::copy(f.path(), stage.path().join(f.file_name()))
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| "核心压缩包损坏")?;
        for i in 0..archive.len() {
            self.cancelled()?;
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            let name = std::path::Path::new(entry.name())
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let allowed = if kind == "xray" {
                if geo_only {
                    vec!["geoip.dat", "geosite.dat"]
                } else {
                    vec!["xray.exe", "geoip.dat", "geosite.dat", "LICENSE"]
                }
            } else {
                vec!["sing-box.exe", "libcronet.dll", "LICENSE"]
            };
            if allowed.contains(&name.as_str()) {
                if entry.size() > 120 * 1024 * 1024 {
                    return Err("核心解压文件过大".into());
                }
                let mut file =
                    fs::File::create(stage.path().join(name)).map_err(|e| e.to_string())?;
                std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
            }
        }
        let executable = stage.path().join(if kind == "xray" {
            "xray.exe"
        } else {
            "sing-box.exe"
        });
        cores::output(&executable, &["version"], Duration::from_secs(10))?;
        if kind == "xray"
            && (!stage.path().join("geoip.dat").exists()
                || !stage.path().join("geosite.dat").exists())
        {
            return Err("压缩包缺少 geoip/geosite，未替换当前版本".into());
        }
        self.cancelled()?;
        let backup = self.root.join("cores").join(format!("{kind}.previous"));
        if backup.exists() {
            fs::remove_dir_all(&backup).map_err(|e| e.to_string())?;
        }
        if current.exists() {
            fs::rename(&current, &backup).map_err(|e| e.to_string())?;
        }
        let staged = stage.keep();
        if let Err(e) = fs::rename(&staged, &current) {
            let _ = fs::rename(&backup, &current);
            return Err(e.to_string());
        }
        self.refresh_core();
        Ok(format!(
            "{} 已更新至 {}",
            if geo_only { "规则数据" } else { kind },
            release["tag_name"].as_str().unwrap_or("最新版本")
        ))
    }
    pub fn diagnostics(&self) -> Result<String, String> {
        let data = self.data.lock().unwrap().clone();
        let mut checks = vec![];
        for kind in ["xray", "singbox"] {
            checks.push(
                json!({"item":format!("{kind} 核心"),"ok":cores::exe(&self.root,kind).exists()}),
            );
        }
        let connected = self.runtime.lock().unwrap().processes.is_some();
        for (name, port) in [
            ("HTTP", data.settings.http_port),
            ("SOCKS", data.settings.socks_port),
        ] {
            checks.push(json!({"item":format!("{name} 端口 {port}"),"ok":connected||std::net::TcpListener::bind(("127.0.0.1",port)).is_ok()}));
        }
        checks.push(json!({"item":"规则数据","ok":self.core.lock().unwrap().geo_ready}));
        checks.push(json!({"item":"已选择节点","ok":data.active_node_id.is_some()||data.settings.proxy_mode=="direct"}));
        let failures = checks.iter().filter(|c| c["ok"] == false).count();
        for c in checks {
            self.log(
                if c["ok"] == true { "INFO" } else { "WARN" },
                "diagnostic",
                &format!(
                    "{}：{}",
                    c["item"].as_str().unwrap_or("检查"),
                    if c["ok"] == true {
                        "正常"
                    } else {
                        "需要检查"
                    }
                ),
            );
        }
        Ok(format!("诊断完成，{failures} 项需要检查；详见日志"))
    }
    pub fn export_diagnostics(&self) -> Result<String, String> {
        let data = self.data.lock().unwrap().clone();
        let dir = self.root.join("diagnostics");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!(
            "myray-diagnostics-{}.zip",
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        ));
        let file = fs::File::create(&path).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        let summary = json!({"version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"nodeCount":data.nodes.len(),"subscriptionCount":data.subscriptions.len(),"proxyMode":data.settings.proxy_mode,"httpPort":data.settings.http_port,"socksPort":data.settings.socks_port,"core":self.core.lock().unwrap().clone(),"note":"不包含节点原文、订阅地址、账户、密码、UUID 或生成的核心配置"});
        zip.start_file("summary.json", options)
            .map_err(|e| e.to_string())?;
        zip.write_all(&serde_json::to_vec_pretty(&summary).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        // Diagnostic exports include app events only, not raw core output which can contain server identities.
        let logs:Vec<_>=self.logs.lock().unwrap().iter().filter(|l|!["xray","singbox","probe"].contains(&l.source.as_str())).map(|l|json!({"timestamp":l.timestamp,"level":l.level,"source":l.source,"message":if l.level=="ERROR"{"操作失败（详细错误仅保留在本机日志）"}else{&l.message}})).collect();
        zip.start_file("events.json", options)
            .map_err(|e| e.to_string())?;
        zip.write_all(&serde_json::to_vec_pretty(&logs).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        zip.finish().map_err(|e| e.to_string())?;
        Ok(path.display().to_string())
    }
    pub fn clear_logs(&self) -> Result<(), String> {
        let mut logs = self.logs.lock().unwrap();
        logs.clear();
        storage::atomic_write(&self.root.join("app.log"), b"")
    }
    pub fn background(self: &Arc<Self>) {
        let s = self.clone();
        thread::spawn(move || {
            let mut networks = sysinfo::Networks::new_with_refreshed_list();
            let mut previous = Instant::now();
            let mut save_ticks = 0;
            while !s.exit.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_secs(1));
                let crashed = {
                    let mut r = s.runtime.lock().unwrap();
                    r.processes
                        .as_mut()
                        .map(|p| {
                            p.children
                                .iter_mut()
                                .any(|c| matches!(c.try_wait(), Ok(Some(_)) | Err(_)))
                        })
                        .unwrap_or(false)
                };
                if crashed {
                    let _op = s.operation.lock().unwrap();
                    let _ = s.stop_inner();
                    s.runtime.lock().unwrap().connection.error =
                        Some("核心异常退出，已停止代理并尝试恢复系统设置".into());
                    s.log(
                        "ERROR",
                        "connection",
                        "核心异常退出，已执行清理与系统代理恢复",
                    );
                }
                networks.refresh(true);
                let elapsed = previous.elapsed().as_secs_f64().max(0.01);
                previous = Instant::now();
                let (upload, download) = networks
                    .iter()
                    .filter(|(name, _)| !name.to_lowercase().contains("loopback"))
                    .fold((0u64, 0u64), |(u, d), (_, n)| {
                        (
                            u.saturating_add(n.transmitted()),
                            d.saturating_add(n.received()),
                        )
                    });
                let connected = s.runtime.lock().unwrap().processes.is_some();
                {
                    let mut data = s.data.lock().unwrap();
                    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
                    if data.traffic_date != date {
                        data.traffic_date = date;
                        data.today_upload = 0;
                        data.today_download = 0;
                    }
                    if connected {
                        data.today_upload = data.today_upload.saturating_add(upload);
                        data.today_download = data.today_download.saturating_add(download);
                    }
                    *s.traffic.lock().unwrap() = Traffic {
                        upload_speed: if connected {
                            (upload as f64 / elapsed) as u64
                        } else {
                            0
                        },
                        download_speed: if connected {
                            (download as f64 / elapsed) as u64
                        } else {
                            0
                        },
                        today_upload: data.today_upload,
                        today_download: data.today_download,
                    };
                    save_ticks += 1;
                    if save_ticks >= 60 && connected && s.storage_error.lock().unwrap().is_none() {
                        let _ = storage::save(&s.root, &data);
                        save_ticks = 0;
                    }
                }
                if !s.job.lock().unwrap().running {
                    let due = s
                        .data
                        .lock()
                        .unwrap()
                        .subscriptions
                        .iter()
                        .find(|sub| subscription_due(sub, chrono::Utc::now()))
                        .map(|sub| sub.id.clone());
                    if let Some(id) = due {
                        let _ = s.start_job("subscription", 1, move |service| {
                            service.update_subscription(&id)
                        });
                    }
                }
            }
        });
    }
}
fn subscription_due(s: &Subscription, now: chrono::DateTime<chrono::Utc>) -> bool {
    if s.interval_hours == 0 || s.interval_hours > 720 {
        return false;
    }
    let parsed = |v: &Option<String>| {
        v.as_ref()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&chrono::Utc))
    };
    if parsed(&s.last_attempt)
        .map(|t| now - t < chrono::Duration::minutes(15))
        .unwrap_or(false)
    {
        return false;
    }
    parsed(&s.last_updated)
        .map(|t| now - t >= chrono::Duration::hours(s.interval_hours as i64))
        .unwrap_or(true)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schedule_backoff() {
        let mut s = Subscription {
            interval_hours: 6,
            ..Subscription::default()
        };
        assert!(subscription_due(&s, chrono::Utc::now()));
        s.last_attempt = Some(now());
        assert!(!subscription_due(&s, chrono::Utc::now()));
        s.interval_hours = 0;
        assert!(!subscription_due(&s, chrono::Utc::now()));
    }
    #[test]
    fn redact_secrets() {
        let text = redact("https://host/private-token vless://secret@host:443 192.168.1.2");
        assert!(!text.contains("private-token"));
        assert!(!text.contains("secret"));
        assert!(!text.contains("192.168"));
    }
}

#[cfg(test)]
#[path = "service_integration.rs"]
mod integration;
