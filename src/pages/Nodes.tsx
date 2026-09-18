import { useMemo, useState } from "react";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import {
  MoreHorizontal,
  Plus,
  Search,
  Check,
  Network,
  Clipboard,
  Trash2,
  Info,
} from "lucide-react";
import { coreName, type ProxyNode, type Snapshot } from "../lib/proxy";
import { Hint } from "../components/ui/Hint";
import type { Run } from "../components/ProxyDialogs";
export function Nodes({
  snapshot,
  run,
  onImport,
  onDetail,
  onDelete,
  onError,
}: {
  snapshot: Snapshot;
  run: Run;
  onImport: () => void;
  onDetail: (n: ProxyNode) => void;
  onDelete: (n: ProxyNode) => void;
  onError: (s: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [protocol, setProtocol] = useState("");
  const [sort, setSort] = useState("default");
  const [mode, setMode] = useState("tcp");
  const nodes = useMemo(() => {
    let list = snapshot.data.nodes.filter(
      (n) =>
        (!protocol || n.protocol === protocol) &&
        `${n.name} ${n.address}`.toLowerCase().includes(query.toLowerCase()),
    );
    if (sort === "delay")
      list = [...list].sort(
        (a, b) => (a.delayMs ?? Infinity) - (b.delayMs ?? Infinity),
      );
    if (sort === "name")
      list = [...list].sort((a, b) => a.name.localeCompare(b.name));
    return list;
  }, [snapshot.data.nodes, query, protocol, sort]);
  return (
    <>
      <div className="page-heading">
        <div>
          <div className="eyebrow">NODES</div>
          <h1>节点</h1>
          <p>{snapshot.data.nodes.length} 个节点 · 选择、连接与批量测速</p>
        </div>
        <button className="button primary" onClick={onImport}>
          <Plus size={16} />
          导入节点
        </button>
      </div>
      <div className="nodes-toolbar">
        <div className="search-field">
          <Search size={17} />
          <input
            aria-label="搜索节点"
            placeholder="搜索名称或服务器…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <select
          aria-label="筛选协议"
          value={protocol}
          onChange={(e) => setProtocol(e.target.value)}
        >
          <option value="">全部协议</option>
          {[...new Set(snapshot.data.nodes.map((n) => n.protocol))].map((p) => (
            <option key={p}>{p}</option>
          ))}
        </select>
        <select
          aria-label="节点排序"
          value={sort}
          onChange={(e) => setSort(e.target.value)}
        >
          <option value="default">默认排序</option>
          <option value="delay">延迟优先</option>
          <option value="name">名称排序</option>
        </select>
      </div>
      <div className="test-toolbar">
        <span>已显示 {nodes.length} 个</span>
        <select
          aria-label="测速方式"
          value={mode}
          onChange={(e) => setMode(e.target.value)}
        >
          <option value="tcp">TCP 延迟</option>
          <option value="http">HTTP 延迟</option>
          <option value="download">下载测速</option>
        </select>
        <button
          className="button secondary"
          disabled={snapshot.job.running || nodes.length === 0}
          onClick={() =>
            void run("test", { mode, ids: nodes.map((n) => n.id) })
          }
        >
          测试当前列表
        </button>
      </div>
      {!nodes.length ? (
        <div className="empty-state">
          <Network size={35} />
          <h2>
            {snapshot.data.nodes.length
              ? "没有匹配的节点"
              : "从你的第一个节点开始"}
          </h2>
          <p>
            支持分享链接、Base64、Clash Meta YAML 和 sing-box JSON。AnyTLS/TUIC
            使用 sing-box。
          </p>
        </div>
      ) : (
        <div className="node-list">
          {nodes.map((n) => (
            <article
              className={`node-row ${snapshot.data.activeNodeId === n.id ? "selected" : ""}`}
              key={n.id}
            >
              <button
                className="node-select"
                aria-label={`选择 ${n.name}`}
                aria-pressed={snapshot.data.activeNodeId === n.id}
                onClick={() => void run("select", { id: n.id })}
              >
                {snapshot.data.activeNodeId === n.id ? (
                  <Check size={17} />
                ) : (
                  <span />
                )}
              </button>
              <button className="node-info" onClick={() => onDetail(n)}>
                <strong>{n.name}</strong>
                <span>
                  {n.protocol.toUpperCase()} · {n.address}:{n.port}
                </span>
                {n.unsupportedReason && (
                  <small className="form-error">{n.unsupportedReason}</small>
                )}
              </button>
              <Hint
                label={
                  n.lastError ??
                  `${coreName(n)} · ${n.lastTestMode || "未测试"}`
                }
              >
                <span
                  className={`latency ${n.lastError ? "failed" : ""}`}
                  tabIndex={0}
                >
                  {n.lastError
                    ? "失败"
                    : n.lastTestMode === "download" && n.downloadMbps !== null
                      ? `${n.downloadMbps.toFixed(2)} MiB/s`
                      : n.delayMs !== null
                        ? `${n.delayMs} ms`
                        : "—"}
                </span>
              </Hint>
              <button
                className="button secondary small"
                disabled={!!n.unsupportedReason || snapshot.job.running}
                onClick={() => void run("connect", { id: n.id })}
              >
                连接
              </button>
              <DropdownMenu.Root>
                <DropdownMenu.Trigger asChild>
                  <button
                    className="icon-button"
                    aria-label={`${n.name}的操作`}
                  >
                    <MoreHorizontal size={18} />
                  </button>
                </DropdownMenu.Trigger>
                <DropdownMenu.Portal>
                  <DropdownMenu.Content className="dropdown" align="end">
                    <DropdownMenu.Item onSelect={() => onDetail(n)}>
                      <Info size={15} />
                      节点详情
                    </DropdownMenu.Item>
                    <DropdownMenu.Item
                      onSelect={() =>
                        void navigator.clipboard
                          .writeText(n.name)
                          .catch((e) => onError(String(e)))
                      }
                    >
                      <Clipboard size={15} />
                      复制名称
                    </DropdownMenu.Item>
                    <DropdownMenu.Item
                      className="danger"
                      onSelect={() => onDelete(n)}
                    >
                      <Trash2 size={15} />
                      删除节点
                    </DropdownMenu.Item>
                  </DropdownMenu.Content>
                </DropdownMenu.Portal>
              </DropdownMenu.Root>
            </article>
          ))}
        </div>
      )}
    </>
  );
}
