import { invoke, isTauri } from "@tauri-apps/api/core";
export interface ProxySettings {
  httpPort: number;
  socksPort: number;
  proxyMode: string;
  routingMode: string;
  systemProxy: boolean;
  bypassMainland: boolean;
  startOnBoot: boolean;
  autoConnect: boolean;
  minimizeToTray: boolean;
  directDomains: string;
  directIps: string;
  proxyDomains: string;
  proxyIps: string;
  blockDomains: string;
  blockIps: string;
  customDns: boolean;
  fakeDns: boolean;
  splitDns: boolean;
  domesticDns: string;
  foreignDns: string;
  dohDns: string;
  dotDns: string;
  testUrl: string;
  testTimeout: number;
  testConcurrency: number;
  testRetries: number;
  downloadBytes: number;
  updateRepo: string;
  autoCheckUpdates: boolean;
}
export interface ProxyNode {
  id: string;
  name: string;
  address: string;
  port: number;
  protocol: string;
  raw: string;
  subscriptionId: string | null;
  userId: string;
  password: string;
  method: string;
  network: string;
  security: string;
  flow: string;
  host: string;
  path: string;
  serviceName: string;
  sni: string;
  fingerprint: string;
  publicKey: string;
  shortId: string;
  spiderX: string;
  alpn: string;
  obfs: string;
  obfsPassword: string;
  allowInsecure: boolean;
  certSha256: string;
  unsupportedReason: string | null;
  delayMs: number | null;
  downloadMbps: number | null;
  lastError: string | null;
  lastTested: string | null;
  lastTestMode: string;
}
export interface Subscription {
  id: string;
  name: string;
  url: string;
  intervalHours: number;
  lastUpdated: string | null;
  lastAttempt: string | null;
  error: string | null;
}
export interface ProxyLog {
  timestamp: string;
  level: string;
  source: string;
  message: string;
}
export interface Snapshot {
  isolated: boolean;
  data: {
    version: number;
    settings: ProxySettings;
    nodes: ProxyNode[];
    subscriptions: Subscription[];
    activeNodeId: string | null;
    trafficDate: string;
    todayUpload: number;
    todayDownload: number;
  };
  connection: {
    status: string;
    nodeId: string | null;
    since: string | null;
    systemProxy: boolean;
    error: string | null;
  };
  job: {
    running: boolean;
    kind: string;
    completed: number;
    total: number;
    message: string;
  };
  traffic: {
    uploadSpeed: number;
    downloadSpeed: number;
    todayUpload: number;
    todayDownload: number;
  };
  core: {
    version: string;
    installed: boolean;
    geoReady: boolean;
    singboxVersion: string;
    singboxInstalled: boolean;
  };
  logs: ProxyLog[];
  storageError: string | null;
}
export const readSnapshot = () => invoke<Snapshot>("proxy_snapshot");
export const proxyAction = <T = unknown>(
  action: string,
  args: Record<string, unknown> = {},
) => {
  if (!isTauri())
    return Promise.reject(new Error("此功能需要在 Windows 桌面应用中运行。"));
  return invoke<T>("proxy_action", { action, args });
};
export const bytes = (value: number) =>
  value >= 1048576
    ? `${(value / 1048576).toFixed(2)} MiB`
    : value >= 1024
      ? `${(value / 1024).toFixed(1)} KiB`
      : `${value} B`;
export const coreName = (node: ProxyNode) =>
  ["anytls", "tuic"].includes(node.protocol) ? "Xray + sing-box" : "Xray";
export const dateText = (value: string | null) =>
  value ? new Date(value).toLocaleString("zh-CN") : "从未";
