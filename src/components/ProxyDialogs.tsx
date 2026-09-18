import * as Dialog from "@radix-ui/react-dialog";
import {
  useEffect,
  useRef,
  useState,
  useContext,
  createContext,
  type ReactNode,
} from "react";
import { ClipboardPaste, FileUp, QrCode, X } from "lucide-react";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import type { ProxyNode, Subscription } from "../lib/proxy";
import { coreName } from "../lib/proxy";
export const ProxyErrorContext = createContext("");
export type Run = (
  action: string,
  args?: Record<string, unknown>,
  message?: string,
) => Promise<{ ok: boolean; result: unknown }>;
export function Modal({
  title,
  description,
  children,
  onClose,
}: {
  title: string;
  description: string;
  children: ReactNode;
  onClose: () => void;
}) {
  const error = useContext(ProxyErrorContext);
  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog-content proxy-dialog">
          <Dialog.Title>{title}</Dialog.Title>
          <Dialog.Description>{description}</Dialog.Description>
          <Dialog.Close asChild>
            <button
              className="icon-button dialog-close"
              aria-label="关闭对话框"
            >
              <X size={18} />
            </button>
          </Dialog.Close>
          {error && (
            <div className="error-banner" role="alert">
              {error}
            </div>
          )}
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
export function ImportDialog({
  run,
  onClose,
}: {
  run: Run;
  onClose: () => void;
}) {
  const [payload, setPayload] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const file = useRef<HTMLInputElement>(null);
  return (
    <Modal
      title="导入节点"
      description="粘贴分享链接、Base64、Clash Meta YAML 或 sing-box JSON。不会读取旧版数据。"
      onClose={onClose}
    >
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (!payload.trim()) {
            setError("请先粘贴节点内容");
            return;
          }
          setBusy(true);
          const r = await run("import", { payload });
          setBusy(false);
          if (r.ok) {
            const result = r.result as { added: number; rejected: number };
            setError(
              `成功导入 ${result.added} 个，跳过 ${result.rejected} 个无效条目。`,
            );
            if (result.added > 0) onClose();
          }
        }}
      >
        <label className="field-label" htmlFor="node-import">
          节点内容
        </label>
        <textarea
          id="node-import"
          rows={8}
          value={payload}
          onChange={(e) => setPayload(e.target.value)}
          placeholder="vless://… 或 proxies: …"
          autoFocus
          spellCheck={false}
        />
        <input
          ref={file}
          hidden
          type="file"
          accept=".txt,.yaml,.yml,.json"
          onChange={async (e) => {
            const f = e.target.files?.[0];
            if (!f) return;
            if (f.size > 8 * 1024 * 1024) {
              setError("文件超过 8 MiB");
              return;
            }
            try {
              setPayload(await f.text());
            } catch {
              setError("文件读取失败");
            }
          }}
        />
        {error && (
          <p className="form-error" role="status">
            {error}
          </p>
        )}
        <div className="dialog-actions">
          <button
            type="button"
            className="button secondary"
            onClick={() => file.current?.click()}
          >
            <FileUp size={15} />
            选择文件
          </button>
          <button className="button primary" disabled={busy}>
            {busy ? "导入中…" : "导入节点"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
export function SubscriptionDialog({
  editing,
  run,
  onClose,
  updateAfterSave = false,
  quickImport = false,
}: {
  editing?: Subscription;
  run: Run;
  onClose: () => void;
  updateAfterSave?: boolean;
  quickImport?: boolean;
}) {
  const [sub, setSub] = useState<Subscription>(
    editing ?? {
      id: "",
      name: "",
      url: "",
      intervalHours: 0,
      lastUpdated: null,
      lastAttempt: null,
      error: null,
      format: "",
      uploadBytes: null,
      downloadBytes: null,
      totalBytes: null,
      expiresAt: null,
    },
  );
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const qrFile = useRef<HTMLInputElement>(null);
  const generatedName = useRef("");
  const applyUrl = (value: string) => {
    const match = value.match(/https?:\/\/[^\s<>"']+/i);
    if (!match) return setMessage("没有找到 HTTP/HTTPS 订阅链接");
    try {
      const parsed = new URL(match[0]);
      const suggestedName = parsed.hostname.replace(/^www\./, "");
      const previousGeneratedName = generatedName.current;
      setSub((current) => ({
        ...current,
        url: parsed.href,
        name:
          !current.name || current.name === previousGeneratedName
            ? suggestedName
            : current.name,
      }));
      generatedName.current = suggestedName;
      setMessage(`已识别：${parsed.hostname}`);
    } catch {
      setMessage("订阅链接格式无效");
    }
  };
  const readClipboard = async () => {
    try {
      applyUrl(await readText());
    } catch {
      setMessage("无法读取剪贴板，请检查权限或手动粘贴");
    }
  };
  useEffect(() => {
    if (quickImport) void readClipboard();
  }, []);
  return (
    <Modal
      title={editing ? "编辑订阅" : "添加订阅"}
      description={
        quickImport
          ? "识别剪贴板或二维码，保存后立即获取节点。"
          : "保存订阅地址后，点击更新获取节点。失败时保留上次成功的节点。"
      }
      onClose={onClose}
    >
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          const r = await run(
            "saveSubscription",
            { subscription: sub },
            "订阅已保存",
          );
          if (r.ok && updateAfterSave) {
            await run(
              "updateSubscription",
              { id: r.result as string },
              "订阅已保存，正在获取节点",
            );
          }
          setBusy(false);
          if (r.ok) onClose();
        }}
      >
        <label className="field-label" htmlFor="sub-name">
          订阅名称
        </label>
        <input
          id="sub-name"
          value={sub.name}
          required
          maxLength={100}
          onChange={(e) => setSub({ ...sub, name: e.target.value })}
        />
        <label className="field-label" htmlFor="sub-url">
          订阅 URL
        </label>
        <input
          id="sub-url"
          value={sub.url}
          type="url"
          required
          autoComplete="off"
          onChange={(e) => setSub({ ...sub, url: e.target.value })}
        />
        {sub.url && (
          <p className="subscription-preview">
            域名预览：
            {(() => {
              try {
                return new URL(sub.url).hostname;
              } catch {
                return "地址无效";
              }
            })()}
          </p>
        )}
        {quickImport && (
          <>
            <input
              ref={qrFile}
              hidden
              type="file"
              accept="image/png,image/jpeg,image/webp,image/bmp"
              onChange={async (event) => {
                const selected = event.target.files?.[0];
                if (!selected) return;
                try {
                  const bitmap = await createImageBitmap(selected);
                  const canvas = document.createElement("canvas");
                  canvas.width = bitmap.width;
                  canvas.height = bitmap.height;
                  const context = canvas.getContext("2d", {
                    willReadFrequently: true,
                  });
                  if (!context) throw new Error("canvas");
                  context.drawImage(bitmap, 0, 0);
                  const pixels = context.getImageData(
                    0,
                    0,
                    bitmap.width,
                    bitmap.height,
                  );
                  const jsQR = (await import("jsqr")).default;
                  const result = jsQR(pixels.data, bitmap.width, bitmap.height);
                  if (!result) throw new Error("qr");
                  applyUrl(result.data);
                } catch {
                  setMessage("没有从图片中识别到订阅二维码");
                } finally {
                  event.target.value = "";
                }
              }}
            />
            <div className="quick-subscription-actions">
              <button
                type="button"
                className="button secondary"
                onClick={() => void readClipboard()}
              >
                <ClipboardPaste size={15} />
                读取剪贴板
              </button>
              <button
                type="button"
                className="button secondary"
                onClick={() => qrFile.current?.click()}
              >
                <QrCode size={15} />
                扫描二维码图片
              </button>
            </div>
          </>
        )}
        {message && <p className="form-hint">{message}</p>}
        <label className="field-label" htmlFor="sub-interval">
          自动更新
        </label>
        <select
          id="sub-interval"
          value={sub.intervalHours}
          onChange={(e) =>
            setSub({ ...sub, intervalHours: Number(e.target.value) })
          }
        >
          {[0, 1, 6, 12, 24, 48, 168].map((n) => (
            <option key={n} value={n}>
              {n ? `每 ${n} 小时` : "仅手动更新"}
            </option>
          ))}
        </select>
        <div className="dialog-actions">
          <button className="button primary" disabled={busy}>
            {quickImport ? "保存并更新" : "保存订阅"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
export function NodeDetail({
  node,
  run,
  onClose,
}: {
  node: ProxyNode;
  run: Run;
  onClose: () => void;
}) {
  const [name, setName] = useState(node.name);
  const [pin, setPin] = useState(node.certSha256);
  const [show, setShow] = useState(false);
  return (
    <Modal
      title="节点详情"
      description={`${node.protocol.toUpperCase()} · ${coreName(node)}`}
      onClose={onClose}
    >
      <div className="detail-grid">
        {[
          ["服务器", `${node.address}:${node.port}`],
          ["传输", node.network || "tcp"],
          ["安全", node.security || "none"],
          ["SNI", node.sni || "—"],
          [
            "最近结果",
            node.lastError ??
              (node.delayMs !== null ? `${node.delayMs} ms` : "未测试"),
          ],
        ].map(([k, v]) => (
          <div key={k}>
            <span>{k}</span>
            <strong>{v}</strong>
          </div>
        ))}
      </div>
      {node.unsupportedReason && (
        <p className="form-error">{node.unsupportedReason}</p>
      )}
      <label className="field-label" htmlFor="rename-node">
        显示名称
      </label>
      <input
        id="rename-node"
        value={name}
        maxLength={100}
        onChange={(e) => setName(e.target.value)}
      />
      <>
        {!["anytls", "tuic"].includes(node.protocol) &&
          node.security === "tls" && (
            <>
              <label className="field-label" htmlFor="cert-pin">
                服务器证书 SHA-256 指纹
              </label>
              <input
                id="cert-pin"
                value={pin}
                placeholder="64 位十六进制指纹（可留空）"
                onChange={(e) => setPin(e.target.value)}
              />
              <p className="tiny muted">
                用于验证自签名证书；请从服务器管理者处获取正确指纹。
              </p>
              <button
                className="button secondary small"
                onClick={() =>
                  void run(
                    "setNodePin",
                    { id: node.id, fingerprint: pin },
                    "证书指纹已保存，下次连接生效",
                  )
                }
              >
                保存证书指纹
              </button>
            </>
          )}
      </>
      <button className="text-button reveal" onClick={() => setShow(!show)}>
        {show ? "隐藏敏感信息" : "显示节点原文（含凭据）"}
      </button>
      {show && (
        <textarea readOnly rows={5} value={node.raw} aria-label="节点原文" />
      )}
      <div className="dialog-actions">
        <button
          className="button secondary"
          onClick={() => void run("test", { ids: [node.id], mode: "http" })}
        >
          HTTP 测速
        </button>
        <button
          className="button primary"
          onClick={async () => {
            if (
              (await run("renameNode", { id: node.id, name }, "节点已更新")).ok
            )
              onClose();
          }}
        >
          保存名称
        </button>
      </div>
    </Modal>
  );
}
