import { useEffect, useState } from "react";
import {
  ArrowDown,
  ArrowUp,
  Clock3,
  Globe2,
  Power,
  ShieldCheck,
  Activity,
  Download,
  Network,
} from "lucide-react";
import {
  bytes,
  captureModeText,
  connectionActive,
  connectionStatusText,
  coreName,
  dateText,
  type Snapshot,
} from "../lib/proxy";
import type { Run } from "../components/ProxyDialogs";
export function ProxyOverview({
  snapshot,
  run,
  onNodes,
  onImport,
  busy,
}: {
  snapshot: Snapshot;
  run: Run;
  onNodes: () => void;
  onImport: () => void;
  busy: boolean;
}) {
  const { data, connection, traffic, core, job } = snapshot;
  const node = data.nodes.find(
    (n) => n.id === (connection.nodeId ?? data.activeNodeId),
  );
  const connected = connection.status === "connected";
  const active = connectionActive(connection.status);
  const [clock, setClock] = useState(() => Date.now());
  useEffect(() => {
    setClock(Date.now());
    if (!connection.since) return;
    const timer = window.setInterval(() => setClock(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [connection.since]);
  const seconds = connection.since
    ? Math.max(
        0,
        Math.floor((clock - Date.parse(connection.since)) / 1000),
      )
    : 0;
  const duration = `${Math.floor(seconds / 3600)
    .toString()
    .padStart(2, "0")}:${Math.floor((seconds / 60) % 60)
    .toString()
    .padStart(2, "0")}:${(seconds % 60).toString().padStart(2, "0")}`;
  return (
    <>
      <div className="page-heading">
        <div>
          <div className="eyebrow">YOUR CONNECTION, YOUR CONTROL</div>
          <h1>连接，从容一点。</h1>
          <p>双核心代理客户端 · 本机独立配置</p>
        </div>
        <span className="pill">
          <span className="status-dot" />
          {data.settings.proxyMode === "rule"
            ? "规则模式"
            : data.settings.proxyMode === "global"
              ? "全局模式"
              : "直连模式"}
        </span>
      </div>
      <section
        className={`connection-card ${connected ? "is-connected" : ""} ${
          connection.status === "networkUnavailable" ||
          connection.status === "coreCrashed" ||
          connection.status === "proxyFailed"
            ? "has-problem"
            : ""
        }`}
      >
        <div className="connection-main">
          <div className="connection-icon">
            <Globe2 size={34} />
          </div>
          <div>
            <span className="eyebrow">CONNECTION</span>
            <h2>{connectionStatusText(connection.status)}</h2>
            <p>
              {node?.name ??
                (data.settings.proxyMode === "direct"
                  ? "直连模式"
                  : "选择一个节点，开始你的连接。")}
            </p>
          </div>
        </div>
        <button
          className={`power-button ${active ? "on" : ""}`}
          aria-label={active ? "断开连接" : "启动连接"}
          disabled={busy || (!active && job.running)}
          onClick={() =>
            void run(
              active ? "disconnect" : "connect",
              {},
              active ? "已断开" : "",
            )
          }
        >
          <Power size={29} />
        </button>
        <div className="connection-bottom">
          <span>
            <ShieldCheck size={14} />
            {captureModeText(
              active ? connection.captureMode : data.settings.captureMode,
              active,
            )}
          </span>
          <span>{node ? coreName(node) : "Xray + sing-box"}</span>
          <span>
            <Clock3 size={14} />
            {duration}
          </span>
        </div>
      </section>
      <p className="connection-note">
        {connection.lastVerified
          ? `最近验证：${dateText(connection.lastVerified)}${
              connection.recoveryReason ? ` · ${connection.recoveryReason}` : ""
            }`
          : "连接时会依次检查核心、本地端口、流量接管和远端网络。"}
      </p>
      {connection.error && (
        <div className="error-banner" role="alert">
          {connection.error}
        </div>
      )}
      <div className="traffic-grid">
        <div className="metric">
          <ArrowDown size={19} />
          <small>实时下载</small>
          <strong>
            {bytes(traffic.downloadSpeed)}
            <em>/s</em>
          </strong>
        </div>
        <div className="metric">
          <ArrowUp size={19} />
          <small>实时上传</small>
          <strong>
            {bytes(traffic.uploadSpeed)}
            <em>/s</em>
          </strong>
        </div>
        <div className="metric">
          <Activity size={19} />
          <small>今日流量估算</small>
          <strong>{bytes(traffic.todayUpload + traffic.todayDownload)}</strong>
        </div>
      </div>
      <p className="muted tiny">
        流量按连接期间的 Windows
        网卡计数器估算，包含其他应用流量，不是核心精确分流统计。
      </p>
      <div className="section-heading">
        <h2>当前节点</h2>
        <button className="text-button" onClick={onNodes}>
          管理全部 {data.nodes.length} 个节点 →
        </button>
      </div>
      {node ? (
        <button className="selected-node" onClick={onNodes}>
          <div className="summary-icon blue">
            <Network size={22} />
          </div>
          <div>
            <strong>{node.name}</strong>
            <p>
              {node.protocol.toUpperCase()} · {node.address}:{node.port}
            </p>
          </div>
          <span className="pill">
            {node.lastError
              ? "测试失败"
              : node.delayMs !== null
                ? `${node.delayMs} ms`
                : "未测试"}
          </span>
        </button>
      ) : (
        <div className="empty-inline">
          <Network size={28} />
          <div>
            <h3>还没有选择节点</h3>
            <p>导入分享链接，或在订阅页面添加订阅。</p>
          </div>
          <button className="button secondary" onClick={onImport}>
            导入节点
          </button>
        </div>
      )}
      <div className="core-summary">
        <span>
          {core.installed ? "✓" : "!"} {core.version}
        </span>
        <span>
          {core.singboxInstalled ? "✓" : "!"} {core.singboxVersion}
        </span>
        <span>
          <Download size={13} />
          {core.geoReady ? "路由数据已就绪" : "路由数据缺失"}
        </span>
      </div>
    </>
  );
}
