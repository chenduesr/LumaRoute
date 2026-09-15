import { useState, type ReactNode } from "react";
import * as Switch from "@radix-ui/react-switch";
import { Save } from "lucide-react";
import type { ProxySettings as Config, Snapshot } from "../lib/proxy";
import type { Settings as Appearance } from "../lib/types";
import type { Run } from "../components/ProxyDialogs";

export function ProxySettings({
  snapshot,
  run,
  appearance,
  saveAppearance,
  onRestore,
}: {
  snapshot: Snapshot;
  run: Run;
  appearance: Appearance;
  saveAppearance: (s: Appearance) => unknown;
  onRestore: () => void;
}) {
  const [draft, setDraft] = useState<Config>(snapshot.data.settings);
  const [tab, setTab] = useState("常规");
  const [saving, setSaving] = useState(false);
  const set = <K extends keyof Config>(k: K, v: Config[K]) =>
    setDraft((d) => ({ ...d, [k]: v }));
  const row = (title: string, help: string, control: ReactNode) => (
    <div className="config-row">
      <div>
        <strong>{title}</strong>
        <p>{help}</p>
      </div>
      {control}
    </div>
  );
  const toggle = (k: keyof Config, title: string, help: string) =>
    row(
      title,
      help,
      <Switch.Root
        className="switch"
        checked={Boolean(draft[k])}
        onCheckedChange={(v) => set(k, v)}
        aria-label={title}
      >
        <Switch.Thumb className="switch-thumb" />
      </Switch.Root>,
    );
  const input = (
    k: keyof Config,
    title: string,
    help: string,
    numeric = false,
  ) =>
    row(
      title,
      help,
      <input
        aria-label={title}
        type={numeric ? "number" : "text"}
        value={String(draft[k])}
        onChange={(e) =>
          set(k, numeric ? Number(e.target.value) : e.target.value)
        }
      />,
    );
  const select = (
    k: keyof Config,
    title: string,
    options: [string, string][],
  ) =>
    row(
      title,
      "",
      <select
        aria-label={title}
        value={String(draft[k])}
        onChange={(e) => set(k, e.target.value)}
      >
        {options.map(([v, label]) => (
          <option value={v} key={v}>
            {label}
          </option>
        ))}
      </select>,
    );
  return (
    <>
      <div className="page-heading">
        <div>
          <div className="eyebrow">PREFERENCES</div>
          <h1>设置</h1>
          <p>更改网络设置前请断开连接。保存后生效。</p>
        </div>
        <button
          className="button primary"
          disabled={saving}
          onClick={async () => {
            setSaving(true);
            await run("saveSettings", { settings: draft }, "设置已保存");
            setSaving(false);
          }}
        >
          <Save size={16} />
          保存设置
        </button>
      </div>
      <div className="settings-tabs" role="tablist" aria-label="设置分类">
        {["常规", "路由", "DNS", "测速", "更新", "外观"].map((t) => (
          <button
            role="tab"
            aria-selected={tab === t}
            className={tab === t ? "active" : ""}
            key={t}
            onClick={() => setTab(t)}
          >
            {t}
          </button>
        ))}
      </div>
      <section className="config-panel" role="tabpanel" aria-label={tab}>
        {tab === "常规" && (
          <>
            {select("proxyMode", "代理模式", [
              ["rule", "规则模式"],
              ["global", "全局代理"],
              ["direct", "全部直连"],
            ])}
            {input("httpPort", "HTTP 端口", "仅监听本机 127.0.0.1", true)}
            {input(
              "socksPort",
              "SOCKS 端口",
              "支持 SOCKS5 / UDP；与 HTTP 端口不同",
              true,
            )}
            {toggle(
              "systemProxy",
              "接管系统代理",
              "连接时应用到 Windows，断开和退出时恢复原设置。",
            )}
            {toggle(
              "startOnBoot",
              "登录 Windows 时启动",
              "只添加当前用户的启动项。",
            )}
            {toggle(
              "autoConnect",
              "启动后自动连接",
              "先更新订阅，再使用当前选中的节点和已保存的代理模式。",
            )}
            {toggle(
              "updateSubscriptionsOnLaunch",
              "启动时更新订阅",
              "依次更新全部订阅；失败时保留上次成功的节点。每次程序启动只执行一次。",
            )}
            {toggle(
              "minimizeToTray",
              "关闭窗口时隐藏到托盘",
              "从托盘菜单选择“退出”可断开连接并退出应用。",
            )}
          </>
        )}
        {tab === "路由" && (
          <>
            {select("routingMode", "规则策略", [
              ["smart", "智能规则"],
              ["whitelist", "白名单（其余直连）"],
              ["blacklist", "黑名单（其余代理）"],
            ])}
            {toggle(
              "bypassMainland",
              "中国大陆地址直连",
              "使用 geosite:cn 和 geoip:cn 路由数据。",
            )}
            <p className="config-note">
              规则优先级：阻止 → 直连 → 代理 → 大陆直连 → 默认。每行一条；域名与
              IP 分别匹配。智能与黑名单策略均默认代理。
            </p>
            {(
              [
                ["blockDomains", "阻止域名"],
                ["blockIps", "阻止 IP / CIDR"],
                ["directDomains", "直连域名"],
                ["directIps", "直连 IP / CIDR"],
                ["proxyDomains", "代理域名"],
                ["proxyIps", "代理 IP / CIDR"],
              ] as [keyof Config, string][]
            ).map(([k, title]) => (
              <label className="config-area" key={k}>
                {title}
                <textarea
                  rows={3}
                  value={String(draft[k])}
                  onChange={(e) => set(k, e.target.value)}
                  placeholder={
                    k.endsWith("Domains")
                      ? "domain:example.com\nfull:api.example.com"
                      : "192.168.0.0/16"
                  }
                />
              </label>
            ))}
          </>
        )}
        {tab === "DNS" && (
          <>
            {toggle(
              "customDns",
              "自定义 DNS",
              "由 Xray 统一解析，双核心共用相同路由入口。",
            )}
            {toggle(
              "splitDns",
              "国内外 DNS 分流",
              "geosite:cn 使用国内 DNS，其余使用国外 DNS。",
            )}
            {toggle(
              "fakeDns",
              "Fake DNS",
              "需启用自定义 DNS；HTTP / SOCKS 入口通过嗅探恢复域名。",
            )}
            {input("domesticDns", "国内 DNS", "IP 或 HTTPS DNS 地址")}
            {input("foreignDns", "国外 DNS", "IP 或 HTTPS DNS 地址")}
            {input(
              "dohDns",
              "DNS over HTTPS",
              "可留空；填写后用于国外域名解析。",
            )}
            {input(
              "dotDns",
              "DNS over TLS",
              "可留空，例如 tls://1.1.1.1；优先于 DoH。",
            )}
          </>
        )}
        {tab === "测速" && (
          <>
            {input(
              "testUrl",
              "测速地址",
              "HTTP 延迟与下载共用；下载测速请使用较大的测试文件。",
            )}
            {input("testTimeout", "超时（秒）", "1–60 秒", true)}
            {input("testConcurrency", "并发数", "1–32 个节点", true)}
            {input("testRetries", "重试次数", "0–3 次", true)}
            {input(
              "downloadBytes",
              "下载采样字节数",
              "1 KiB–20 MiB；遇到文件末尾时停止。",
              true,
            )}
            <p className="config-note">
              TCP 测试服务器端口；HTTP
              和下载测试通过临时本地核心连接远端。不会切换 Windows 系统代理。
            </p>
          </>
        )}
        {tab === "更新" && (
          <>
            {input(
              "updateRepo",
              "新版发布源",
              "GitHub owner/repository。独立配置，不使用旧 WPF 发布源。",
            )}
            {toggle(
              "autoCheckUpdates",
              "启动时检查应用更新",
              "仅检查并提示，不会自动下载安装。",
            )}
            <div className="config-actions">
              <button
                className="button secondary"
                disabled={snapshot.job.running}
                onClick={() => void run("checkUpdate", { kind: "app" })}
              >
                检查应用更新
              </button>
              <button
                className="button secondary"
                onClick={() => void run("openRelease")}
              >
                打开发布页面
              </button>
            </div>
            <p className="config-note">
              请先保存发布源。核心包从各自官方 GitHub 仓库下载，验证 SHA-256
              后替换，失败时保留现有核心。
            </p>
            {row(
              "Xray",
              snapshot.core.version,
              <button
                className="button secondary"
                disabled={
                  snapshot.job.running ||
                  snapshot.connection.status !== "disconnected"
                }
                onClick={() => void run("updateCore", { kind: "xray" })}
              >
                更新 Xray
              </button>,
            )}
            {row(
              "sing-box",
              snapshot.core.singboxVersion,
              <button
                className="button secondary"
                disabled={
                  snapshot.job.running ||
                  snapshot.connection.status !== "disconnected"
                }
                onClick={() => void run("updateCore", { kind: "singbox" })}
              >
                更新 sing-box
              </button>,
            )}
            {row(
              "路由数据",
              "更新 Xray 发布包中的 geoip.dat 与 geosite.dat",
              <button
                className="button secondary"
                disabled={
                  snapshot.job.running ||
                  snapshot.connection.status !== "disconnected"
                }
                onClick={() =>
                  void run("updateCore", { kind: "xray", geoOnly: true })
                }
              >
                更新路由数据
              </button>,
            )}
          </>
        )}
        {tab === "外观" && (
          <>
            {row(
              "简洁模式",
              "只显示节点、连接和订阅更新；完整功能随时可以切回。",
              <Switch.Root
                className="switch"
                aria-label="简洁模式"
                checked={Boolean(appearance.simpleMode)}
                onCheckedChange={(value) =>
                  saveAppearance({ ...appearance, simpleMode: value })
                }
              >
                <Switch.Thumb className="switch-thumb" />
              </Switch.Root>,
            )}
            {row(
              "主题",
              "立即生效",
              <select
                aria-label="主题"
                value={appearance.theme}
                onChange={(e) =>
                  saveAppearance({
                    ...appearance,
                    theme: e.target.value as Appearance["theme"],
                  })
                }
              >
                <option value="dark">深色</option>
                <option value="light">浅色</option>
                <option value="system">跟随系统</option>
              </select>,
            )}
            {row(
              "紧凑布局",
              "减少界面间距",
              <Switch.Root
                className="switch"
                aria-label="紧凑布局"
                checked={appearance.compact}
                onCheckedChange={(v) =>
                  saveAppearance({ ...appearance, compact: v })
                }
              >
                <Switch.Thumb className="switch-thumb" />
              </Switch.Root>,
            )}
            {row(
              "减少动态效果",
              "立即生效",
              <Switch.Root
                className="switch"
                aria-label="减少动态效果"
                checked={appearance.reducedMotion}
                onCheckedChange={(v) =>
                  saveAppearance({ ...appearance, reducedMotion: v })
                }
              >
                <Switch.Thumb className="switch-thumb" />
              </Switch.Root>,
            )}
            <div className="config-actions">
              <button
                className="button secondary"
                onClick={() => void run("openData")}
              >
                打开数据目录
              </button>
              <button className="button secondary" onClick={onRestore}>
                恢复上次配置备份
              </button>
            </div>
            <p className="config-note">
              LumaRoute 0.3.0 · Tauri 2 / React / Rust
              <br />
              旧项目与旧数据保持独立。节点凭据保存在当前用户的应用数据目录，请妥善保管。
            </p>
          </>
        )}
      </section>
    </>
  );
}
