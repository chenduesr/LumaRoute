import { chromium, expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { createServer } from "node:net";
import { spawn } from "node:child_process";
import { resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import QRCode from "qrcode";

const artifactRoot = resolve("test-artifacts");
await mkdir(artifactRoot, { recursive: true });
const qrFixture = resolve(artifactRoot, "subscription-qr.png");
await QRCode.toFile(qrFixture, "https://qr.example/sub", {
  width: 320,
  margin: 2,
});
const socket = createServer();
await new Promise((done) => socket.listen(0, "127.0.0.1", done));
const port = socket.address().port;
await new Promise((done) => socket.close(done));

const settings = {
  httpPort: 7890,
  socksPort: 7891,
  proxyMode: "rule",
  routingMode: "smart",
  captureMode: "none",
  tunIpv6: true,
  tunBypassLan: true,
  tunMtu: 1500,
  bypassMainland: true,
  startOnBoot: false,
  autoConnect: false,
  autoRecoverConnection: true,
  updateSubscriptionsOnLaunch: true,
  minimizeToTray: true,
  directDomains: "",
  directIps: "",
  proxyDomains: "",
  proxyIps: "",
  blockDomains: "",
  blockIps: "",
  customDns: false,
  fakeDns: false,
  splitDns: true,
  domesticDns: "223.5.5.5",
  foreignDns: "1.1.1.1",
  dohDns: "https://dns.google/dns-query",
  dotDns: "",
  testUrl: "https://www.gstatic.com/generate_204",
  testTimeout: 10,
  testConcurrency: 8,
  testRetries: 1,
  downloadBytes: 1048576,
  updateRepo: "chenduesr/LumaRoute",
  autoCheckUpdates: false,
};
const snapshot = {
  version: "0.3.2",
  isolated: true,
  data: {
    version: 2,
    settings,
    nodes: [
      {
        id: "fixture-node",
        name: "示例节点",
        address: "example.test",
        port: 443,
        protocol: "vless",
        raw: "",
        subscriptionId: "fixture-sub",
        userId: "",
        password: "",
        method: "",
        network: "tcp",
        security: "tls",
        flow: "",
        host: "",
        path: "",
        serviceName: "",
        sni: "example.test",
        fingerprint: "chrome",
        publicKey: "",
        shortId: "",
        spiderX: "",
        alpn: "",
        obfs: "",
        obfsPassword: "",
        allowInsecure: false,
        certSha256: "",
        unsupportedReason: null,
        delayMs: 58,
        downloadMbps: null,
        lastError: null,
        lastTested: null,
        lastTestMode: "",
      },
    ],
    subscriptions: [
      {
        id: "fixture-sub",
        name: "示例订阅",
        url: "https://example.test/subscription",
        intervalHours: 24,
        lastUpdated: new Date().toISOString(),
        lastAttempt: new Date().toISOString(),
        error: null,
        format: "Clash Meta YAML",
        uploadBytes: 1024,
        downloadBytes: 2048,
        totalBytes: 10485760,
        expiresAt: "2030-01-01T00:00:00Z",
      },
    ],
    activeNodeId: "fixture-node",
    trafficDate: "2026-09-15",
    todayUpload: 0,
    todayDownload: 0,
  },
  connection: {
    status: "disconnected",
    nodeId: null,
    since: null,
    captureMode: "none",
    error: null,
  },
  job: {
    running: false,
    kind: "startup",
    completed: 1,
    total: 1,
    message: "当前已是最新版本（发布版本 v0.3.2）",
  },
  traffic: {
    uploadSpeed: 0,
    downloadSpeed: 0,
    todayUpload: 0,
    todayDownload: 0,
  },
  core: {
    version: "Xray 26.3.27",
    installed: true,
    geoReady: true,
    singboxVersion: "sing-box 1.14.1",
    singboxInstalled: true,
  },
  logs: [],
  storageError: null,
};

const child = spawn(
  process.execPath,
  [
    resolve("node_modules/vite/bin/vite.js"),
    "preview",
    "--host",
    "127.0.0.1",
    "--port",
    String(port),
  ],
  { windowsHide: true, stdio: "ignore" },
);
let browser;
try {
  const until = Date.now() + 30000;
  while (Date.now() < until) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}`);
      if (response.ok) break;
    } catch {}
    await delay(150);
  }
  browser = await chromium.launch({ headless: true, channel: "msedge" });
  const context = await browser.newContext({
    viewport: { width: 1280, height: 860 },
  });
  await context.addInitScript((initialSnapshot) => {
    globalThis.isTauri = true;
    Object.defineProperty(navigator, "clipboard", {
      value: { readText: async () => "https://clipboard.example/sub" },
      configurable: true,
    });
    window.__LUMAROUTE_TEST_ACTIONS__ = [];
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: () => {},
    };
    const callbacks = new Map();
    let callbackId = 0;
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" } },
      transformCallback: (callback) => {
        const id = ++callbackId;
        callbacks.set(id, callback);
        return id;
      },
      invoke: async (command, payload) => {
        if (command === "proxy_snapshot") return initialSnapshot;
        if (command === "proxy_action") {
          window.__LUMAROUTE_TEST_ACTIONS__.push({
            action: payload?.action,
            args: payload?.args,
          });
          return payload?.action === "saveSubscription"
            ? "fixture-new-sub"
            : null;
        }
        if (command === "plugin:clipboard-manager|read_text")
          return "https://clipboard.example/sub";
        if (command === "plugin:event|listen") return callbackId;
        if (command === "plugin:event|unlisten") return null;
        if (command === "plugin:window|scale_factor") return 1;
        if (command === "plugin:window|inner_size")
          return { width: innerWidth, height: innerHeight };
        if (command === "plugin:window|outer_position") return { x: 40, y: 40 };
        if (command === "plugin:window|is_maximized") return false;
        return null;
      },
    };
  }, snapshot);
  const page = await context.newPage();
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.goto(`http://127.0.0.1:${port}`);
  await delay(300);
  if (!(await page.getByRole("heading", { name: "未连接" }).count())) {
    throw new Error(
      `Application did not render. Errors: ${pageErrors.join(" | ")} Body: ${await page.locator("body").innerText()}`,
    );
  }
  await expect(page.locator(".simple-brand strong")).toHaveText("LumaRoute");
  await expect(page.locator(".simple-shell")).toBeVisible();
  await expect(page.getByText("简洁模式", { exact: true })).toBeVisible();
  await expect(page.getByText("双核心代理客户端", { exact: true })).toHaveCount(
    0,
  );
  await expect(page.getByText(/当前已是最新版本/)).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "未连接" })).toBeVisible();
  await page.getByRole("button", { name: "选择流量接管方式" }).click();
  await page.getByRole("menuitem", { name: /TUN 模式/ }).click();
  await page.screenshot({
    path: resolve(artifactRoot, "simple-default-1280x860.png"),
  });
  await page.getByRole("button", { name: "完整模式" }).click();
  await expect(
    page.getByRole("heading", { name: "连接，从容一点。" }),
  ).toBeVisible();
  await page.screenshot({
    path: resolve(artifactRoot, "full-mode-1280x860.png"),
  });
  await page.getByRole("button", { name: "切换到简洁模式" }).click();
  await expect(page.locator(".simple-shell")).toBeVisible();
  await expect(page.getByRole("heading", { name: "未连接" })).toBeVisible();
  expect(
    await page.evaluate(
      () =>
        JSON.parse(localStorage.getItem("lumaroute.settings.v1")).simpleMode,
    ),
  ).toBe(true);
  const nodeSelect = page.getByRole("combobox", {
    name: "当前节点",
    exact: true,
  });
  await expect(nodeSelect).toHaveValue("fixture-node");
  await expect(nodeSelect).toContainText("58 ms");
  await page.screenshot({
    path: resolve(artifactRoot, "simple-mode-1280x860.png"),
  });
  await page.getByRole("button", { name: "添加订阅" }).click();
  await expect(page.getByLabel("订阅名称")).toHaveValue("clipboard.example");
  await expect(page.getByText("域名预览：clipboard.example")).toBeVisible();
  await page.getByRole("button", { name: "保存并更新" }).click();
  await expect(page.getByRole("dialog")).toBeHidden();
  await page.getByRole("button", { name: "添加订阅" }).click();
  await page
    .locator('input[type="file"][accept*="image/png"]')
    .setInputFiles(qrFixture);
  await expect(page.getByLabel("订阅名称")).toHaveValue("qr.example");
  await expect(page.getByLabel("订阅 URL")).toHaveValue(
    "https://qr.example/sub",
  );
  await expect(page.getByText("域名预览：qr.example")).toBeVisible();
  await page.getByRole("button", { name: "保存并更新" }).click();
  await page.getByRole("button", { name: "测试当前节点，HTTP 延迟" }).click();
  await page.getByRole("button", { name: "选择测速范围和方式" }).click();
  await page.getByRole("menuitem", { name: "全部节点 · TCP 延迟" }).click();
  await page.setViewportSize({ width: 900, height: 660 });
  await expect
    .poll(() =>
      page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
    )
    .toBe(true);
  await page.screenshot({
    path: resolve(artifactRoot, "simple-mode-900x660.png"),
  });
  await page.getByRole("button", { name: "更新全部订阅" }).click();
  await expect
    .poll(() =>
      page.evaluate(() =>
        window.__LUMAROUTE_TEST_ACTIONS__.map((item) => item.action),
      ),
    )
    .toEqual(
      expect.arrayContaining([
        "saveSubscription",
        "updateSubscription",
        "test",
        "updateAllSubscriptions",
        "setCaptureMode",
      ]),
    );
  await page.getByRole("button", { name: "完整模式" }).click();
  await expect(page.getByRole("navigation", { name: "主导航" })).toBeVisible();
  expect(pageErrors).toEqual([]);
  console.log(
    "PASS simple mode subscription add/update, latency actions and 900x660 layout",
  );
} finally {
  if (browser) await browser.close();
  child.kill();
}
