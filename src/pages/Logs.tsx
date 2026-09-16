import { useEffect, useRef, useState } from "react";
import {
  CircleAlert,
  CircleCheck,
  CircleMinus,
  CircleX,
  Copy,
  Download,
  Stethoscope,
  Trash2,
} from "lucide-react";
import type { Snapshot } from "../lib/proxy";
import type { Run } from "../components/ProxyDialogs";
export function Logs({
  snapshot,
  run,
  onClear,
  onError,
}: {
  snapshot: Snapshot;
  run: Run;
  onClear: () => void;
  onError: (s: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [level, setLevel] = useState("");
  const [source, setSource] = useState("");
  const [follow, setFollow] = useState(true);
  const panel = useRef<HTMLDivElement>(null);
  const list = snapshot.logs.filter(
    (l) =>
      (!level || l.level === level) &&
      (!source || l.source === source) &&
      l.message.toLowerCase().includes(query.toLowerCase()),
  );
  useEffect(() => {
    if (follow && panel.current)
      panel.current.scrollTop = panel.current.scrollHeight;
  }, [snapshot.logs[snapshot.logs.length - 1]?.timestamp, follow]);
  return (
    <>
      <div className="page-heading">
        <div>
          <div className="eyebrow">LOGS & DIAGNOSTICS</div>
          <h1>日志</h1>
          <p>最近 1,000 条事件 · 核心和应用日志 · 本地滚动保存</p>
        </div>
        <button
          className="button secondary"
          disabled={snapshot.job.running}
          onClick={() => void run("diagnostics")}
        >
          <Stethoscope size={16} />
          {snapshot.job.running && snapshot.job.kind === "diagnostics"
            ? "诊断中…"
            : "运行诊断"}
        </button>
      </div>
      {snapshot.diagnostics && (
        <section className="diagnostic-report" aria-label="最近一次连接诊断">
          <div className="diagnostic-summary">
            <div>
              <strong>{snapshot.diagnostics.summary}</strong>
              <span>
                {new Date(snapshot.diagnostics.completedAt).toLocaleString(
                  "zh-CN",
                )}
              </span>
            </div>
          </div>
          <div className="diagnostic-checks">
            {snapshot.diagnostics.checks.map((check) => {
              const Icon =
                check.status === "ok"
                  ? CircleCheck
                  : check.status === "failed"
                    ? CircleX
                    : check.status === "warning"
                      ? CircleAlert
                      : CircleMinus;
              return (
                <article
                  key={check.key}
                  className={`diagnostic-check ${check.status}`}
                >
                  <Icon size={18} />
                  <div>
                    <strong>{check.label}</strong>
                    <p>{check.detail}</p>
                    {check.suggestion && <small>{check.suggestion}</small>}
                  </div>
                </article>
              );
            })}
          </div>
        </section>
      )}
      <div className="log-toolbar">
        <input
          aria-label="搜索日志"
          placeholder="搜索日志内容…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <select
          aria-label="日志级别"
          value={level}
          onChange={(e) => setLevel(e.target.value)}
        >
          <option value="">全部级别</option>
          {["INFO", "WARN", "ERROR"].map((s) => (
            <option key={s}>{s}</option>
          ))}
        </select>
        <select
          aria-label="日志来源"
          value={source}
          onChange={(e) => setSource(e.target.value)}
        >
          <option value="">全部来源</option>
          {[...new Set(snapshot.logs.map((l) => l.source))].map((s) => (
            <option key={s}>{s}</option>
          ))}
        </select>
      </div>
      <div className="log-actions">
        <label>
          <input
            type="checkbox"
            checked={follow}
            onChange={(e) => setFollow(e.target.checked)}
          />
          自动滚动
        </label>
        <span>{list.length} 条</span>
        <button
          className="text-button"
          onClick={() =>
            void navigator.clipboard
              .writeText(
                list
                  .map((l) => `[${l.timestamp}] [${l.level}] ${l.message}`)
                  .join("\n"),
              )
              .catch((e) => onError(String(e)))
          }
        >
          <Copy size={14} />
          复制可见日志
        </button>
        <button
          className="text-button"
          onClick={() => void run("exportDiagnostics")}
        >
          <Download size={14} />
          导出脱敏诊断包
        </button>
        <button className="text-button" onClick={onClear}>
          <Trash2 size={14} />
          清空
        </button>
      </div>
      <div className="log-panel" ref={panel}>
        {list.length ? (
          list.map((l, i) => (
            <div
              key={`${l.timestamp}-${i}`}
              className={`log-line ${l.level.toLowerCase()}`}
            >
              <time>{new Date(l.timestamp).toLocaleTimeString("zh-CN")}</time>
              <span className="log-level">{l.level}</span>
              <span className="log-source">{l.source}</span>
              <span>{l.message}</span>
            </div>
          ))
        ) : (
          <p className="empty-log">暂无匹配日志</p>
        )}
      </div>
      <p className="muted tiny">
        诊断包不含节点原文、订阅地址和凭据。导出后可在数据目录的 diagnostics
        文件夹查看。
      </p>
    </>
  );
}
