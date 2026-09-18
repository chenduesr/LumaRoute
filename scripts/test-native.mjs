import { chromium, expect } from "@playwright/test";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { createServer as tcpServer, createConnection } from "node:net";
import { createSocket } from "node:dgram";
import { spawn } from "node:child_process";
import { resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import QRCode from "qrcode";

const artifactRoot = resolve("test-artifacts");
await mkdir(artifactRoot, { recursive: true });
const report = { started: new Date().toISOString(), checks: [], errors: [] };
const check = (name) => {
  report.checks.push(name);
  console.log("PASS", name);
};
const port = async () => {
  for (let i = 0; i < 100; i++) {
    const candidate = 12000 + Math.floor(Math.random() * 7000);
    const s = tcpServer();
    try {
      await new Promise((resolveListen, reject) => {
        s.once("error", reject);
        s.listen(candidate, "127.0.0.1", resolveListen);
      });
      await new Promise((resolveClose) => s.close(resolveClose));
      return candidate;
    } catch {
      s.close();
    }
  }
  throw new Error("No stable TCP test port available");
};
const udpPort = async () => {
  for (let i = 0; i < 100; i++) {
    const s = createSocket("udp4");
    await new Promise((r) =>
      s.bind(20000 + Math.floor(Math.random() * 15000), "127.0.0.1", r),
    );
    const n = s.address().port;
    const tcp = tcpServer();
    try {
      await new Promise((r, reject) => {
        tcp.once("error", reject);
        tcp.listen(n, "127.0.0.1", r);
      });
      await new Promise((r) => tcp.close(r));
      s.close();
      return n;
    } catch {
      s.close();
    }
  }
  throw new Error("No dual TCP/UDP test port available");
};
let httpPort = await port(),
  socksPort = await udpPort();
let subscriptionBody = "",
  subscriptionFailure = false;
const server = createServer((req, res) => {
  if (req.url === "/subscription") {
    res.writeHead(subscriptionFailure ? 503 : 200, {
      "Subscription-Userinfo":
        "upload=1024; download=2048; total=10485760; expire=1893456000",
    });
    res.end(subscriptionFailure ? "temporary failure" : subscriptionBody);
  } else {
    res.writeHead(200, { "Content-Type": "text/plain" });
    res.end("native-fixture-data".repeat(8192));
  }
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const fixturePort = server.address().port;
if (httpPort === fixturePort || httpPort === socksPort) httpPort = await port();
while (socksPort === fixturePort || socksPort === httpPort)
  socksPort = await udpPort();
const qrFixture = resolve(artifactRoot, "native-subscription-qr.png");
await QRCode.toFile(qrFixture, `http://127.0.0.1:${fixturePort}/subscription`, {
  width: 320,
  margin: 2,
});
server.on("connect", (req, socket, head) => {
  if (req.url !== `127.0.0.1:${fixturePort}`) {
    socket.end("HTTP/1.1 403 Forbidden\r\n\r\n");
    return;
  }
  const upstream = createConnection(
    { host: "127.0.0.1", port: fixturePort },
    () => {
      socket.write("HTTP/1.1 200 Connection Established\r\n\r\n");
      if (head.length) upstream.write(head);
      socket.pipe(upstream);
      upstream.pipe(socket);
    },
  );
  upstream.on("error", () => socket.destroy());
  socket.on("error", () => upstream.destroy());
  socket.on("close", () => upstream.destroy());
});
let child, browser, page, dataRoot;
let safeToAct = false;
const debugPort = Number(process.env.LUMAROUTE_CDP_PORT || 9223);
const releaseMode = process.argv.includes("--release");
try {
  if (!process.argv.includes("--attach")) {
    dataRoot = await mkdtemp(resolve(artifactRoot, "native-run-"));
    await writeFile(
      resolve(dataRoot, "profile.json"),
      JSON.stringify({
        version: 1,
        settings: {
          systemProxy: false,
          autoConnect: false,
          minimizeToTray: false,
        },
      }),
    );
    child = spawn(
      releaseMode
        ? resolve("src-tauri/target/release/lumaroute.exe")
        : process.execPath,
      releaseMode ? [] : ["scripts/tauri.mjs", "dev", "--no-watch"],
      {
        windowsHide: true,
        stdio: ["ignore", "pipe", "pipe"],
        env: {
          ...process.env,
          LUMAROUTE_TEST_ROOT: dataRoot,
          WEBVIEW2_USER_DATA_FOLDER: resolve(dataRoot, "webview2"),
          WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${debugPort}`,
        },
      },
    );
    let log = "";
    child.stdout.on("data", (d) => {
      log += d;
    });
    child.stderr.on("data", (d) => {
      log += d;
    });
    child.on(
      "exit",
      () => void writeFile(resolve(artifactRoot, "native-launch.log"), log),
    );
  }
  const until = Date.now() + 180000;
  while (Date.now() < until) {
    if (child && child.exitCode !== null)
      throw new Error(`Native process exited: ${child.exitCode}`);
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${debugPort}`);
      break;
    } catch {
      await delay(500);
    }
  }
  if (!browser) throw new Error("Native WebView2 did not become available");
  page = browser.contexts()[0].pages()[0];
  const pageErrors = [];
  page.on("pageerror", (e) => pageErrors.push(e.message));
  await page.waitForFunction(() => !!window.__TAURI_INTERNALS__);
  const action = (action, args = {}) =>
    page.evaluate(
      ({ action, args }) =>
        window.__TAURI_INTERNALS__.invoke("proxy_action", { action, args }),
      { action, args },
    );
  const snapshot = () =>
    page.evaluate(() => window.__TAURI_INTERNALS__.invoke("proxy_snapshot"));
  const initial = await snapshot();
  if (!initial.isolated) throw new Error("Refusing to test a non-isolated app");
  if (initial.data.nodes.length || initial.data.subscriptions.length)
    throw new Error("Tests require an empty isolated profile");
  safeToAct = true;
  await expect(page.locator(".simple-brand strong")).toHaveText("LumaRoute");
  await expect(page.locator(".simple-shell")).toBeVisible();
  await expect(page.getByText("简洁模式", { exact: true })).toBeVisible();
  await expect(page.getByText("双核心代理客户端", { exact: true })).toHaveCount(
    0,
  );
  await expect(page.getByRole("heading", { name: "未连接" })).toBeVisible();
  expect(
    initial.core.installed &&
      initial.core.singboxInstalled &&
      initial.core.geoReady,
  ).toBe(true);
  check("Default simple mode, LumaRoute brand and bundled core resources");
  await page.getByRole("button", { name: "完整模式" }).click();
  await expect(
    page.getByRole("heading", { name: "连接，从容一点。" }),
  ).toBeVisible();
  const windowInvoke = (command, payload = {}) =>
    page.evaluate(
      ({ command, payload }) =>
        window.__TAURI_INTERNALS__.invoke(`plugin:window|${command}`, {
          label: "main",
          ...payload,
        }),
      { command, payload },
    );
  await page.locator(".drag-region").dispatchEvent("dblclick");
  await expect.poll(() => windowInvoke("is_maximized")).toBe(true);
  await page.locator(".drag-region").dispatchEvent("dblclick");
  await expect.poll(() => windowInvoke("is_maximized")).toBe(false);
  await windowInvoke("set_size", {
    value: { Logical: { width: 1160, height: 780 } },
  });
  await expect
    .poll(() =>
      page.evaluate(() => {
        const saved = JSON.parse(
          localStorage.getItem("lumaroute.window.full.v1") || "null",
        );
        return (
          saved &&
          Math.abs(saved.width - 1160) <= 10 &&
          Math.abs(saved.height - 780) <= 10
        );
      }),
    )
    .toBe(true);
  await page.getByRole("button", { name: "切换到简洁模式" }).click();
  await expect(page.locator(".simple-shell")).toBeVisible();
  await delay(500);
  await windowInvoke("set_size", {
    value: { Logical: { width: 980, height: 700 } },
  });
  await expect
    .poll(() =>
      page.evaluate(() => {
        const saved = JSON.parse(
          localStorage.getItem("lumaroute.window.simple.v1") || "null",
        );
        return (
          saved &&
          Math.abs(saved.width - 980) <= 10 &&
          Math.abs(saved.height - 700) <= 10
        );
      }),
    )
    .toBe(true);
  await page.getByRole("button", { name: "完整模式" }).click();
  await expect
    .poll(async () => {
      const size = await windowInvoke("inner_size");
      const scale = await windowInvoke("scale_factor");
      return Math.abs(size.width / scale - 1160) <= 10;
    })
    .toBe(true);
  check(
    "Window position/size memory per mode, DPI conversion and titlebar double-click maximize",
  );
  const nav = (name) =>
    page
      .getByRole("navigation", { name: "主导航" })
      .getByRole("button", { name, exact: true });
  await nav("设置").click();
  await page.getByLabel("HTTP 端口", { exact: true }).fill(String(httpPort));
  await page.getByLabel("SOCKS 端口", { exact: true }).fill(String(httpPort));
  await page.getByRole("button", { name: "保存设置", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("不能相同");
  await page.getByLabel("SOCKS 端口", { exact: true }).fill(String(socksPort));
  await page.getByLabel("代理模式", { exact: true }).selectOption("global");
  await page.getByRole("tab", { name: "测速", exact: true }).click();
  await page
    .getByLabel("测速地址", { exact: true })
    .fill(`http://127.0.0.1:${fixturePort}/download`);
  await page.getByRole("button", { name: "保存设置", exact: true }).click();
  await expect
    .poll(async () => (await snapshot()).data.settings.httpPort)
    .toBe(httpPort);
  expect((await snapshot()).data.settings.systemProxy).toBe(false);
  const encryptedProfile = await readFile(resolve(dataRoot, "profile.json"));
  expect(
    encryptedProfile.subarray(0, 18).equals(Buffer.from("LUMAROUTE-DPAPI-1\0")),
  ).toBe(true);
  check("Settings validation and persistence through Rust");
  await page.getByRole("tab", { name: "外观", exact: true }).click();
  await page.getByLabel("主题", { exact: true }).selectOption("light");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.screenshot({
    path: resolve(artifactRoot, "native-settings-light.png"),
  });
  await page.getByLabel("主题", { exact: true }).selectOption("dark");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  expect((await snapshot()).data.settings.httpPort).toBe(httpPort);
  check("Light/dark theme and reload persistence");
  await nav("节点").click();
  await page.getByRole("button", { name: "导入节点", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog
    .getByLabel("节点内容")
    .fill(`http://127.0.0.1:${fixturePort}#Native-Fixture`);
  await dialog.getByRole("button", { name: "导入节点", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "选择 Native-Fixture" }),
  ).toBeVisible();
  const importedNode = (await snapshot()).data.nodes.find(
    (node) => node.name === "Native-Fixture",
  );
  await action("select", { id: importedNode.id });
  await expect(
    page.getByRole("button", { name: "选择 Native-Fixture" }),
  ).toHaveAttribute("aria-pressed", "true");
  check("Backend state events update the UI without one-second polling");
  await page.getByRole("button", { name: "Native-Fixture的操作" }).click();
  await page.getByRole("menuitem", { name: "节点详情" }).click();
  await expect(dialog.getByRole("heading", { name: "节点详情" })).toBeVisible();
  await dialog.getByLabel("显示名称").fill("Native-Renamed");
  await dialog.getByRole("button", { name: "保存名称" }).click();
  await expect(dialog).toHaveCount(0);
  await page.getByLabel("测速方式").selectOption("http");
  await page.getByRole("button", { name: "测试当前列表" }).click();
  await expect
    .poll(async () => (await snapshot()).job.message, { timeout: 20000 })
    .toContain("1 / 1 可用");
  await page.locator(".latency").focus();
  await expect(page.locator(".latency")).toContainText("ms");
  await expect(page.getByRole("tooltip")).toBeVisible();
  await page.mouse.move(0, 0);
  await page.screenshot({
    path: resolve(artifactRoot, "native-nodes-dark.png"),
  });
  check(
    "Import, select, rename, Radix Dialog/Dropdown/Tooltip and real HTTP test",
  );
  await page.getByRole("button", { name: "连接", exact: true }).click();
  await expect
    .poll(async () => (await snapshot()).connection.status, { timeout: 20000 })
    .toBe("connected");
  expect((await snapshot()).connection.systemProxy).toBe(false);
  await nav("概览").click();
  await expect(
    page.getByRole("heading", { name: "已连接并验证" }),
  ).toBeVisible();
  await page.screenshot({
    path: resolve(artifactRoot, "native-connected.png"),
  });
  await page.getByRole("button", { name: "断开连接", exact: true }).click();
  await expect
    .poll(async () => (await snapshot()).connection.status)
    .toBe("disconnected");
  check("Native UI connect/disconnect without system proxy");
  subscriptionBody = `socks://127.0.0.1:${fixturePort}#Subscription-Fixture`;
  await page.getByRole("button", { name: "切换到简洁模式" }).click();
  await page.getByRole("button", { name: "添加订阅", exact: true }).click();
  await expect(dialog.locator(".form-hint")).toBeVisible();
  await dialog
    .locator('input[type="file"][accept*="image/png"]')
    .setInputFiles(qrFixture);
  await expect(dialog.getByLabel("订阅 URL")).toHaveValue(
    `http://127.0.0.1:${fixturePort}/subscription`,
  );
  await expect(dialog.getByText("域名预览：127.0.0.1")).toBeVisible();
  await dialog.getByLabel("订阅名称").fill("本地测试订阅");
  await dialog.getByRole("button", { name: "保存并更新", exact: true }).click();
  await expect
    .poll(async () => (await snapshot()).data.nodes.length, { timeout: 15000 })
    .toBe(2);
  await expect.poll(async () => (await snapshot()).job.running).toBe(false);
  expect((await snapshot()).data.subscriptions[0].id).toBeTruthy();
  expect((await snapshot()).data.subscriptions[0].format).toBe("分享链接");
  expect((await snapshot()).data.subscriptions[0].totalBytes).toBe(10485760);
  await page.getByRole("button", { name: "测试当前节点，HTTP 延迟" }).click();
  await expect
    .poll(async () => (await snapshot()).job.message, { timeout: 20000 })
    .toContain("1 / 1 可用");
  await page.getByRole("button", { name: "选择测速范围和方式" }).click();
  await page.getByRole("menuitem", { name: "全部节点 · TCP 延迟" }).click();
  await expect
    .poll(async () => (await snapshot()).job.message, { timeout: 20000 })
    .toContain("2 / 2 可用");
  await page.getByRole("button", { name: "添加订阅", exact: true }).click();
  await expect(dialog.locator(".form-hint")).toBeVisible();
  await dialog.getByLabel("订阅名称").fill("重复订阅");
  await dialog
    .getByLabel("订阅 URL")
    .fill(`http://127.0.0.1:${fixturePort}/subscription`);
  await dialog.getByRole("button", { name: "保存并更新", exact: true }).click();
  await expect(dialog.getByRole("alert")).toContainText("已经存在");
  await dialog.getByRole("button", { name: "关闭对话框" }).click();
  await page.getByRole("button", { name: "关闭错误提示" }).click();
  check(
    "Quick clipboard/QR subscription, duplicate warning and current/all latency tests",
  );
  await page.getByRole("button", { name: "完整模式" }).click();
  await nav("订阅").click();
  await page.getByRole("button", { name: "添加订阅", exact: true }).click();
  await expect(
    dialog.getByText("识别剪贴板或二维码，保存后立即获取节点。"),
  ).toBeVisible();
  await dialog.getByRole("button", { name: "关闭对话框" }).click();
  await expect(page.getByText("格式：分享链接", { exact: true })).toBeVisible();
  await expect(page.getByText("订阅流量", { exact: true })).toBeVisible();
  await expect(page.getByText(/到期时间：/)).toBeVisible();
  subscriptionFailure = true;
  await page.getByRole("button", { name: "立即更新" }).click();
  await expect
    .poll(async () => (await snapshot()).data.subscriptions[0].error, {
      timeout: 15000,
    })
    .toBeTruthy();
  expect((await snapshot()).data.nodes.length).toBe(2);
  await page.screenshot({
    path: resolve(artifactRoot, "native-subscription-failure.png"),
  });
  check("Subscription fetch and failed-update data protection");
  await nav("日志").click();
  await page.getByRole("button", { name: "运行诊断" }).click();
  await expect
    .poll(async () => (await snapshot()).job.running, { timeout: 20000 })
    .toBe(false);
  expect((await snapshot()).diagnostics.checks).toHaveLength(11);
  await expect(page.locator(".diagnostic-report")).toContainText(
    "核心文件与版本",
  );
  await expect(page.locator(".diagnostic-report")).toContainText(
    "DNS 泄漏风险",
  );
  await expect(page.locator(".diagnostic-report")).toContainText("IPv6 可用性");
  const diagnostics = await action("exportDiagnostics");
  expect(diagnostics.endsWith(".zip")).toBe(true);
  await page.screenshot({ path: resolve(artifactRoot, "native-logs.png") });
  await nav("设置").click();
  await page.getByRole("tab", { name: "更新", exact: true }).click();
  await expect(page.getByLabel("新版发布源", { exact: true })).toHaveValue(
    "chenduesr/LumaRoute",
  );
  await page.getByRole("button", { name: "检查应用更新", exact: true }).click();
  await expect
    .poll(async () => (await snapshot()).job.running, { timeout: 15000 })
    .toBe(false);
  expect((await snapshot()).job.message.length).toBeGreaterThan(0);
  check("Diagnostics export and configured app-update source");
  if (process.argv.includes("--check-core-updates")) {
    for (const kind of ["xray", "singbox"]) {
      await action("updateCore", { kind });
      await expect
        .poll(async () => (await snapshot()).job.running, { timeout: 150000 })
        .toBe(false);
      expect((await snapshot()).job.message).toContain("已更新至");
    }
    await action("updateCore", { kind: "xray", geoOnly: true });
    await expect
      .poll(async () => (await snapshot()).job.running, { timeout: 150000 })
      .toBe(false);
    expect((await snapshot()).job.message).toContain("规则数据 已更新至");
    check(
      "Official core and geo updates with SHA-256 verification in isolated storage",
    );
  }
  const original = await page.evaluate(() => ({
    width: innerWidth,
    height: innerHeight,
  }));
  await page.setViewportSize({ width: 900, height: 660 });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: resolve(artifactRoot, "native-minimum-size.png"),
  });
  await page.setViewportSize(original);
  check("900 x 660 layout without horizontal overflow");
  expect(pageErrors).toEqual([]);
  check("No JavaScript runtime errors");
  report.finished = new Date().toISOString();
  report.passed = true;
} catch (e) {
  report.errors.push(String(e));
  report.passed = false;
  if (page)
    await page
      .screenshot({ path: resolve(artifactRoot, "native-failure.png") })
      .catch(() => {});
  throw e;
} finally {
  await writeFile(
    resolve(
      artifactRoot,
      releaseMode
        ? "native-release-verification.json"
        : "native-verification.json",
    ),
    JSON.stringify(report, null, 2),
  );
  if (page && safeToAct) {
    await page
      .evaluate(() =>
        window.__TAURI_INTERNALS__.invoke("proxy_action", {
          action: "disconnect",
          args: {},
        }),
      )
      .catch(() => {});
  }
  if (child && page && safeToAct)
    await page
      .getByRole("button", { name: "关闭窗口", exact: true })
      .click()
      .catch(() => {});
  await browser?.close().catch(() => {});
  if (child) {
    await delay(1000);
    if (child.exitCode === null) {
      await new Promise((r) => {
        const kill = spawn(
          "taskkill",
          ["/PID", String(child.pid), "/T", "/F"],
          { windowsHide: true, stdio: "ignore" },
        );
        kill.on("exit", r);
      });
    }
  }
  server.closeAllConnections();
  await new Promise((r) => server.close(r));
}
