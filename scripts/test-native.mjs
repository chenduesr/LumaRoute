import { chromium, expect } from "@playwright/test";
import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { createServer as tcpServer, createConnection } from "node:net";
import { createSocket } from "node:dgram";
import { spawn } from "node:child_process";
import { resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

const artifactRoot = resolve("test-artifacts");
await mkdir(artifactRoot, { recursive: true });
const report = { started: new Date().toISOString(), checks: [], errors: [] };
const check = (name) => {
  report.checks.push(name);
  console.log("PASS", name);
};
const port = async () => {
  const s = tcpServer();
  await new Promise((r) => s.listen(0, "127.0.0.1", r));
  const n = s.address().port;
  await new Promise((r) => s.close(r));
  return n;
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
const httpPort = await port(),
  socksPort = await udpPort();
let subscriptionBody = "",
  subscriptionFailure = false;
const server = createServer((req, res) => {
  if (req.url === "/subscription") {
    res.writeHead(subscriptionFailure ? 503 : 200);
    res.end(subscriptionFailure ? "temporary failure" : subscriptionBody);
  } else {
    res.writeHead(200, { "Content-Type": "text/plain" });
    res.end("native-fixture-data".repeat(8192));
  }
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const fixturePort = server.address().port;
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
let child, browser, page;
let safeToAct = false;
const debugPort = Number(process.env.MYRAY_CDP_PORT || 9223);
const releaseMode = process.argv.includes("--release");
try {
  if (!process.argv.includes("--attach")) {
    const dataRoot = await mkdtemp(resolve(artifactRoot, "native-run-"));
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
        ? resolve("src-tauri/target/release/myray-lite-tauri.exe")
        : process.execPath,
      releaseMode ? [] : ["scripts/tauri.mjs", "dev", "--no-watch"],
      {
        windowsHide: true,
        stdio: ["ignore", "pipe", "pipe"],
        env: {
          ...process.env,
          MYRAY_TEST_ROOT: dataRoot,
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
  await expect(
    page.getByRole("heading", { name: "连接，从容一点。" }),
  ).toBeVisible();
  expect(
    initial.core.installed &&
      initial.core.singboxInstalled &&
      initial.core.geoReady,
  ).toBe(true);
  check("Native Tauri IPC and bundled Xray/sing-box resources");
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
  await page.getByRole("button", { name: "选择 Native-Fixture" }).click();
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
    page.getByRole("heading", { name: "本地代理已就绪" }),
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
  await nav("订阅").click();
  await page.getByRole("button", { name: "添加订阅", exact: true }).click();
  await dialog.getByLabel("订阅名称").fill("本地测试订阅");
  await dialog
    .getByLabel("订阅 URL")
    .fill(`http://127.0.0.1:${fixturePort}/subscription`);
  await dialog.getByRole("button", { name: "保存订阅", exact: true }).click();
  await page.getByRole("button", { name: "立即更新" }).click();
  await expect
    .poll(async () => (await snapshot()).data.nodes.length, { timeout: 15000 })
    .toBe(2);
  subscriptionFailure = true;
  await expect.poll(async () => (await snapshot()).job.running).toBe(false);
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
  await expect(page.locator(".log-panel")).toContainText("正常");
  const diagnostics = await action("exportDiagnostics");
  expect(diagnostics.endsWith(".zip")).toBe(true);
  await page.screenshot({ path: resolve(artifactRoot, "native-logs.png") });
  await nav("设置").click();
  await page.getByRole("tab", { name: "更新", exact: true }).click();
  await page.getByRole("button", { name: "检查应用更新", exact: true }).click();
  await expect
    .poll(async () => (await snapshot()).job.message)
    .toContain("请先配置新版");
  check("Diagnostics export and separate app-update source guard");
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
  await page.setViewportSize({ width: 820, height: 620 });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: resolve(artifactRoot, "native-minimum-size.png"),
  });
  await page.setViewportSize(original);
  check("820 x 620 layout without horizontal overflow");
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
