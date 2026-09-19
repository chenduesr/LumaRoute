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
#[derive(Default)]
struct SubscriptionInfo {
    provided: bool,
    upload_bytes: Option<u64>,
    download_bytes: Option<u64>,
    total_bytes: Option<u64>,
    expires_at: Option<String>,
}
pub enum UiEvent {
    Snapshot(Box<Snapshot>),
    Log(LogEntry),
}

fn subscription_info(value: Option<&reqwest::header::HeaderValue>) -> SubscriptionInfo {
    let Some(value) = value.and_then(|value| value.to_str().ok()) else {
        return SubscriptionInfo::default();
    };
    let mut info = SubscriptionInfo {
        provided: true,
        ..SubscriptionInfo::default()
    };
    for item in value.split(';') {
        let Some((key, value)) = item.trim().split_once('=') else {
            continue;
        };
        let number = value.trim().parse::<u64>().ok();
        match key.trim().to_ascii_lowercase().as_str() {
            "upload" => info.upload_bytes = number,
            "download" => info.download_bytes = number,
            "total" => info.total_bytes = number,
            "expire" => {
                info.expires_at = number.and_then(|timestamp| {
                    if timestamp == 0 {
                        return None;
                    }
                    let seconds = if timestamp > 10_000_000_000 {
                        timestamp / 1000
                    } else {
                        timestamp
                    };
                    chrono::DateTime::<chrono::Utc>::from_timestamp(seconds as i64, 0)
                        .map(|value| value.to_rfc3339())
                });
            }
            _ => {}
        }
    }
    info
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
    diagnostics: Mutex<Option<DiagnosticReport>>,
    last_auto_restart: Mutex<Option<Instant>>,
    storage_error: Mutex<Option<String>>,
    event_tx: Mutex<Option<std::sync::mpsc::Sender<UiEvent>>>,
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
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            let source = resources.join(kind);
            if source.exists() {
                for f in fs::read_dir(source).map_err(|e| e.to_string())?.flatten() {
                    if f.file_type().map(|t| t.is_file()).unwrap_or(false) {
                        let destination = target.join(f.file_name());
                        if !destination.exists() {
                            fs::copy(f.path(), destination).map_err(|e| e.to_string())?;
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
            diagnostics: Mutex::new(None),
            last_auto_restart: Mutex::new(None),
            storage_error: Mutex::new(error),
            event_tx: Mutex::new(None),
            child_job: windows::ChildJob::new()?,
            isolated,
        });
        service.refresh_core();
        if !isolated {
            let start_on_boot = service.data.lock().unwrap().settings.start_on_boot;
            if let Err(error) = windows::autostart(start_on_boot) {
                service.log("ERROR", "system", &format!("登录启动状态同步失败：{error}"));
            }
            match windows::restore(&service.root) {
                Ok(true) => service.log("WARN", "system", "已恢复上次异常退出遗留的系统代理"),
                Err(e) => service.log("ERROR", "system", &e),
                _ => {}
            }
        }
        service.log("INFO", "app", "LumaRoute 原生业务服务已启动");
        Ok(service)
    }
    pub fn attach_events(&self, event_tx: std::sync::mpsc::Sender<UiEvent>) {
        *self.event_tx.lock().unwrap() = Some(event_tx);
        self.emit_snapshot();
    }
    fn emit_snapshot(&self) {
        let sender = self.event_tx.lock().unwrap().clone();
        if let Some(sender) = sender {
            let _ = sender.send(UiEvent::Snapshot(Box::new(self.snapshot())));
        }
    }
    fn emit_log(&self, entry: &LogEntry) {
        let sender = self.event_tx.lock().unwrap().clone();
        if let Some(sender) = sender {
            let _ = sender.send(UiEvent::Log(entry.clone()));
        }
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
        drop(logs);
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
        self.emit_log(&entry);
    }
    pub fn snapshot(&self) -> Snapshot {
        let data = self.data.lock().unwrap().clone();
        let connection = self.runtime.lock().unwrap().connection.clone();
        let job = self.job.lock().unwrap().clone();
        let traffic = self.traffic.lock().unwrap().clone();
        let core = self.core.lock().unwrap().clone();
        let logs = self.logs.lock().unwrap().iter().cloned().collect();
        let diagnostics = self.diagnostics.lock().unwrap().clone();
        let storage_error = self.storage_error.lock().unwrap().clone();
        Snapshot {
            version: env!("CARGO_PKG_VERSION").into(),
            isolated: self.isolated,
            data,
            connection,
            job,
            traffic,
            core,
            logs,
            diagnostics,
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
        drop(data);
        self.emit_snapshot();
        Ok(())
    }
    pub fn restore_backup(&self) -> Result<(), String> {
        let _op = self.operation.lock().unwrap();
        if self.runtime.lock().unwrap().processes.is_some() || self.job.lock().unwrap().running {
            return Err("请先断开连接并等待后台任务结束".into());
        }
        let bytes =
            fs::read(self.root.join("profile.backup.json")).map_err(|_| "没有可恢复的备份")?;
        let mut d = storage::decode(&bytes).map_err(|_| "备份文件损坏")?;
        if d.version == 1 {
            d.settings.capture_mode = if d.settings.legacy_system_proxy.unwrap_or(true) {
                "systemProxy"
            } else {
                "none"
            }
            .into();
            d.settings.legacy_system_proxy = None;
            d.version = 2;
        }
        if d.version != 2 {
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
        storage::save(&self.root, &d)?;
        *self.data.lock().unwrap() = d;
        *self.storage_error.lock().unwrap() = None;
        self.emit_snapshot();
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
    pub fn set_capture_mode(&self, mode: &str) -> Result<(), String> {
        if !["none", "systemProxy", "tun"].contains(&mode) {
            return Err("流量接管方式无效".into());
        }
        let _op = self.operation.lock().unwrap();
        if self.runtime.lock().unwrap().processes.is_some() || self.job.lock().unwrap().running {
            return Err("请先断开连接并等待当前任务结束".into());
        }
        self.change(|data| {
            data.settings.capture_mode = mode.into();
            Ok(())
        })
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
            d.recent_node_ids.retain(|node_id| node_id != id);
            d.recent_node_ids.insert(0, id.into());
            d.recent_node_ids.truncate(5);
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
            d.recent_node_ids.retain(|node_id| node_id != id);
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
    pub fn save_subscription(&self, mut sub: Subscription) -> Result<String, String> {
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
        if self
            .data
            .lock()
            .unwrap()
            .subscriptions
            .iter()
            .any(|existing| existing.id != sub.id && existing.url.trim() == sub.url.trim())
        {
            return Err("该订阅地址已经存在，请直接更新现有订阅".into());
        }
        if sub.id.is_empty() {
            sub.id = id(&format!("{}{}", sub.url, now()));
        }
        let subscription_id = sub.id.clone();
        self.change(|d| {
            if let Some(old) = d.subscriptions.iter_mut().find(|s| s.id == sub.id) {
                if sub.url == old.url {
                    sub.last_updated = old.last_updated.clone();
                    sub.last_attempt = old.last_attempt.clone();
                    sub.error = old.error.clone();
                    sub.format = old.format.clone();
                    sub.upload_bytes = old.upload_bytes;
                    sub.download_bytes = old.download_bytes;
                    sub.total_bytes = old.total_bytes;
                    sub.expires_at = old.expires_at.clone();
                } else {
                    sub.last_updated = None;
                    sub.last_attempt = None;
                    sub.error = None;
                    sub.format.clear();
                    sub.upload_bytes = None;
                    sub.download_bytes = None;
                    sub.total_bytes = None;
                    sub.expires_at = None;
                }
                *old = sub;
            } else {
                d.subscriptions.push(sub);
            }
            Ok(())
        })?;
        Ok(subscription_id)
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
            d.recent_node_ids
                .retain(|node_id| d.nodes.iter().any(|node| &node.id == node_id));
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
        self.emit_snapshot();
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
            drop(job);
            s.emit_snapshot();
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
        self.emit_snapshot();
    }
    fn client(timeout: u64) -> Result<reqwest::blocking::Client, String> {
        reqwest::blocking::Client::builder()
            .no_proxy()
            .user_agent(concat!("LumaRoute/", env!("CARGO_PKG_VERSION")))
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
    fn fetch_subscription(
        &self,
        url: &str,
        limit: usize,
    ) -> Result<(Vec<u8>, SubscriptionInfo), String> {
        self.cancelled()?;
        let mut response = Self::client(90)?
            .get(url)
            .header(
                reqwest::header::ACCEPT,
                "application/yaml, application/json, text/yaml, text/plain, */*",
            )
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
        let usage = subscription_info(
            response
                .headers()
                .get("subscription-userinfo")
                .or_else(|| response.headers().get("subscription-user-info")),
        );
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
        Ok((result, usage))
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
            let (bytes, usage) = self.fetch_subscription(&sub.url, 8 * 1024 * 1024)?;
            let parsed = parser::parse_detailed(
                &String::from_utf8(bytes).map_err(|_| "订阅不是有效 UTF-8")?,
            )?;
            self.cancelled()?;
            let count = parsed.nodes.len();
            let skipped = parsed.rejected;
            let source_format = parsed.format;
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
                for mut n in parsed.nodes {
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
                    d.active_node_id = if active_was_from_subscription || d.active_node_id.is_none()
                    {
                        d.nodes
                            .iter()
                            .find(|node| node.subscription_id.as_deref() == Some(id))
                            .map(|node| node.id.clone())
                    } else {
                        None
                    };
                }
                d.recent_node_ids
                    .retain(|node_id| d.nodes.iter().any(|node| &node.id == node_id));
                if let Some(s) = d.subscriptions.iter_mut().find(|s| s.id == id) {
                    s.last_updated = Some(now());
                    s.error = None;
                    s.format = source_format;
                    if usage.provided {
                        s.upload_bytes = usage.upload_bytes;
                        s.download_bytes = usage.download_bytes;
                        s.total_bytes = usage.total_bytes;
                        s.expires_at = usage.expires_at;
                    }
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
        self.emit_snapshot();
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
            drop(job);
            self.emit_snapshot();
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
        self.connect_internal(id, None)
    }

    fn connect_internal(
        self: &Arc<Self>,
        id: Option<String>,
        recovery_reason: Option<String>,
    ) -> Result<String, String> {
        let _op = self.operation.lock().unwrap();
        self.cancelled()?;
        self.stop_inner()?;
        if recovery_reason.is_none() {
            *self.last_auto_restart.lock().unwrap() = None;
        }
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
        if self.isolated && data.settings.capture_mode != "none" {
            return Err("隔离测试模式禁止修改系统代理或创建 TUN 接口".into());
        }
        if data.settings.capture_mode == "tun" && !windows::is_elevated() {
            return Err("TUN 模式需要管理员权限，请重新点击连接并同意 Windows 提权提示".into());
        }
        self.runtime.lock().unwrap().connection = Connection {
            status: "starting".into(),
            node_id: selected.clone(),
            capture_mode: data.settings.capture_mode.clone(),
            recovery_reason: recovery_reason.clone(),
            ..Connection::default()
        };
        self.emit_snapshot();
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
            {
                let mut runtime = self.runtime.lock().unwrap();
                runtime.connection.status = "localReady".into();
                runtime.connection.since = Some(now());
                runtime.connection.recovery_reason = recovery_reason.clone();
            }
            self.emit_snapshot();
            if let Some(id) = selected.as_ref() {
                self.select(id)?;
            }
            if data.settings.capture_mode == "systemProxy" {
                if let Err(error) = windows::enable(
                    &self.root,
                    data.settings.http_port,
                    data.settings.socks_port,
                ) {
                    self.runtime.lock().unwrap().connection.status = "proxyFailed".into();
                    self.emit_snapshot();
                    return Err(format!("系统代理应用失败：{error}"));
                }
            }
            {
                let mut runtime = self.runtime.lock().unwrap();
                runtime.processes = Some(set);
                runtime.connection.capture_mode = data.settings.capture_mode.clone();
                runtime.connection.status = "verifying".into();
            }
            self.emit_snapshot();
            Ok(())
        })();
        if let Err(e) = result {
            if !self.isolated {
                let _ = windows::restore(&self.root);
            }
            let status = if self.runtime.lock().unwrap().connection.status == "proxyFailed" {
                "proxyFailed"
            } else {
                "disconnected"
            };
            self.runtime.lock().unwrap().connection = Connection {
                status: status.into(),
                node_id: selected,
                capture_mode: data.settings.capture_mode.clone(),
                error: Some(redact(&e)),
                recovery_reason,
                ..Connection::default()
            };
            self.emit_snapshot();
            return Err(e);
        }
        match self.verify_active_connection(recovery_reason.as_deref(), true) {
            Ok(()) => {
                self.log("INFO", "connection", "本地代理与远端网络验证通过");
                Ok(if recovery_reason.is_some() {
                    "当前节点已恢复连接".into()
                } else {
                    "连接已建立并验证".into()
                })
            }
            Err(error) => {
                self.log(
                    "WARN",
                    "connection",
                    &format!("本地代理已启动，但远端验证失败：{error}"),
                );
                Ok("本地代理已启动，远端网络暂不可用；将继续检查当前节点".into())
            }
        }
    }

    fn proxy_http_check(&self, settings: &Settings, timeout: Duration) -> Result<(), String> {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .proxy(
                reqwest::Proxy::all(format!("http://127.0.0.1:{}", settings.http_port))
                    .map_err(|e| e.to_string())?,
            )
            .connect_timeout(timeout)
            .timeout(timeout)
            .build()
            .map_err(|e| e.to_string())?;
        client
            .get(&settings.test_url)
            .send()
            .map_err(|_| "代理 HTTP 请求失败或超时".to_string())?
            .error_for_status()
            .map_err(|e| {
                format!(
                    "代理测速地址返回状态 {}",
                    e.status().map(|status| status.as_u16()).unwrap_or(0)
                )
            })?;
        Ok(())
    }

    fn verify_active_connection(
        &self,
        reason: Option<&str>,
        show_progress: bool,
    ) -> Result<(), String> {
        let settings = self.data.lock().unwrap().settings.clone();
        {
            let mut runtime = self.runtime.lock().unwrap();
            if runtime.processes.is_none() {
                return Err("核心进程未运行".into());
            }
            if show_progress {
                runtime.connection.status = "verifying".into();
                runtime.connection.recovery_reason = reason.map(str::to_string);
            }
        }
        if show_progress {
            self.emit_snapshot();
        }
        let timeout = Duration::from_secs(settings.test_timeout.clamp(3, 10));
        let capture_state = match settings.capture_mode.as_str() {
            "systemProxy" if !self.isolated => {
                windows::proxy_matches(settings.http_port, settings.socks_port)
                    .map_err(|error| format!("无法读取 Windows 系统代理：{error}"))
                    .and_then(|matches| {
                        matches
                            .then_some(())
                            .ok_or_else(|| "Windows 系统代理已被修改或未正确启用".to_string())
                    })
            }
            "tun" if !self.isolated => {
                let mut ready = false;
                for _ in 0..12 {
                    self.cancelled()?;
                    if windows::tun_ready(settings.tun_ipv6)? {
                        ready = true;
                        break;
                    }
                    thread::sleep(Duration::from_millis(250));
                }
                ready
                    .then_some(())
                    .ok_or_else(|| "TUN 网络接口或默认路由尚未就绪".to_string())
            }
            _ => Ok(()),
        };
        let capture_failed = capture_state.is_err();
        let result = capture_state.and_then(|_| self.proxy_http_check(&settings, timeout));
        if capture_failed {
            let _ = self.stop_inner();
        }
        let mut runtime = self.runtime.lock().unwrap();
        if result.is_ok() {
            runtime.connection.status = "connected".into();
            runtime.connection.error = None;
            runtime.connection.last_verified = Some(now());
            runtime.connection.recovery_reason = reason.map(str::to_string);
        } else {
            runtime.connection.status = if capture_failed {
                "proxyFailed"
            } else {
                "networkUnavailable"
            }
            .into();
            runtime.connection.error = Some(if capture_failed {
                format!(
                    "流量接管状态异常：{}；已停止核心并尝试恢复系统设置",
                    result
                        .as_ref()
                        .err()
                        .map(String::as_str)
                        .unwrap_or("未知错误")
                )
            } else {
                format!(
                    "远端网络验证失败：{}；本地代理仍在运行",
                    result
                        .as_ref()
                        .err()
                        .map(String::as_str)
                        .unwrap_or("未知错误")
                )
            });
            runtime.connection.recovery_reason = reason.map(str::to_string);
        }
        drop(runtime);
        self.emit_snapshot();
        result
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
        drop(runtime);
        self.emit_snapshot();
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
        self.emit_snapshot();
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
                drop(j);
                self.emit_snapshot();
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
        let _op = self.operation.lock().unwrap();
        let started_at = now();
        let data = self.data.lock().unwrap().clone();
        {
            let mut job = self.job.lock().unwrap();
            job.total = 11;
            job.completed = 0;
        }
        self.emit_snapshot();
        let mut checks: Vec<DiagnosticCheck> = vec![];
        let mut add = |check: DiagnosticCheck| {
            self.log(
                match check.status.as_str() {
                    "ok" | "skipped" => "INFO",
                    "warning" => "WARN",
                    _ => "ERROR",
                },
                "diagnostic",
                &format!("{}：{}（{}）", check.label, check.status, check.detail),
            );
            checks.push(check);
            let mut job = self.job.lock().unwrap();
            job.completed = checks.len();
            job.message = format!("正在诊断 {} / {}", job.completed, job.total);
            drop(job);
            self.emit_snapshot();
        };
        let core = self.core.lock().unwrap().clone();
        let cores_ok = core.installed && core.singbox_installed;
        add(DiagnosticCheck {
            key: "cores".into(),
            label: "核心文件与版本".into(),
            status: if cores_ok { "ok" } else { "failed" }.into(),
            detail: format!("Xray：{}；sing-box：{}", core.version, core.singbox_version),
            suggestion: (!cores_ok).then(|| "请在设置 → 更新中重新安装缺失的核心".into()),
        });
        let connected = self.runtime.lock().unwrap().processes.is_some();
        let port_check = |port| {
            if connected {
                std::net::TcpStream::connect_timeout(
                    &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                    Duration::from_secs(2),
                )
                .is_ok()
            } else {
                std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
            }
        };
        let http_ok = port_check(data.settings.http_port);
        let socks_ok = port_check(data.settings.socks_port);
        add(DiagnosticCheck {
            key: "localPorts".into(),
            label: "本地代理端口".into(),
            status: if http_ok && socks_ok { "ok" } else { "failed" }.into(),
            detail: format!(
                "HTTP {}：{}；SOCKS {}：{}",
                data.settings.http_port,
                if http_ok { "正常" } else { "异常" },
                data.settings.socks_port,
                if socks_ok { "正常" } else { "异常" }
            ),
            suggestion: (!(http_ok && socks_ok))
                .then(|| "请检查端口占用，或在设置中更换 HTTP/SOCKS 端口".into()),
        });

        let selected = data
            .active_node_id
            .as_ref()
            .and_then(|id| data.nodes.iter().find(|node| &node.id == id));
        let udp_only = selected
            .map(|node| matches!(node.protocol.as_str(), "tuic" | "hysteria2"))
            .unwrap_or(false);
        let tcp_result = if data.settings.proxy_mode == "direct" || udp_only {
            None
        } else {
            selected.map(|node| {
                use std::net::ToSocketAddrs;
                (node.address.as_str(), node.port)
                    .to_socket_addrs()
                    .map_err(|_| "节点地址无法解析".to_string())?
                    .any(|address| {
                        std::net::TcpStream::connect_timeout(&address, Duration::from_secs(5))
                            .is_ok()
                    })
                    .then_some(())
                    .ok_or_else(|| "节点 TCP 连接失败或超时".to_string())
            })
        };
        add(match tcp_result {
            None if data.settings.proxy_mode == "direct" => DiagnosticCheck {
                key: "nodeTcp".into(),
                label: "节点 TCP 可达性".into(),
                status: "skipped".into(),
                detail: "直连模式无需检查节点".into(),
                suggestion: None,
            },
            None if udp_only => DiagnosticCheck {
                key: "nodeTcp".into(),
                label: "节点 TCP 可达性".into(),
                status: "skipped".into(),
                detail: "当前节点使用 UDP 传输，由代理 HTTP 请求继续验证".into(),
                suggestion: None,
            },
            None => DiagnosticCheck {
                key: "nodeTcp".into(),
                label: "节点 TCP 可达性".into(),
                status: "failed".into(),
                detail: "尚未选择节点".into(),
                suggestion: Some("请先选择一个节点".into()),
            },
            Some(Ok(())) => DiagnosticCheck {
                key: "nodeTcp".into(),
                label: "节点 TCP 可达性".into(),
                status: "ok".into(),
                detail: "节点地址和端口可以建立 TCP 连接".into(),
                suggestion: None,
            },
            Some(Err(error)) => DiagnosticCheck {
                key: "nodeTcp".into(),
                label: "节点 TCP 可达性".into(),
                status: "failed".into(),
                detail: error,
                suggestion: Some("请检查本机网络、节点地址、端口或服务器状态".into()),
            },
        });

        let proxy_result = if connected {
            self.proxy_http_check(
                &data.settings,
                Duration::from_secs(data.settings.test_timeout.clamp(3, 10)),
            )
        } else {
            Err("当前未连接，无法通过本地代理发起请求".into())
        };
        add(DiagnosticCheck {
            key: "proxyHttp".into(),
            label: "代理 HTTP 请求".into(),
            status: if proxy_result.is_ok() { "ok" } else { "failed" }.into(),
            detail: proxy_result
                .as_ref()
                .map(|_| format!("{} 请求成功", data.settings.test_url))
                .unwrap_or_else(|error| error.clone()),
            suggestion: proxy_result
                .is_err()
                .then(|| "请先连接，再检查节点协议、TLS 指纹和测速地址".into()),
        });

        use std::net::ToSocketAddrs;
        let dns_target = url::Url::parse(&data.settings.test_url).ok();
        let dns_host = dns_target
            .as_ref()
            .and_then(|url| url.host_str().map(str::to_string));
        let dns_port = dns_target
            .as_ref()
            .and_then(url::Url::port_or_known_default)
            .unwrap_or(443);
        let resolved = dns_host
            .as_ref()
            .ok_or_else(|| "测速地址缺少域名".to_string())
            .and_then(|host| {
                (host.as_str(), dns_port)
                    .to_socket_addrs()
                    .map(|values| {
                        let mut values = values.collect::<Vec<_>>();
                        values.sort();
                        values.dedup();
                        values
                    })
                    .map_err(|_| "测速地址域名解析失败".to_string())
            });
        let (ipv4_addresses, ipv6_addresses) = resolved
            .as_ref()
            .map(|addresses| {
                (
                    addresses
                        .iter()
                        .filter(|address| address.is_ipv4())
                        .copied()
                        .collect::<Vec<_>>(),
                    addresses
                        .iter()
                        .filter(|address| address.is_ipv6())
                        .copied()
                        .collect::<Vec<_>>(),
                )
            })
            .unwrap_or_default();
        add(DiagnosticCheck {
            key: "dns".into(),
            label: "DNS 解析结果".into(),
            status: if resolved.is_ok() { "ok" } else { "failed" }.into(),
            detail: resolved
                .as_ref()
                .map(|_| {
                    format!(
                        "{}：IPv4 {} 个，IPv6 {} 个",
                        dns_host.as_deref().unwrap_or("测速地址"),
                        ipv4_addresses.len(),
                        ipv6_addresses.len()
                    )
                })
                .unwrap_or_else(|error| error.clone()),
            suggestion: resolved
                .is_err()
                .then(|| "请检查 Windows DNS、自定义 DNS 地址或测速 URL".into()),
        });

        let foreign_dns = if !data.settings.dot_dns.trim().is_empty() {
            config::list(&data.settings.dot_dns)
        } else if !data.settings.doh_dns.trim().is_empty() {
            config::list(&data.settings.doh_dns)
        } else {
            config::list(&data.settings.foreign_dns)
        };
        add(DiagnosticCheck {
            key: "dnsPath".into(),
            label: "DNS 解析路径".into(),
            status: if data.settings.custom_dns {
                "ok"
            } else {
                "warning"
            }
            .into(),
            detail: if data.settings.custom_dns {
                format!(
                    "由 Xray 核心处理；{}；境外解析器 {} 个；当前查询策略优先 IPv4",
                    if data.settings.split_dns {
                        "国内/境外分流"
                    } else {
                        "统一解析"
                    },
                    foreign_dns.len()
                )
            } else {
                "未启用核心自定义 DNS，域名解析依赖 Windows 与核心默认路径".into()
            },
            suggestion: (!data.settings.custom_dns)
                .then(|| "如需明确控制解析路径，可在设置 → DNS 中启用自定义 DNS".into()),
        });

        let encrypted_foreign_dns = !foreign_dns.is_empty()
            && foreign_dns
                .iter()
                .all(|server| server.starts_with("https://") || server.starts_with("tls://"));
        let leak_check = if data.settings.proxy_mode == "direct" {
            (
                "skipped",
                "直连模式不进行代理 DNS 泄漏判断".to_string(),
                None,
            )
        } else if !data.settings.custom_dns {
            (
                "warning",
                "代理模式仍依赖系统 DNS，域名查询可能由本地网络解析".to_string(),
                Some("建议启用自定义 DNS，并为境外查询配置 DoH 或 DoT".into()),
            )
        } else if !encrypted_foreign_dns {
            (
                "warning",
                "境外 DNS 未全部使用 DoH/DoT；配置检查无法确认查询是否暴露给本地网络".to_string(),
                Some("建议将境外 DNS 配置为 HTTPS DoH 或 tls:// DoT 地址".into()),
            )
        } else if data.settings.split_dns {
            (
                "warning",
                "境外查询使用加密 DNS；国内域名按设置分流到国内解析器，这是有意的分流路径"
                    .to_string(),
                Some("如需所有查询走同一路径，请关闭 DNS 分流".into()),
            )
        } else {
            (
                "ok",
                "境外解析器均使用 DoH/DoT，未发现明显的系统 DNS 泄漏配置风险".to_string(),
                None,
            )
        };
        add(DiagnosticCheck {
            key: "dnsLeak".into(),
            label: "DNS 泄漏风险（配置检查）".into(),
            status: leak_check.0.into(),
            detail: leak_check.1,
            suggestion: leak_check.2,
        });

        let networks = sysinfo::Networks::new_with_refreshed_list();
        let has_global_ipv6 = networks.iter().any(|(name, network)| {
            !name.to_lowercase().contains("loopback")
                && network
                    .ip_networks()
                    .iter()
                    .any(|network| is_global_ipv6(network.addr))
        });
        let connect_family = |addresses: &[std::net::SocketAddr]| {
            addresses.iter().take(2).any(|address| {
                std::net::TcpStream::connect_timeout(address, Duration::from_millis(1500)).is_ok()
            })
        };
        let (ipv4_reachable, ipv6_reachable) = if self.isolated {
            (None, None)
        } else {
            (
                (!ipv4_addresses.is_empty()).then(|| connect_family(&ipv4_addresses)),
                (has_global_ipv6 && !ipv6_addresses.is_empty())
                    .then(|| connect_family(&ipv6_addresses)),
            )
        };
        let ipv6_check = if self.isolated {
            ("skipped", "隔离测试模式不探测公网 IPv6".to_string(), None)
        } else if !has_global_ipv6 {
            (
                "warning",
                "未检测到可路由的全局 IPv6 地址；当前网络将使用 IPv4".to_string(),
                Some("如果运营商不提供 IPv6，可忽略；否则检查网卡、路由器和系统 IPv6".into()),
            )
        } else if ipv6_addresses.is_empty() {
            (
                "skipped",
                "系统具有 IPv6，但测速域名没有返回 AAAA 记录".to_string(),
                None,
            )
        } else if ipv6_reachable == Some(true) {
            (
                "ok",
                "检测到全局 IPv6，并可通过 IPv6 连接测速目标".to_string(),
                None,
            )
        } else {
            (
                "warning",
                "系统具有 IPv6 地址，但 IPv6 连接失败或超时".to_string(),
                Some("可能存在无效 IPv6 默认路由；可修复 IPv6 网络或暂时使用 IPv4".into()),
            )
        };
        add(DiagnosticCheck {
            key: "ipv6".into(),
            label: "IPv6 可用性".into(),
            status: ipv6_check.0.into(),
            detail: ipv6_check.1,
            suggestion: ipv6_check.2,
        });

        let dual_stack = if self.isolated {
            ("skipped", "隔离测试模式不探测公网双栈".to_string(), None)
        } else if ipv4_addresses.is_empty() || ipv6_addresses.is_empty() {
            (
                "skipped",
                "测速域名没有同时提供 A 与 AAAA 记录，无法比较双栈".to_string(),
                None,
            )
        } else {
            match (ipv4_reachable, ipv6_reachable) {
                (Some(true), Some(true)) => (
                    "ok",
                    "IPv4 与 IPv6 均可连接，双栈工作正常".to_string(),
                    None,
                ),
                (Some(true), Some(false) | None) => (
                    "warning",
                    "IPv4 可用但 IPv6 不可用，系统需要回退到 IPv4".to_string(),
                    Some("若访问出现首连延迟，请修复无效 IPv6 路由或调整系统地址优先级".into()),
                ),
                (Some(false), Some(true)) => (
                    "warning",
                    "IPv6 可用但 IPv4 不可用，部分仅 IPv4 服务可能失败".to_string(),
                    Some("请检查 IPv4 网关、DNS A 记录和本机防火墙".into()),
                ),
                _ => (
                    "failed",
                    "IPv4 与 IPv6 均无法连接测速目标".to_string(),
                    Some("请先确认本机网络可用，再检查防火墙和测速地址".into()),
                ),
            }
        };
        add(DiagnosticCheck {
            key: "dualStack".into(),
            label: "双栈连接".into(),
            status: dual_stack.0.into(),
            detail: dual_stack.1,
            suggestion: dual_stack.2,
        });

        let capture_state = if data.settings.capture_mode == "none" {
            None
        } else if !connected {
            Some(Ok(false))
        } else {
            match data.settings.capture_mode.as_str() {
                "systemProxy" => Some(windows::proxy_matches(
                    data.settings.http_port,
                    data.settings.socks_port,
                )),
                "tun" => Some(windows::tun_ready(data.settings.tun_ipv6)),
                "none" => None,
                _ => Some(Err("未知流量接管方式".into())),
            }
        };
        add(match capture_state {
            None => DiagnosticCheck {
                key: "captureMode".into(),
                label: "流量接管".into(),
                status: "skipped".into(),
                detail: "仅开放本地 HTTP / SOCKS 端口".into(),
                suggestion: None,
            },
            Some(Ok(true)) => DiagnosticCheck {
                key: "captureMode".into(),
                label: "流量接管".into(),
                status: "ok".into(),
                detail: if data.settings.capture_mode == "tun" {
                    "TUN 网络接口与默认路由已就绪".into()
                } else {
                    "Windows 系统代理与 LumaRoute 当前端口一致".into()
                },
                suggestion: None,
            },
            Some(Ok(false)) => DiagnosticCheck {
                key: "captureMode".into(),
                label: "流量接管".into(),
                status: "failed".into(),
                detail: if data.settings.capture_mode == "tun" {
                    "TUN 网络接口或默认路由未就绪".into()
                } else {
                    "系统代理未启用或已被其他程序修改".into()
                },
                suggestion: Some("请断开后重新连接；TUN 模式还需同意管理员权限提示".into()),
            },
            Some(Err(error)) => DiagnosticCheck {
                key: "captureMode".into(),
                label: "流量接管".into(),
                status: "failed".into(),
                detail: redact(&error),
                suggestion: Some("请检查系统代理设置或 TUN 网络接口权限".into()),
            },
        });

        let read_ip = |proxy: Option<u16>| -> Result<String, String> {
            let mut builder = reqwest::blocking::Client::builder()
                .no_proxy()
                .connect_timeout(Duration::from_secs(6))
                .timeout(Duration::from_secs(8));
            if let Some(port) = proxy {
                builder = builder.proxy(
                    reqwest::Proxy::all(format!("http://127.0.0.1:{port}"))
                        .map_err(|e| e.to_string())?,
                );
            }
            builder
                .build()
                .map_err(|e| e.to_string())?
                .get("https://api.ipify.org")
                .send()
                .map_err(|_| "出口 IP 服务不可达".to_string())?
                .error_for_status()
                .map_err(|_| "出口 IP 服务返回错误".to_string())?
                .text()
                .map(|value| value.trim().to_string())
                .map_err(|_| "出口 IP 响应无效".to_string())
        };
        let exit_check = if self.isolated {
            ("skipped", "隔离测试模式不访问公网出口服务", None)
        } else if connected && data.settings.proxy_mode != "direct" {
            match (read_ip(None), read_ip(Some(data.settings.http_port))) {
                (Ok(direct), Ok(proxied)) if direct != proxied => {
                    ("ok", "直连与代理出口 IP 不同", None)
                }
                (Ok(_), Ok(_)) => (
                    "warning",
                    "直连与代理出口 IP 相同",
                    Some("节点可能未改变出口，或上游网络使用了相同出口".into()),
                ),
                _ => (
                    "warning",
                    "无法同时取得直连和代理出口 IP",
                    Some("出口查询服务可能被网络拦截；不影响其他诊断结果".into()),
                ),
            }
        } else {
            ("skipped", "需要处于代理连接状态", None)
        };
        add(DiagnosticCheck {
            key: "exitIp".into(),
            label: "实际出口 IP".into(),
            status: exit_check.0.into(),
            detail: exit_check.1.into(),
            suggestion: exit_check.2,
        });

        let failures = checks
            .iter()
            .filter(|check| check.status == "failed")
            .count();
        let warnings = checks
            .iter()
            .filter(|check| check.status == "warning")
            .count();
        let summary = if failures == 0 && warnings == 0 {
            "诊断完成，所有已执行项目正常".to_string()
        } else {
            format!("诊断完成：{failures} 项失败，{warnings} 项提醒")
        };
        *self.diagnostics.lock().unwrap() = Some(DiagnosticReport {
            started_at,
            completed_at: now(),
            checks,
            summary: summary.clone(),
        });
        Ok(summary)
    }
    pub fn export_diagnostics(&self) -> Result<String, String> {
        let data = self.data.lock().unwrap().clone();
        let dir = self.root.join("diagnostics");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!(
            "lumaroute-diagnostics-{}.zip",
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
        if let Some(report) = self.diagnostics.lock().unwrap().clone() {
            zip.start_file("diagnostics.json", options)
                .map_err(|e| e.to_string())?;
            zip.write_all(&serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        }
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
            let mut network_signature = active_network_signature(&networks);
            let mut previous_tick = Instant::now();
            let mut last_health_check = Instant::now();
            let mut failed_health_checks = 0u8;
            let mut pending_restart: Option<String> = None;
            let mut save_ticks = 0;
            while !s.exit.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_secs(1));
                let tick_gap = previous_tick.elapsed();
                previous_tick = Instant::now();
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
                    {
                        let mut runtime = s.runtime.lock().unwrap();
                        runtime.connection.status = "coreCrashed".into();
                        runtime.connection.error =
                            Some("核心异常退出，已停止代理并恢复系统设置".into());
                        runtime.connection.recovery_reason = Some("核心异常退出".into());
                    }
                    s.emit_snapshot();
                    s.log(
                        "ERROR",
                        "connection",
                        "核心异常退出，已执行清理与系统代理恢复",
                    );
                    if s.data.lock().unwrap().settings.auto_recover_connection {
                        pending_restart = Some("核心异常退出".into());
                    }
                }
                networks.refresh(true);
                let next_signature = active_network_signature(&networks);
                let network_changed = !network_signature.is_empty()
                    && !next_signature.is_empty()
                    && network_signature != next_signature;
                network_signature = next_signature;
                let resumed = tick_gap > Duration::from_secs(5);
                let elapsed = tick_gap.as_secs_f64().clamp(0.01, 5.0);
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
                if connected {
                    s.emit_snapshot();
                }
                let status = s.runtime.lock().unwrap().connection.status.clone();
                let active = s.runtime.lock().unwrap().processes.is_some();
                let health_due = last_health_check.elapsed()
                    >= if status == "networkUnavailable" {
                        Duration::from_secs(10)
                    } else {
                        Duration::from_secs(30)
                    };
                let recovery_reason = if resumed {
                    Some("电脑从休眠中恢复")
                } else if network_changed {
                    Some("网络接口发生变化")
                } else if health_due {
                    Some("定期连接检查")
                } else {
                    None
                };
                if active && !s.job.lock().unwrap().running {
                    if let Some(reason) = recovery_reason {
                        if let Ok(_operation) = s.operation.try_lock() {
                            last_health_check = Instant::now();
                            // Periodic recovery checks run silently. They only
                            // publish when the resulting connection state is
                            // known, so the UI does not flicker to `verifying`.
                            match s.verify_active_connection(Some(reason), false) {
                                Ok(()) => {
                                    if resumed || network_changed || failed_health_checks > 0 {
                                        s.log(
                                            "INFO",
                                            "recovery",
                                            &format!("{reason}后，当前节点连接验证通过"),
                                        );
                                    }
                                    failed_health_checks = 0;
                                }
                                Err(error) => {
                                    failed_health_checks = failed_health_checks.saturating_add(1);
                                    s.log(
                                        "WARN",
                                        "recovery",
                                        &format!("{reason}后连接验证失败：{error}"),
                                    );
                                    if failed_health_checks >= 2
                                        && s.data.lock().unwrap().settings.auto_recover_connection
                                    {
                                        pending_restart = Some(reason.into());
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(reason) = pending_restart.clone() {
                    if s.job.lock().unwrap().running {
                        continue;
                    }
                    let can_restart = {
                        let mut last = s.last_auto_restart.lock().unwrap();
                        let allowed = last
                            .map(|time| time.elapsed() >= Duration::from_secs(300))
                            .unwrap_or(true);
                        if allowed {
                            *last = Some(Instant::now());
                        }
                        allowed
                    };
                    if can_restart {
                        pending_restart = None;
                        failed_health_checks = 0;
                        let selected = s
                            .runtime
                            .lock()
                            .unwrap()
                            .connection
                            .node_id
                            .clone()
                            .or_else(|| s.data.lock().unwrap().active_node_id.clone());
                        s.log(
                            "WARN",
                            "recovery",
                            &format!("{reason}，将自动重启一次当前节点"),
                        );
                        let reason_for_job = reason.clone();
                        let _ = s.start_job("recovery", 1, move |service| {
                            service.connect_internal(selected, Some(reason_for_job))
                        });
                    } else if !can_restart {
                        pending_restart = None;
                        s.log(
                            "WARN",
                            "recovery",
                            "五分钟内已经自动重启过一次，不再重复重启；可手动断开后重连",
                        );
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

fn active_network_signature(networks: &sysinfo::Networks) -> String {
    let mut entries = networks
        .iter()
        .filter(|(name, _)| !name.to_lowercase().contains("loopback"))
        .flat_map(|(name, network)| {
            network
                .ip_networks()
                .iter()
                .filter(|network| !network.addr.is_loopback() && !network.addr.is_unspecified())
                .map(move |network| format!("{name}:{network}"))
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries.join("|")
}

fn is_global_ipv6(address: std::net::IpAddr) -> bool {
    let std::net::IpAddr::V6(address) = address else {
        return false;
    };
    let octets = address.octets();
    !address.is_loopback()
        && !address.is_unspecified()
        && !address.is_multicast()
        && !(octets[0] == 0xfe && octets[1] & 0xc0 == 0x80)
        && octets[0] & 0xfe != 0xfc
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
    #[test]
    fn parses_subscription_usage_header() {
        let header = reqwest::header::HeaderValue::from_static(
            "upload=1024; download=2048; total=10485760; expire=1893456000",
        );
        let info = subscription_info(Some(&header));
        assert!(info.provided);
        assert_eq!(info.upload_bytes, Some(1024));
        assert_eq!(info.download_bytes, Some(2048));
        assert_eq!(info.total_bytes, Some(10485760));
        assert!(info
            .expires_at
            .as_deref()
            .unwrap()
            .starts_with("2030-01-01"));
    }
}

#[cfg(test)]
#[path = "service_integration.rs"]
mod integration;
