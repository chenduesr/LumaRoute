import { Plus, Rss, RefreshCw, Pencil, Trash2 } from "lucide-react";
import {
  bytes,
  dateText,
  type Snapshot,
  type Subscription,
} from "../lib/proxy";
import type { Run } from "../components/ProxyDialogs";
export function Subscriptions({
  snapshot,
  run,
  onEdit,
  onDelete,
}: {
  snapshot: Snapshot;
  run: Run;
  onEdit: (s?: Subscription) => void;
  onDelete: (s: Subscription) => void;
}) {
  return (
    <>
      <div className="page-heading">
        <div>
          <div className="eyebrow">SUBSCRIPTIONS</div>
          <h1>订阅</h1>
          <p>节点集中管理，更新失败时保留上次成功的数据。</p>
        </div>
        <div className="page-heading-actions">
          <button
            className="button secondary"
            disabled={
              snapshot.job.running || !snapshot.data.subscriptions.length
            }
            onClick={() => void run("updateAllSubscriptions")}
          >
            <RefreshCw size={15} />
            全部更新
          </button>
          <button className="button primary" onClick={() => onEdit()}>
            <Plus size={16} />
            添加订阅
          </button>
        </div>
      </div>
      {snapshot.data.subscriptions.length ? (
        <div className="subscription-list">
          {snapshot.data.subscriptions.map((s) => {
            const used = (s.uploadBytes ?? 0) + (s.downloadBytes ?? 0);
            const percentage = s.totalBytes
              ? Math.min(100, Math.max(0, (used / s.totalBytes) * 100))
              : 0;
            const expired = s.expiresAt
              ? new Date(s.expiresAt).getTime() < Date.now()
              : false;
            return (
              <article key={s.id} className="subscription-card">
                <div className="subscription-heading">
                  <span className="summary-icon blue">
                    <Rss size={20} />
                  </span>
                  <div>
                    <h2>{s.name}</h2>
                    <p>
                      {(() => {
                        try {
                          return new URL(s.url).hostname;
                        } catch {
                          return "地址无效";
                        }
                      })()}{" "}
                      ·{" "}
                      {
                        snapshot.data.nodes.filter(
                          (n) => n.subscriptionId === s.id,
                        ).length
                      }{" "}
                      个节点
                    </p>
                  </div>
                  <button
                    className="icon-button"
                    aria-label={`编辑 ${s.name}`}
                    onClick={() => onEdit(s)}
                  >
                    <Pencil size={16} />
                  </button>
                  <button
                    className="icon-button"
                    aria-label={`删除 ${s.name}`}
                    onClick={() => onDelete(s)}
                  >
                    <Trash2 size={16} />
                  </button>
                </div>
                <div className="subscription-meta">
                  <span>上次成功更新：{dateText(s.lastUpdated)}</span>
                  {s.format && <span>格式：{s.format}</span>}
                  <span>
                    {s.intervalHours
                      ? `每 ${s.intervalHours} 小时自动更新`
                      : "手动更新"}
                  </span>
                </div>
                {(s.totalBytes !== null || s.expiresAt) && (
                  <div className="subscription-usage">
                    {s.totalBytes !== null && (
                      <>
                        <div>
                          <span>订阅流量</span>
                          <strong>
                            已用 {bytes(used)} / 共 {bytes(s.totalBytes)}
                          </strong>
                        </div>
                        <div
                          className="subscription-usage-track"
                          role="progressbar"
                          aria-label={`${s.name} 流量使用比例`}
                          aria-valuemin={0}
                          aria-valuemax={100}
                          aria-valuenow={Math.round(percentage)}
                        >
                          <span style={{ width: `${percentage}%` }} />
                        </div>
                        <small>
                          剩余 {bytes(Math.max(0, s.totalBytes - used))}
                        </small>
                      </>
                    )}
                    {s.expiresAt && (
                      <small className={expired ? "expired" : undefined}>
                        {expired ? "已到期" : "到期时间"}：
                        {dateText(s.expiresAt)}
                      </small>
                    )}
                  </div>
                )}
                {s.error && <p className="form-error">{s.error}</p>}
                <button
                  className="button secondary"
                  disabled={snapshot.job.running}
                  onClick={() => void run("updateSubscription", { id: s.id })}
                >
                  <RefreshCw size={15} />
                  立即更新
                </button>
              </article>
            );
          })}
        </div>
      ) : (
        <div className="empty-state">
          <Rss size={35} />
          <h2>添加你的订阅来源</h2>
          <p>支持多个订阅，自动更新可单独设置。</p>
        </div>
      )}
    </>
  );
}
