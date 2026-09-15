use crate::models::{id, Node};
use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
    Engine,
};
use percent_encoding::percent_decode_str;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use url::Url;

fn decode(s: &str) -> String {
    percent_decode_str(s).decode_utf8_lossy().into_owned()
}
fn b64(s: &str) -> Result<String, String> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    [STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|e| e.decode(&s).ok().and_then(|b| String::from_utf8(b).ok()))
        .ok_or_else(|| "Base64 编码无效".into())
}
fn field(v: &Value, key: &str) -> String {
    match &v[key] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}
fn truth(s: &str) -> bool {
    matches!(s.to_lowercase().as_str(), "true" | "1" | "yes")
}

pub fn parse(payload: &str) -> Result<(Vec<Node>, usize), String> {
    if payload.len() > 8 * 1024 * 1024 {
        return Err("订阅内容超过 8 MiB 限制".into());
    }
    let text = payload.trim().trim_start_matches('\u{feff}');
    let decoded = if text.contains("://") || text.contains("proxies:") {
        text.to_string()
    } else {
        b64(text).unwrap_or_else(|_| text.to_string())
    };
    let mut nodes = vec![];
    let mut rejected = 0;
    if let Ok(v) = serde_yaml::from_str::<Value>(&decoded) {
        if let Some(entries) = v.get("proxies").and_then(Value::as_array) {
            for entry in entries.iter().take(10000) {
                match parse_clash(entry) {
                    Ok(n) => nodes.push(n),
                    Err(_) => rejected += 1,
                }
            }
            rejected += entries.len().saturating_sub(10000);
        }
    }
    if nodes.is_empty() && !decoded.contains("proxies:") {
        for line in decoded
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
            .take(10000)
        {
            match parse_link(line.trim_matches(|c| c == '"' || c == '\'' || c == ',')) {
                Ok(n) => nodes.push(n),
                Err(_) => rejected += 1,
            }
        }
    }
    let mut seen = HashSet::new();
    nodes.retain(|n| seen.insert(n.id.clone()));
    if nodes.is_empty() {
        return Err("未解析到有效节点。支持分享链接、Base64 订阅和 Clash YAML。".into());
    }
    Ok((nodes, rejected))
}

pub fn parse_link(raw: &str) -> Result<Node, String> {
    let mut n = Node {
        id: id(raw),
        raw: raw.into(),
        network: "tcp".into(),
        security: "none".into(),
        vmess_security: "auto".into(),
        ..Node::default()
    };
    if let Some(body) = raw.strip_prefix("vmess://") {
        let v: Value = serde_json::from_str(&b64(body)?).map_err(|_| "VMess JSON 无效")?;
        n.protocol = "vmess".into();
        n.address = field(&v, "add");
        n.port = field(&v, "port").parse().map_err(|_| "端口无效")?;
        n.name = field(&v, "ps");
        n.user_id = field(&v, "id");
        n.alter_id = field(&v, "aid").parse().unwrap_or(0);
        for (key, target) in [
            ("net", &mut n.network),
            ("tls", &mut n.security),
            ("scy", &mut n.vmess_security),
        ] {
            let s = field(&v, key);
            if !s.is_empty() {
                *target = s;
            }
        }
        n.host = field(&v, "host");
        n.path = field(&v, "path");
        n.sni = field(&v, "sni");
    } else if let Some(body) = raw.strip_prefix("ss://") {
        n.protocol = "ss".into();
        let (body, name) = body.split_once('#').unwrap_or((body, ""));
        n.name = decode(name);
        let (body, query) = body.split_once('?').unwrap_or((body, ""));
        if query.contains("plugin=") {
            n.unsupported_reason =
                Some("此 Shadowsocks 节点需要外部插件，当前 Xray 模式未提供该插件。".into());
        }
        let decoded;
        let body = body.trim_end_matches('/');
        let body = if body.contains('@') {
            body
        } else {
            decoded = b64(body)?;
            &decoded
        };
        let (auth, address) = body.rsplit_once('@').ok_or("SS 地址格式无效")?;
        let auth = if auth.contains(':') {
            decode(auth)
        } else {
            b64(auth)?
        };
        let (method, password) = auth.split_once(':').ok_or("SS 凭据格式无效")?;
        let u = Url::parse(&format!("socks://{address}")).map_err(|_| "SS 地址无效")?;
        n.address = u
            .host_str()
            .unwrap_or_default()
            .trim_matches(['[', ']'])
            .into();
        n.port = u.port().ok_or("SS 缺少端口")?;
        n.method = method.into();
        n.password = password.into();
    } else {
        let u = Url::parse(raw).map_err(|_| "链接格式无效")?;
        n.protocol = match u.scheme() {
            "vless" | "trojan" | "socks" | "http" | "anytls" | "tuic" => u.scheme(),
            "https" => "http",
            "socks5" => "socks",
            "hysteria2" | "hy2" => "hysteria2",
            _ => return Err("未知分享链接协议".into()),
        }
        .into();
        n.address = u
            .host_str()
            .unwrap_or_default()
            .trim_matches(['[', ']'])
            .into();
        n.port = u
            .port_or_known_default()
            .or(if n.protocol == "hysteria2" {
                Some(443)
            } else {
                None
            })
            .ok_or("缺少端口")?;
        n.name = decode(u.fragment().unwrap_or_default());
        let q: HashMap<String, String> = u
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        let get = |keys: &[&str]| {
            keys.iter()
                .find_map(|k| q.get(*k))
                .cloned()
                .unwrap_or_default()
        };
        if matches!(n.protocol.as_str(), "vless" | "http" | "socks" | "tuic") {
            n.user_id = decode(u.username());
            n.password = decode(u.password().unwrap_or_default());
        } else {
            n.password = decode(u.username());
            if let Some(pass) = u.password() {
                n.password.push(':');
                n.password.push_str(&decode(pass));
            }
        }
        n.network = get(&["type"]);
        if n.network.is_empty() {
            n.network = "tcp".into();
        }
        n.security = get(&["security"]);
        if n.security.is_empty() {
            n.security = if matches!(n.protocol.as_str(), "trojan" | "hysteria2" | "anytls")
                || u.scheme() == "https"
            {
                "tls"
            } else {
                "none"
            }
            .into();
        }
        n.flow = get(&["flow"]);
        n.host = get(&["host"]);
        n.path = get(&["path"]);
        n.service_name = get(&["serviceName", "grpc-service-name"]);
        n.sni = get(&["sni", "peer", "servername"]);
        n.fingerprint = get(&["fp", "fingerprint", "client-fingerprint"]);
        n.public_key = get(&["pbk", "public-key"]);
        n.short_id = get(&["sid", "short-id"]);
        n.spider_x = get(&["spx", "spider-x"]);
        n.alpn = get(&["alpn"]);
        n.obfs = get(&["obfs", "obfs-type"]);
        n.obfs_password = get(&["obfs-password", "obfs_password"]);
        n.allow_insecure = truth(&get(&["allowInsecure", "insecure"]));
        n.cert_sha256 = get(&["pinnedPeerCertSha256", "pinSHA256", "cert-sha256"]);
    }
    finish(n)
}
fn parse_clash(v: &Value) -> Result<Node, String> {
    let protocol = match field(v, "type").as_str() {
        "shadowsocks" => "ss".into(),
        "socks5" => "socks".into(),
        "hy2" => "hysteria2".into(),
        s => s.to_string(),
    };
    let raw = serde_json::to_string(v).map_err(|e| e.to_string())?;
    let mut n = Node {
        id: id(&raw),
        raw,
        protocol,
        name: field(v, "name"),
        address: field(v, "server"),
        port: field(v, "port").parse().map_err(|_| "端口无效")?,
        ..Node::default()
    };
    n.user_id = field(v, "uuid");
    if n.user_id.is_empty() {
        n.user_id = field(v, "username");
    }
    n.password = field(v, "password");
    n.method = field(v, "cipher");
    n.flow = field(v, "flow");
    n.alter_id = field(v, "alterId").parse().unwrap_or(0);
    n.vmess_security = field(v, "cipher");
    if n.vmess_security.is_empty() {
        n.vmess_security = "auto".into();
    }
    n.network = field(v, "network");
    if n.network.is_empty() {
        n.network = "tcp".into();
    }
    n.public_key = field(&v["reality-opts"], "public-key");
    n.short_id = field(&v["reality-opts"], "short-id");
    n.security = if !n.public_key.is_empty() {
        "reality"
    } else if truth(&field(v, "tls"))
        || matches!(n.protocol.as_str(), "trojan" | "hysteria2" | "anytls")
    {
        "tls"
    } else {
        "none"
    }
    .into();
    n.sni = field(v, "servername");
    if n.sni.is_empty() {
        n.sni = field(v, "sni");
    }
    n.fingerprint = field(v, "client-fingerprint");
    n.allow_insecure = truth(&field(v, "skip-cert-verify"));
    n.cert_sha256 = field(v, "certificate-sha256");
    n.host = field(&v["ws-opts"]["headers"], "Host");
    n.path = field(&v["ws-opts"], "path");
    if n.path.is_empty() {
        n.path = field(v, "ws-path");
    }
    n.service_name = field(&v["grpc-opts"], "grpc-service-name");
    if let Some(a) = v["alpn"].as_array() {
        n.alpn = a
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(",");
    }
    n.obfs = field(v, "obfs");
    n.obfs_password = field(v, "obfs-password");
    if !field(v, "plugin").is_empty() {
        n.unsupported_reason = Some("节点依赖外部 Shadowsocks 插件。".into());
    }
    finish(n)
}
fn finish(mut n: Node) -> Result<Node, String> {
    if n.address.is_empty() || n.port == 0 {
        return Err("节点地址或端口无效".into());
    }
    if n.name.is_empty() {
        n.name = n.address.clone();
    }
    if matches!(n.protocol.as_str(), "vmess" | "vless") && n.user_id.is_empty() {
        return Err("缺少 UUID".into());
    }
    if n.unsupported_reason.is_none() {
        if !matches!(
            n.protocol.as_str(),
            "vmess"
                | "vless"
                | "trojan"
                | "ss"
                | "socks"
                | "http"
                | "hysteria2"
                | "tuic"
                | "anytls"
        ) {
            n.unsupported_reason = Some(format!("当前核心未支持 {}", n.protocol));
        } else if matches!(n.network.as_str(), "h2" | "http" | "quic") {
            n.unsupported_reason = Some(format!(
                "当前 Xray 已移除旧 {} 传输；请向服务提供方获取新配置。",
                n.network
            ));
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_and_base64() {
        let s="vless://00000000-0000-0000-0000-000000000001@[::1]:443?security=reality&pbk=test&sid=ab&fp=chrome&type=ws&path=%2Fa#%E6%B5%8B%E8%AF%95";
        let (n, _) = parse(&STANDARD.encode(s)).unwrap();
        assert_eq!(n[0].name, "测试");
        assert_eq!(n[0].address, "::1");
        assert_eq!(n[0].path, "/a");
        assert_eq!(n[0].security, "reality");
    }
    #[test]
    fn yaml_nested() {
        let(n,_)=parse("proxies:\n - name: 'a, b'\n   type: vless\n   server: localhost\n   port: 443\n   uuid: test\n   network: ws\n   ws-opts:\n     path: /hello\n     headers: {Host: example.com}\n   reality-opts: {public-key: abc, short-id: '12'}").unwrap();
        assert_eq!(n[0].name, "a, b");
        assert_eq!(n[0].host, "example.com");
        assert_eq!(n[0].path, "/hello");
        assert_eq!(n[0].short_id, "12");
    }
    #[test]
    fn ss_variants() {
        for s in [
            format!(
                "ss://{}@localhost:8388#hello",
                STANDARD.encode("aes-128-gcm:a:b")
            ),
            format!("ss://{}", STANDARD.encode("aes-128-gcm:a:b@localhost:8388")),
        ] {
            let n = parse_link(&s).unwrap();
            assert_eq!(n.password, "a:b");
            assert_eq!(n.port, 8388);
        }
    }
    #[test]
    fn invalid_entries_and_duplicates() {
        let (n, rejected) =
            parse("socks://127.0.0.1:1080#a\ninvalid\nsocks://127.0.0.1:1080#a").unwrap();
        assert_eq!(n.len(), 1);
        assert_eq!(rejected, 1);
        assert!(parse("bad").is_err());
    }
}
