use crate::models::{Node, Settings};
use serde_json::{json, Value};
use std::net::IpAddr;

pub fn list(s: &str) -> Vec<String> {
    s.split(['\n', '\r', ',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .map(str::to_string)
        .collect()
}
pub fn validate(s: &Settings) -> Result<(), String> {
    if s.http_port == 0 || s.socks_port == 0 || s.http_port == s.socks_port {
        return Err("HTTP / SOCKS 端口必须为 1–65535 且不能相同".into());
    }
    if !["rule", "global", "direct"].contains(&s.proxy_mode.as_str())
        || !["smart", "whitelist", "blacklist"].contains(&s.routing_mode.as_str())
    {
        return Err("代理或路由模式无效".into());
    }
    if !["none", "systemProxy", "tun"].contains(&s.capture_mode.as_str()) {
        return Err("流量接管方式无效".into());
    }
    if !(576..=9000).contains(&s.tun_mtu) {
        return Err("TUN MTU 应在 576 到 9000 之间".into());
    }
    if !(1..=60).contains(&s.test_timeout)
        || !(1..=32).contains(&s.test_concurrency)
        || s.test_retries > 3
        || !(1024..=20 * 1024 * 1024).contains(&s.download_bytes)
    {
        return Err("测速范围无效：超时 1–60 秒、并发 1–32、重试 0–3、下载 1KiB–20MiB".into());
    }
    validate_http(&s.test_url)?;
    for value in [&s.direct_ips, &s.proxy_ips, &s.block_ips] {
        for ip in list(value) {
            if ip.starts_with("geoip:") {
                continue;
            }
            let (host, mask) = ip
                .split_once('/')
                .map(|(h, m)| (h, Some(m)))
                .unwrap_or((&ip, None));
            let addr: IpAddr = host.parse().map_err(|_| format!("IP 规则格式无效：{ip}"))?;
            if let Some(mask) = mask {
                let n: u8 = mask.parse().map_err(|_| format!("CIDR 格式无效：{ip}"))?;
                if n > if addr.is_ipv4() { 32 } else { 128 } {
                    return Err(format!("CIDR 掩码超出范围：{ip}"));
                }
            }
        }
    }
    for value in [&s.direct_domains, &s.proxy_domains, &s.block_domains] {
        for domain in list(value) {
            if let Some(re) = domain.strip_prefix("regexp:") {
                regex::Regex::new(re).map_err(|_| format!("域名正则无效：{domain}"))?;
            } else if domain.chars().any(char::is_whitespace) || domain.contains("://") {
                return Err(format!("请填写域名规则，而非网址：{domain}"));
            }
        }
    }
    if s.fake_dns && !s.custom_dns {
        return Err("FakeDNS 需要先启用自定义 DNS".into());
    }
    if s.custom_dns {
        for server in [&s.domestic_dns, &s.foreign_dns, &s.doh_dns, &s.dot_dns]
            .into_iter()
            .flat_map(|v| list(v))
        {
            if server.parse::<IpAddr>().is_err()
                && !server.starts_with("https://")
                && !server.starts_with("tls://")
                && server != "localhost"
            {
                return Err(format!("DNS 地址无效：{server}"));
            }
        }
    }
    if !s.update_repo.is_empty() {
        let parts: Vec<_> = s.update_repo.split('/').collect();
        if parts.len() != 2
            || parts.iter().any(|p| {
                p.is_empty()
                    || !p
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            })
        {
            return Err("更新源应为 GitHub 的 owner/repo".into());
        }
    }
    Ok(())
}
pub fn validate_http(s: &str) -> Result<(), String> {
    let u = url::Url::parse(s).map_err(|_| "URL 无效")?;
    if !["http", "https"].contains(&u.scheme())
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
    {
        return Err("请输入不含内嵌用户名密码的 HTTP/HTTPS 地址".into());
    }
    Ok(())
}
pub fn outbound(n: &Node) -> Result<Value, String> {
    if let Some(reason) = &n.unsupported_reason {
        return Err(reason.clone());
    }
    let mut out = json!({"tag":"proxy","protocol":n.protocol,"settings":{}});
    out["settings"] = match n.protocol.as_str() {
        "vmess" | "vless" => {
            let mut user = json!({"id":n.user_id});
            if n.protocol == "vmess" {
                user["alterId"] = json!(n.alter_id);
                user["security"] = json!(if n.vmess_security.is_empty() {
                    "auto"
                } else {
                    &n.vmess_security
                });
            } else {
                user["encryption"] = json!("none");
                if !n.flow.is_empty() {
                    user["flow"] = json!(n.flow);
                }
            }
            json!({"vnext":[{"address":n.address,"port":n.port,"users":[user]}]})
        }
        "trojan" => json!({"servers":[{"address":n.address,"port":n.port,"password":n.password}]}),
        "ss" => {
            out["protocol"] = json!("shadowsocks");
            json!({"servers":[{"address":n.address,"port":n.port,"method":n.method,"password":n.password}]})
        }
        "socks" | "http" => {
            let mut server = json!({"address":n.address,"port":n.port});
            if !n.user_id.is_empty() {
                server["users"] = json!([{"user":n.user_id,"pass":n.password}]);
            }
            json!({"servers":[server]})
        }
        "hysteria2" => {
            out["protocol"] = json!("hysteria");
            json!({"version":2,"address":n.address,"port":n.port})
        }
        _ => return Err(format!("当前 Xray 不支持 {}", n.protocol)),
    };
    let network = if n.protocol == "hysteria2" {
        "hysteria"
    } else if n.network.is_empty() {
        "tcp"
    } else {
        &n.network
    };
    if ![
        "tcp",
        "raw",
        "ws",
        "grpc",
        "httpupgrade",
        "xhttp",
        "splithttp",
        "kcp",
        "hysteria",
    ]
    .contains(&network)
    {
        return Err(format!("不支持的传输方式：{network}"));
    }
    let security = if n.security.is_empty() {
        "none"
    } else {
        &n.security
    };
    let mut stream = json!({"network":network,"security":security});
    match security {
        "tls" => {
            if n.allow_insecure && n.cert_sha256.is_empty() {
                return Err("此版本 Xray 已移除跳过 TLS 校验。请在节点详情填写服务器证书 SHA-256 指纹，或使用有效证书。".into());
            }
            let mut tls = json!({});
            if !n.cert_sha256.is_empty() {
                tls["pinnedPeerCertSha256"] = json!(n.cert_sha256);
            }
            if !n.sni.is_empty() {
                tls["serverName"] = json!(n.sni);
            }
            if !n.fingerprint.is_empty() {
                tls["fingerprint"] = json!(n.fingerprint);
            }
            if !n.alpn.is_empty() {
                tls["alpn"] = json!(list(&n.alpn));
            }
            stream["tlsSettings"] = tls;
        }
        "reality" => {
            stream["realitySettings"] = json!({"serverName":n.sni,"fingerprint":if n.fingerprint.is_empty(){"chrome"}else{&n.fingerprint},"password":n.public_key,"shortId":n.short_id,"spiderX":n.spider_x});
        }
        "none" => {}
        _ => return Err("不支持的传输安全类型".into()),
    }
    match network {
        "ws" => {
            stream["wsSettings"] =
                json!({"path":if n.path.is_empty(){"/"}else{&n.path},"headers":{"Host":n.host}});
        }
        "grpc" => {
            stream["grpcSettings"] = json!({"serviceName":n.service_name});
        }
        "httpupgrade" => {
            stream["httpupgradeSettings"] = json!({"host":n.host,"path":n.path});
        }
        "xhttp" | "splithttp" => {
            stream["xhttpSettings"] = json!({"host":n.host,"path":n.path,"mode":"auto"});
        }
        "hysteria" => {
            stream["hysteriaSettings"] = json!({"version":2,"auth":n.password});
            if !n.obfs.is_empty() {
                if n.obfs != "salamander" {
                    return Err("Hysteria 混淆方式不支持".into());
                }
                stream["finalmask"] =
                    json!({"udp":[{"type":"salamander","settings":{"password":n.obfs_password}}]});
            }
        }
        _ => {}
    }
    out["streamSettings"] = stream;
    Ok(out)
}
fn add_rules(rules: &mut Vec<Value>, tag: &str, domains: Vec<String>, ips: Vec<String>) {
    // Xray fields in one rule are AND, so domain and IP matches must be separate rules.
    if !domains.is_empty() {
        rules.push(json!({"type":"field","outboundTag":tag,"domain":domains}));
    }
    if !ips.is_empty() {
        rules.push(json!({"type":"field","outboundTag":tag,"ip":ips}));
    }
}
pub fn build(node: &Node, s: &Settings) -> Result<Value, String> {
    validate(s)?;
    let out = if s.proxy_mode == "direct" {
        json!({"tag":"proxy","protocol":"freedom"})
    } else {
        outbound(node)?
    };
    let mut rules = vec![];
    if s.proxy_mode == "rule" {
        add_rules(
            &mut rules,
            "block",
            list(&s.block_domains),
            list(&s.block_ips),
        );
        add_rules(
            &mut rules,
            "direct",
            list(&s.direct_domains),
            list(&s.direct_ips),
        );
        add_rules(
            &mut rules,
            "proxy",
            list(&s.proxy_domains),
            list(&s.proxy_ips),
        );
        if s.bypass_mainland {
            add_rules(
                &mut rules,
                "direct",
                vec!["geosite:private".into(), "geosite:cn".into()],
                vec!["geoip:private".into(), "geoip:cn".into()],
            );
        }
        rules.push(json!({"type":"field","network":"tcp,udp","outboundTag":if s.routing_mode=="whitelist"{"direct"}else{"proxy"}}));
    }
    if s.capture_mode == "tun"
        && s.tun_bypass_lan
        && s.proxy_mode != "direct"
        && (s.proxy_mode != "rule" || !s.bypass_mainland)
    {
        let fallback = (s.proxy_mode == "rule").then(|| rules.pop()).flatten();
        add_rules(
            &mut rules,
            "direct",
            vec!["geosite:private".into()],
            vec!["geoip:private".into()],
        );
        if let Some(fallback) = fallback {
            rules.push(fallback);
        }
    }
    let sniff = json!({"enabled":true,"destOverride":if s.fake_dns{vec!["http","tls","fakedns"]}else{vec!["http","tls"]},"routeOnly":!s.fake_dns});
    let mut inbounds = vec![
        json!({"tag":"http","listen":"127.0.0.1","port":s.http_port,"protocol":"http","sniffing":sniff}),
        json!({"tag":"socks","listen":"127.0.0.1","port":s.socks_port,"protocol":"socks","settings":{"auth":"noauth","udp":true},"sniffing":sniff}),
    ];
    let mut outbounds = vec![
        out,
        json!({"tag":"direct","protocol":"freedom"}),
        json!({"tag":"block","protocol":"blackhole"}),
    ];
    if s.capture_mode == "tun" {
        let mut gateway = vec!["172.19.0.1/30"];
        let mut system_routes = vec!["0.0.0.0/0"];
        let mut tun_dns = vec!["1.1.1.1"];
        if s.tun_ipv6 {
            gateway.push("fdfe:dcba:9876::1/126");
            system_routes.push("::/0");
            tun_dns.push("2606:4700:4700::1111");
        }
        inbounds.push(json!({
            "tag":"tun",
            "protocol":"tun",
            "settings":{
                "name":"lumaroute_tun",
                "desc":"LumaRoute",
                "mtu":s.tun_mtu,
                "gateway":gateway,
                "dns":tun_dns,
                "autoSystemRoutingTable":system_routes,
                "autoOutboundsInterface":"auto"
            },
            "sniffing":{
                "enabled":true,
                "destOverride":if s.fake_dns{vec!["http","tls","quic","fakedns"]}else{vec!["http","tls","quic"]},
                "routeOnly":true
            }
        }));
        rules.insert(0, json!({"type":"field","inboundTag":["tun"],"network":"tcp,udp","port":53,"outboundTag":"dns-out"}));
        outbounds.push(json!({"tag":"dns-out","protocol":"dns"}));
    }
    let mut c = json!({"log":{"loglevel":"warning"},"inbounds":inbounds,"outbounds":outbounds,"routing":{"domainStrategy":if s.proxy_mode=="rule"{"IPIfNonMatch"}else{"AsIs"},"rules":rules}});
    if s.custom_dns {
        let mut servers = vec![];
        if s.fake_dns {
            servers.push(json!("fakedns"));
            c["fakedns"] = json!([{"ipPool":"198.18.0.0/16","poolSize":65535}]);
        }
        for server in list(&s.domestic_dns) {
            servers.push(if s.split_dns {
                json!({"address":server,"domains":["geosite:cn"],"expectIPs":["geoip:cn"]})
            } else {
                json!(server)
            });
        }
        let foreign = if !s.dot_dns.trim().is_empty() {
            &s.dot_dns
        } else if !s.doh_dns.trim().is_empty() {
            &s.doh_dns
        } else {
            &s.foreign_dns
        };
        for server in list(foreign) {
            servers.push(json!(server));
        }
        if servers.is_empty() {
            servers.push(json!("1.1.1.1"));
        }
        c["dns"] = json!({"queryStrategy":"UseIPv4","servers":servers});
    } else if s.capture_mode == "tun" {
        c["dns"] = json!({
            "queryStrategy":if s.tun_ipv6{"UseIP"}else{"UseIPv4"},
            "servers":["1.1.1.1","8.8.8.8"]
        });
    }
    Ok(c)
}
pub fn probe(n: &Node, port: u16) -> Result<Value, String> {
    Ok(
        json!({"log":{"loglevel":"error"},"inbounds":[{"listen":"127.0.0.1","port":port,"protocol":"http"}],"outbounds":[outbound(n)?]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routing_is_or_and_ordered() {
        let s = Settings {
            block_domains: "domain:bad.test".into(),
            block_ips: "10.0.0.0/8".into(),
            ..Settings::default()
        };
        let n = crate::parser::parse_link("socks://127.0.0.1:1080").unwrap();
        let c = build(&n, &s).unwrap();
        let r = c["routing"]["rules"].as_array().unwrap();
        assert_eq!(r[0]["outboundTag"], "block");
        assert!(r[0].get("ip").is_none());
        assert!(r[1].get("domain").is_none());
    }
    #[test]
    fn settings_validation() {
        let mut s = Settings::default();
        s.http_port = s.socks_port;
        assert!(validate(&s).is_err());
        s.http_port = 7890;
        s.direct_ips = "192.168.0.1/99".into();
        assert!(validate(&s).is_err());
    }
    #[test]
    fn direct_mode_works_without_node() {
        let s = Settings {
            proxy_mode: "direct".into(),
            ..Settings::default()
        };
        assert_eq!(
            build(&Node::default(), &s).unwrap()["outbounds"][0]["protocol"],
            "freedom"
        );
    }
    #[test]
    fn tun_mode_adds_native_xray_ingress_routes_and_dns() {
        let s = Settings {
            capture_mode: "tun".into(),
            tun_ipv6: true,
            tun_bypass_lan: true,
            ..Settings::default()
        };
        let n = crate::parser::parse_link("socks://127.0.0.1:1080").unwrap();
        let c = build(&n, &s).unwrap();
        let tun = c["inbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|inbound| inbound["tag"] == "tun")
            .unwrap();
        assert_eq!(tun["protocol"], "tun");
        assert_eq!(tun["settings"]["name"], "lumaroute_tun");
        assert_eq!(tun["settings"]["autoOutboundsInterface"], "auto");
        assert!(tun["settings"]["autoSystemRoutingTable"]
            .as_array()
            .unwrap()
            .iter()
            .any(|route| route == "::/0"));
        assert!(c["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|outbound| outbound["tag"] == "dns-out"));
        assert_eq!(c["routing"]["rules"][0]["port"], 53);
    }
}
