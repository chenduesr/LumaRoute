import {
  ArrowLeft,
  Moon,
  Network,
  Power,
  RefreshCw,
  Settings2,
  Sun,
  X,
} from "lucide-react";
import { coreName, type Snapshot } from "../lib/proxy";
import type { Run } from "../components/ProxyDialogs";
import type { Settings } from "../lib/types";
import mark from "../assets/mark.svg";

export function SimpleMode({
  snapshot,
  error,
  busy,
  run,
  appearance,
  saveAppearance,
  onClearError,
  onFullMode,
  onImport,
}: {
  snapshot: Snapshot | null;
  error: string;
  busy: boolean;
  run: Run;
  appearance: Settings;
  saveAppearance: (settings: Settings) => unknown;
  onClearError: () => void;
  onFullMode: (page?: string) => void;
  onImport: () => void;
}) {
  const data = snapshot?.data;
  const connection = snapshot?.connection;
  const job = snapshot?.job;
  const connected = connection?.status === "connected";
  const connecting = connection?.status === "connecting";
  const node = data?.nodes.find(
    (item) => item.id === (connection?.nodeId ?? data.activeNodeId),
  );
  const subscription = data?.subscriptions.find(
    (item) => item.id === node?.subscriptionId,
  );
  const unavailable = busy || Boolean(job?.running);

  return (
    <div className="simple-shell">
      <header className="simple-toolbar">
        <div className="simple-brand">
          <img src={mark} alt="" />
          <div>
            <strong>MyRay Lite</strong>
            <span>简洁模式</span>
          </div>
        </div>
        <div className="simple-toolbar-actions">
          <button
            className="icon-button"
            aria-label="切换主题"
            title="切换主题"
            onClick={() =>
              saveAppearance({
                ...appearance,
                theme:
                  document.documentElement.dataset.theme === "dark"
                    ? "light"
                    : "dark",
              })
            }
          >
            {appearance.theme === "light" ? (
              <Moon size={18} />
            ) : (
              <Sun size={18} />
            )}
          </button>
          <button
            className="button secondary small"
            onClick={() => onFullMode()}
          >
            <ArrowLeft size={15} />
            完整模式
          </button>
        </div>
      </header>

      <main className="simple-main">
        {(error || snapshot?.storageError) && (
          <div className="error-banner" role="alert">
            {error || `配置读取失败：${snapshot?.storageError}`}
            {error && (
              <button aria-label="关闭错误提示" onClick={onClearError}>
                <X size={16} />
              </button>
            )}
          </div>
        )}

        <section
          className={`simple-connection ${connected ? "is-connected" : ""}`}
        >
          <div className="simple-state">
            <span className={`simple-state-icon ${connected ? "on" : ""}`}>
              <Network size={24} />
            </span>
            <div>
              <span className="eyebrow">CONNECTION</span>
              <h1>
                {!snapshot
                  ? "正在准备…"
                  : connecting
                    ? "正在连接…"
                    : connected
                      ? "已连接"
                      : "未连接"}
              </h1>
              <p>
                {node?.name ??
                  (data?.settings.proxyMode === "direct"
                    ? "直连模式"
                    : "请选择节点")}
              </p>
            </div>
          </div>

          <button
            className={`simple-power ${connected ? "on" : ""}`}
            aria-label={connected || connecting ? "断开连接" : "启动连接"}
            disabled={!snapshot || unavailable}
            onClick={() =>
              void run(
                connected || connecting ? "disconnect" : "connect",
                {},
                connected ? "已断开" : "",
              )
            }
          >
            <Power size={30} />
          </button>

          {data && data.settings.proxyMode !== "direct" && (
            <label className="simple-node-select">
              <span>当前节点</span>
              {data.nodes.length ? (
                <select
                  value={data.activeNodeId ?? ""}
                  disabled={unavailable || connected || connecting}
                  onChange={(event) =>
                    void run("select", { id: event.target.value }, "已切换节点")
                  }
                >
                  <option value="" disabled>
                    选择节点
                  </option>
                  {data.nodes.map((item) => (
                    <option key={item.id} value={item.id}>
                      {item.name} · {item.protocol.toUpperCase()}
                    </option>
                  ))}
                </select>
              ) : (
                <button className="button secondary" onClick={onImport}>
                  导入节点
                </button>
              )}
            </label>
          )}

          <div className="simple-meta">
            <span>
              <span className={`status-dot ${connected ? "" : "offline"}`} />
              {connection?.systemProxy ? "系统代理已接管" : "系统代理未接管"}
            </span>
            <span>{node ? coreName(node) : "Xray + sing-box"}</span>
          </div>
        </section>

        {job?.running ? (
          <div className="simple-job" role="status">
            <RefreshCw size={15} className="spin" />
            <span>{job.message || "正在处理…"}</span>
            <button onClick={() => void run("cancel")}>取消</button>
          </div>
        ) : (
          job?.message && (
            <div className="job-result" role="status">
              {job.message}
            </div>
          )
        )}

        <div className="simple-actions">
          <button
            className="button secondary"
            disabled={unavailable || !data?.subscriptions.length}
            onClick={() => void run("updateAllSubscriptions")}
          >
            <RefreshCw size={15} />
            更新全部订阅
          </button>
          <button
            className="button secondary"
            onClick={() =>
              onFullMode(subscription ? "subscriptions" : "settings")
            }
          >
            <Settings2 size={15} />
            {subscription ? "管理订阅" : "设置"}
          </button>
        </div>
      </main>

      <footer className="simple-footer">
        <span>{data?.nodes.length ?? 0} 个节点</span>
        <span>v0.2.2</span>
      </footer>
    </div>
  );
}
