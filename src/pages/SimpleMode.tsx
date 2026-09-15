import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import {
  ArrowLeft,
  ChevronDown,
  Gauge,
  Moon,
  Network,
  Plus,
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
  onAddSubscription,
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
  onAddSubscription: () => void;
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
  const sortedNodes = [...(data?.nodes ?? [])].sort((a, b) => {
    const aDelay = a.lastError
      ? Number.POSITIVE_INFINITY
      : (a.delayMs ?? Number.POSITIVE_INFINITY);
    const bDelay = b.lastError
      ? Number.POSITIVE_INFINITY
      : (b.delayMs ?? Number.POSITIVE_INFINITY);
    return aDelay - bDelay;
  });
  const nodeResult = (item: (typeof sortedNodes)[number]) =>
    item.lastError
      ? "失败"
      : item.delayMs === null
        ? "未测试"
        : `${item.delayMs} ms`;
  const testNodes = (mode: "tcp" | "http", all: boolean) => {
    const ids = all
      ? sortedNodes.map((item) => item.id)
      : node
        ? [node.id]
        : [];
    if (ids.length) void run("test", { mode, ids });
  };

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
                  aria-label="当前节点"
                  value={data.activeNodeId ?? ""}
                  disabled={unavailable || connected || connecting}
                  onChange={(event) =>
                    void run("select", { id: event.target.value }, "已切换节点")
                  }
                >
                  <option value="" disabled>
                    选择节点
                  </option>
                  {sortedNodes.map((item) => (
                    <option key={item.id} value={item.id}>
                      {item.name} · {item.protocol.toUpperCase()} ·{" "}
                      {nodeResult(item)}
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
            {node && (
              <span
                className={`simple-latency ${node.lastError ? "failed" : ""}`}
              >
                {nodeResult(node)}
              </span>
            )}
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
            disabled={unavailable}
            onClick={onAddSubscription}
          >
            <Plus size={15} />
            添加订阅
          </button>
          <button
            className="button secondary"
            disabled={unavailable || !data?.subscriptions.length}
            onClick={() => void run("updateAllSubscriptions")}
          >
            <RefreshCw size={15} />
            更新全部订阅
          </button>
          <div className="simple-test-actions">
            <button
              className="button secondary simple-test-main"
              aria-label="测试当前节点，HTTP 延迟"
              disabled={unavailable || !node}
              onClick={() => testNodes("http", false)}
            >
              <Gauge size={15} />
              测试当前
            </button>
            <DropdownMenu.Root>
              <DropdownMenu.Trigger asChild>
                <button
                  className="button secondary simple-test-menu"
                  aria-label="选择测速范围和方式"
                  disabled={unavailable || !sortedNodes.length}
                >
                  <ChevronDown size={15} />
                </button>
              </DropdownMenu.Trigger>
              <DropdownMenu.Portal>
                <DropdownMenu.Content
                  className="dropdown"
                  align="end"
                  sideOffset={6}
                >
                  <DropdownMenu.Item
                    disabled={!node}
                    onSelect={() => testNodes("http", false)}
                  >
                    <Gauge size={15} />
                    当前节点 · HTTP 延迟
                  </DropdownMenu.Item>
                  <DropdownMenu.Item
                    disabled={!node}
                    onSelect={() => testNodes("tcp", false)}
                  >
                    <Gauge size={15} />
                    当前节点 · TCP 延迟
                  </DropdownMenu.Item>
                  <DropdownMenu.Separator className="menu-separator" />
                  <DropdownMenu.Item onSelect={() => testNodes("http", true)}>
                    <Network size={15} />
                    全部节点 · HTTP 延迟
                  </DropdownMenu.Item>
                  <DropdownMenu.Item onSelect={() => testNodes("tcp", true)}>
                    <Network size={15} />
                    全部节点 · TCP 延迟
                  </DropdownMenu.Item>
                </DropdownMenu.Content>
              </DropdownMenu.Portal>
            </DropdownMenu.Root>
          </div>
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
        <span>v0.2.5</span>
      </footer>
    </div>
  );
}
