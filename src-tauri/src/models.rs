use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub http_port: u16,
    pub socks_port: u16,
    pub proxy_mode: String,
    pub routing_mode: String,
    pub system_proxy: bool,
    pub bypass_mainland: bool,
    pub start_on_boot: bool,
    pub auto_connect: bool,
    pub update_subscriptions_on_launch: bool,
    pub minimize_to_tray: bool,
    pub direct_domains: String,
    pub direct_ips: String,
    pub proxy_domains: String,
    pub proxy_ips: String,
    pub block_domains: String,
    pub block_ips: String,
    pub custom_dns: bool,
    pub fake_dns: bool,
    pub split_dns: bool,
    pub domestic_dns: String,
    pub foreign_dns: String,
    pub doh_dns: String,
    pub dot_dns: String,
    pub test_url: String,
    pub test_timeout: u64,
    pub test_concurrency: usize,
    pub test_retries: usize,
    pub download_bytes: usize,
    pub update_repo: String,
    pub auto_check_updates: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            http_port: 7890,
            socks_port: 7891,
            proxy_mode: "rule".into(),
            routing_mode: "smart".into(),
            system_proxy: true,
            bypass_mainland: true,
            start_on_boot: false,
            auto_connect: false,
            update_subscriptions_on_launch: true,
            minimize_to_tray: true,
            direct_domains: String::new(),
            direct_ips: String::new(),
            proxy_domains: String::new(),
            proxy_ips: String::new(),
            block_domains: String::new(),
            block_ips: String::new(),
            custom_dns: false,
            fake_dns: false,
            split_dns: true,
            domestic_dns: "223.5.5.5".into(),
            foreign_dns: "1.1.1.1".into(),
            doh_dns: "https://dns.google/dns-query".into(),
            dot_dns: String::new(),
            test_url: "https://www.gstatic.com/generate_204".into(),
            test_timeout: 10,
            test_concurrency: 8,
            test_retries: 1,
            download_bytes: 1048576,
            update_repo: "chenduesr/LumaRoute".into(),
            auto_check_updates: false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub protocol: String,
    pub raw: String,
    pub subscription_id: Option<String>,
    pub user_id: String,
    pub password: String,
    pub method: String,
    pub network: String,
    pub security: String,
    pub flow: String,
    pub host: String,
    pub path: String,
    pub service_name: String,
    pub sni: String,
    pub fingerprint: String,
    pub public_key: String,
    pub short_id: String,
    pub spider_x: String,
    pub alpn: String,
    pub obfs: String,
    pub obfs_password: String,
    pub allow_insecure: bool,
    pub cert_sha256: String,
    pub alter_id: u32,
    pub vmess_security: String,
    pub unsupported_reason: Option<String>,
    pub delay_ms: Option<u64>,
    pub download_mbps: Option<f64>,
    pub last_error: Option<String>,
    pub last_tested: Option<String>,
    pub last_test_mode: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Subscription {
    pub id: String,
    pub name: String,
    pub url: String,
    pub interval_hours: u32,
    pub last_updated: Option<String>,
    pub last_attempt: Option<String>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Data {
    pub version: u32,
    pub settings: Settings,
    pub nodes: Vec<Node>,
    pub subscriptions: Vec<Subscription>,
    pub active_node_id: Option<String>,
    pub recent_node_ids: Vec<String>,
    pub traffic_date: String,
    pub today_upload: u64,
    pub today_download: u64,
}
impl Default for Data {
    fn default() -> Self {
        Self {
            version: 1,
            settings: Settings::default(),
            nodes: vec![],
            subscriptions: vec![],
            active_node_id: None,
            recent_node_ids: vec![],
            traffic_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
            today_upload: 0,
            today_download: 0,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub source: String,
    pub message: String,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub running: bool,
    pub kind: String,
    pub completed: usize,
    pub total: usize,
    pub message: String,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub status: String,
    pub node_id: Option<String>,
    pub since: Option<String>,
    pub system_proxy: bool,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Traffic {
    pub upload_speed: u64,
    pub download_speed: u64,
    pub today_upload: u64,
    pub today_download: u64,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreInfo {
    pub version: String,
    pub installed: bool,
    pub geo_ready: bool,
    pub singbox_version: String,
    pub singbox_installed: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub isolated: bool,
    pub data: Data,
    pub connection: Connection,
    pub job: Job,
    pub traffic: Traffic,
    pub core: CoreInfo,
    pub logs: Vec<LogEntry>,
    pub storage_error: Option<String>,
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
pub fn id(value: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(value.as_bytes()))[..24].to_string()
}
