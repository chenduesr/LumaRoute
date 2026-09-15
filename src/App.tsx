import { useState } from "react";
import * as Tooltip from "@radix-ui/react-tooltip";
import { isTauri } from "@tauri-apps/api/core";
import {
  ChevronRight,
  Moon,
  Sun,
  X,
  LayoutDashboard,
  Network,
  Rss,
  ScrollText,
  Settings2,
  PanelLeftClose,
} from "lucide-react";
import { Titlebar } from "./components/Titlebar";
import {
  ImportDialog,
  SubscriptionDialog,
  NodeDetail,
  Modal,
  ProxyErrorContext,
} from "./components/ProxyDialogs";
import { Hint } from "./components/ui/Hint";
import { usePersistentState } from "./hooks/usePersistentState";
import { useTheme } from "./hooks/useTheme";
import { useProxy } from "./hooks/useProxy";
import { defaultSettings, isSettings } from "./lib/storage";
import type { ProxyNode, Subscription } from "./lib/proxy";
import { ProxyOverview } from "./pages/ProxyOverview";
import { Nodes } from "./pages/Nodes";
import { Subscriptions } from "./pages/Subscriptions";
import { Logs } from "./pages/Logs";
import { ProxySettings } from "./pages/ProxySettings";
import { SimpleMode } from "./pages/SimpleMode";
import mark from "./assets/mark.svg";
import "./App.css";
const pages = [
  { id: "overview", label: "概览", icon: LayoutDashboard },
  { id: "nodes", label: "节点", icon: Network },
  { id: "subscriptions", label: "订阅", icon: Rss },
  { id: "logs", label: "日志", icon: ScrollText },
  { id: "settings", label: "设置", icon: Settings2 },
];
export default function App() {
  const [page, setPage] = useState("overview");
  const appearance = usePersistentState(
    "myray.settings.v1",
    defaultSettings,
    isSettings,
  );
  useTheme(appearance.value);
  const { snapshot, error, setError, notice, busy, run } = useProxy();
  const [importing, setImporting] = useState(false);
  const [subDialog, setSubDialog] = useState<{
    editing?: Subscription;
    updateAfterSave?: boolean;
  } | null>(null);
  const [detail, setDetail] = useState<ProxyNode | null>(null);
  const [confirm, setConfirm] = useState<{
    title: string;
    description: string;
    action: string;
    args?: Record<string, unknown>;
  } | null>(null);
  const confirmDeleteNode = (node: ProxyNode) =>
    setConfirm({
      title: "删除节点？",
      description: `将移除“${node.name}”。订阅节点在下次更新时可能重新出现。`,
      action: "deleteNode",
      args: { id: node.id },
    });
  const openFullMode = (target = "overview") => {
    appearance.save({ ...appearance.value, simpleMode: false });
    setPage(target);
  };
  return (
    <ProxyErrorContext.Provider value={error}>
      <Tooltip.Provider delayDuration={350}>
        <div className="app-shell">
          <Titlebar onError={setError} />
          {appearance.value.simpleMode ? (
            <SimpleMode
              snapshot={snapshot}
              error={error || appearance.error}
              busy={busy}
              run={run}
              appearance={appearance.value}
              saveAppearance={appearance.save}
              onClearError={() => setError("")}
              onFullMode={openFullMode}
              onImport={() => setImporting(true)}
              onAddSubscription={() => setSubDialog({ updateAfterSave: true })}
            />
          ) : (
            <div className="app-body">
              <aside className="sidebar">
                <div className="brand">
                  <img src={mark} alt="MyRay Lite 标志" />
                  <div>
                    <strong>
                      MyRay <span>Lite</span>
                    </strong>
                    <small>连接，从容一点。</small>
                  </div>
                </div>
                <div className="nav-label">我的连接</div>
                <nav aria-label="主导航">
                  {pages.map((p) => (
                    <button
                      key={p.id}
                      aria-label={p.label}
                      className={`nav-item ${page === p.id ? "active" : ""}`}
                      aria-current={page === p.id ? "page" : undefined}
                      onClick={() => setPage(p.id)}
                    >
                      <p.icon size={19} />
                      {p.label}
                      {p.id === "nodes" && (
                        <span className="nav-count">
                          {snapshot?.data.nodes.length ?? 0}
                        </span>
                      )}
                    </button>
                  ))}
                </nav>
                <div className="sidebar-bottom">
                  <div className="local-note">
                    <Network size={22} />
                    <strong>双核心 · 一处管理</strong>
                    <p>
                      Xray + sing-box
                      <br />
                      规则、节点与订阅，井然有序。
                    </p>
                  </div>
                  <div className="sidebar-footer">
                    <span
                      className={`status-dot ${snapshot?.connection.status === "connected" ? "" : "offline"}`}
                    />
                    {snapshot?.connection.status === "connected"
                      ? "已连接"
                      : "未连接"}
                    <span>v0.2.3</span>
                  </div>
                </div>
              </aside>
              <div className="main-shell">
                <div className="toolbar">
                  <div className="breadcrumb">
                    我的连接
                    <ChevronRight size={13} />
                    <span>{pages.find((p) => p.id === page)?.label}</span>
                  </div>
                  <div className="toolbar-actions">
                    <Hint label="切换到简洁模式">
                      <button
                        className="icon-button"
                        aria-label="切换到简洁模式"
                        onClick={() =>
                          appearance.save({
                            ...appearance.value,
                            simpleMode: true,
                          })
                        }
                      >
                        <PanelLeftClose size={18} />
                      </button>
                    </Hint>
                    <Hint label="切换深色 / 浅色主题">
                      <button
                        className="icon-button"
                        aria-label="切换主题"
                        onClick={() =>
                          appearance.save({
                            ...appearance.value,
                            theme:
                              document.documentElement.dataset.theme === "dark"
                                ? "light"
                                : "dark",
                          })
                        }
                      >
                        {appearance.value.theme === "light" ? (
                          <Moon size={18} />
                        ) : (
                          <Sun size={18} />
                        )}
                      </button>
                    </Hint>
                  </div>
                </div>
                <main id="main-content">
                  <div className="content-wrap">
                    {(error || appearance.error) && (
                      <div className="error-banner" role="alert">
                        {error || appearance.error}
                        <button
                          aria-label="关闭错误提示"
                          onClick={() => setError("")}
                        >
                          <X size={16} />
                        </button>
                      </div>
                    )}
                    {snapshot?.storageError && (
                      <div className="error-banner" role="alert">
                        配置读取失败：{snapshot.storageError}
                        。原文件已保留，请到设置 → 外观恢复备份。
                      </div>
                    )}
                    {snapshot?.job.running && (
                      <div className="job-banner" role="status">
                        <div>
                          <strong>
                            {snapshot.job.message || "任务进行中…"}
                          </strong>
                          <small>
                            {snapshot.job.total
                              ? `${snapshot.job.completed} / ${snapshot.job.total}`
                              : "正在处理，请稍候"}
                          </small>
                        </div>
                        <button
                          className="button secondary small"
                          onClick={() => void run("cancel")}
                        >
                          取消任务
                        </button>
                      </div>
                    )}
                    {snapshot &&
                      !snapshot.job.running &&
                      snapshot.job.message && (
                        <div className="job-result" role="status">
                          {snapshot.job.message}
                        </div>
                      )}
                    {!snapshot ? (
                      <div className="empty-state">
                        <Network size={35} />
                        <h1>
                          {isTauri() ? "正在连接本地服务…" : "请启动桌面应用"}
                        </h1>
                        <p>
                          {isTauri()
                            ? "首次启动会准备双核心，请稍候。"
                            : "运行 npm run tauri dev 以使用节点、订阅及代理功能。"}
                        </p>
                      </div>
                    ) : (
                      <div key={page}>
                        {page === "overview" && (
                          <ProxyOverview
                            snapshot={snapshot}
                            run={run}
                            onNodes={() => setPage("nodes")}
                            onImport={() => setImporting(true)}
                            busy={busy}
                          />
                        )}
                        {page === "nodes" && (
                          <Nodes
                            snapshot={snapshot}
                            run={run}
                            onImport={() => setImporting(true)}
                            onDetail={setDetail}
                            onDelete={confirmDeleteNode}
                            onError={setError}
                          />
                        )}
                        {page === "subscriptions" && (
                          <Subscriptions
                            snapshot={snapshot}
                            run={run}
                            onEdit={(editing) => setSubDialog({ editing })}
                            onDelete={(s) =>
                              setConfirm({
                                title: "删除订阅？",
                                description: `将移除“${s.name}”及其节点。`,
                                action: "deleteSubscription",
                                args: { id: s.id },
                              })
                            }
                          />
                        )}
                        {page === "logs" && (
                          <Logs
                            snapshot={snapshot}
                            run={run}
                            onError={setError}
                            onClear={() =>
                              setConfirm({
                                title: "清空日志？",
                                description: "清除当前可见日志和当前日志文件。",
                                action: "clearLogs",
                              })
                            }
                          />
                        )}
                        {page === "settings" && (
                          <ProxySettings
                            snapshot={snapshot}
                            run={run}
                            appearance={appearance.value}
                            saveAppearance={appearance.save}
                            onRestore={() =>
                              setConfirm({
                                title: "恢复上次配置备份？",
                                description:
                                  "将用上一次保存的配置替换当前配置。当前文件会另外保留。请先断开连接。",
                                action: "restoreBackup",
                              })
                            }
                          />
                        )}
                      </div>
                    )}
                  </div>
                </main>
                <div className="statusbar">
                  <span>
                    <span className="status-dot" />
                    {snapshot ? "本地服务已就绪" : "连接本地服务"}
                  </span>
                  <span>独立配置 · 简体中文</span>
                </div>
              </div>
            </div>
          )}
        </div>
        {importing && (
          <ImportDialog run={run} onClose={() => setImporting(false)} />
        )}
        {subDialog && (
          <SubscriptionDialog
            editing={subDialog.editing}
            run={run}
            updateAfterSave={subDialog.updateAfterSave}
            onClose={() => setSubDialog(null)}
          />
        )}
        {detail && (
          <NodeDetail
            node={
              snapshot?.data.nodes.find((n) => n.id === detail.id) ?? detail
            }
            run={run}
            onClose={() => setDetail(null)}
          />
        )}
        {confirm && (
          <Modal
            title={confirm.title}
            description={confirm.description}
            onClose={() => setConfirm(null)}
          >
            <div className="dialog-actions">
              <button
                className="button secondary"
                onClick={() => setConfirm(null)}
              >
                取消
              </button>
              <button
                className="button destructive"
                disabled={busy}
                onClick={async () => {
                  if (
                    (await run(confirm.action, confirm.args, "操作已完成")).ok
                  )
                    setConfirm(null);
                }}
              >
                确认
              </button>
            </div>
          </Modal>
        )}
        {notice && (
          <div className="toast" role="status">
            {notice}
          </div>
        )}
      </Tooltip.Provider>
    </ProxyErrorContext.Provider>
  );
}
