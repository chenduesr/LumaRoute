# LumaRoute · Windows 双核心代理客户端

使用 **Tauri 2 + React + TypeScript + Vite + Tailwind CSS + Radix UI**；业务逻辑由 Rust 实现。当前版本 **0.3.3**。

LumaRoute 使用独立应用标识和数据目录，配置、订阅与节点均从空白状态开始。

目标平台为 Windows 10 / Windows 11 x64。目前已在 Windows 11 本机完成构建与隔离验证，Windows 10 独立机器验收尚未完成。

## 下载与版本关系

- [下载 LumaRoute 0.3.3](https://github.com/chenduesr/LumaRoute/releases/tag/v0.3.3)：提供 EXE、MSI、源码包与 SHA-256 校验文件。
- 应用更新源填写 `chenduesr/LumaRoute`。检查功能查询正式 Release；预发布版本通过发布页面手动下载。

完整功能和验证边界见下文及 [迁移说明](docs/MIGRATION.md)。

## 开发与构建

构建需要 Windows x64、Node.js 24 与 npm、Rust 的 MSVC 工具链、Microsoft C++ 桌面开发工具（MSVC 与 Windows SDK）、WebView2 和 Git。已验证的工具版本见 [环境与验收报告](docs/VERIFICATION.md)。

在仓库根目录执行：

```powershell
npm ci
npm run setup:cores
npm run tauri dev
```

`setup:cores` 按 `scripts/cores.lock.json` 固定版本下载官方核心，校验压缩包和每个资源文件的 SHA-256；资源已存在且一致时跳过。Git 仓库不存储核心二进制，首次开发或构建需要完成这一步。Tauri 入口会补充当前用户的 Cargo PATH。不要同时启动第二个 1420 端口服务。

```powershell
npm run build       # TypeScript 与前端生产构建
npm run test:e2e    # 自动启动隔离 Tauri 窗口，执行真实 IPC / UI 测试
cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
npm run tauri build # Windows x64 EXE 安装包和 MSI
```

测试需要已准备核心。测试的证书与密码均为本地公开夹具；不安装证书到系统，不连接真实节点，不更改系统代理。原生测试使用独立应用和 WebView2 数据目录，结束后退出测试应用，可以与已安装的正式实例并存。截图与报告位于 `test-artifacts/`。

## 功能

- 节点：分享链接、Base64、Clash Meta YAML 与 sing-box JSON 导入，去重、搜索、筛选、排序、选择、重命名、删除、详情与敏感原文显示。
- 订阅：多个来源、手动/定时更新、失败保留原节点、取消任务。
- 连接：可选仅本地端口、Windows 系统代理或 Xray 原生 TUN 接管。Xray 管理 HTTP/SOCKS/TUN 入口、路由与 DNS；AnyTLS/TUIC 使用 sing-box SOCKS 桥接，其余协议直接由 Xray 出站。
- 测速：TCP、实际代理 HTTP、下载采样，支持并发、重试和取消；HTTPS 也经过被测节点。
- 设置：规则/全局/直连，域名与 IP 直连/代理/阻止规则、大陆绕过、自定义 DNS、DoH/DoT、FakeDNS。
- Windows：单实例、登录启动、启动连接、关闭到托盘；托盘可连接/断开、切换流量接管方式与最近节点、更新订阅、测试当前节点、切换简洁模式并显示实时速度。退出/核心崩溃后清理自有进程，并尝试恢复由本应用设置的系统代理或 Xray TUN 路由。
- 日志：来源/级别筛选、滚动、复制、轮转、诊断、脱敏 ZIP 导出。
- 更新：官方双核心更新和路由数据更新；应用更新源单独配置，仅检查并打开发布页面，不自动安装。
- Fluent 深浅主题、紧凑布局、减少动态效果、Radix Dialog/Dropdown Menu/Tooltip/Switch。
- 可持久化的简洁模式：支持添加并立即更新订阅、节点选择、延迟结果、当前/全部节点 TCP 或 HTTP 测试、连接/断开及全部订阅更新，完整界面随时切回。
- 快速添加订阅：打开时读取一次剪贴板中的 HTTP/HTTPS 链接，自动生成名称并预览域名；也可从本地二维码图片识别。重复地址会明确提示。
- 完整模式与简洁模式分别记忆窗口位置、逻辑尺寸和最大化状态；自绘标题栏支持双击最大化，尺寸按 Windows DPI 缩放比例换算。
- 启动时可依次更新全部订阅；失败时保留旧节点，并在更新结束后执行自动连接。
- 连接状态区分核心启动、本地端口就绪、远端验证、网络不可用、核心异常和流量接管失败；休眠恢复、网络接口变化及定期检查会重新验证当前节点。可选的自动恢复仅重启当前节点，五分钟内最多一次。
- 一键连接诊断检查双核心、HTTP/SOCKS 端口、节点 TCP、代理 HTTP、DNS 解析与路径、DNS 泄漏配置风险、IPv6、双栈连接、当前流量接管方式和实际出口 IP，并针对失败项目给出处理建议。
- 订阅更新识别 `subscription-userinfo` 流量与到期时间，并在订阅卡片显示格式、已用流量、剩余额度和到期状态。
- 前端通过 Tauri 事件接收连接、任务、流量和日志更新；空闲状态只保留低频兜底同步。
- `.github/workflows/windows-release.yml` 在推送版本标签后自动验证、构建 EXE/MSI、生成源码包及 SHA-256，并创建或更新 GitHub Release。

Xray 已移除 `allowInsecure`：自签名 TLS 节点可在详情填写服务器证书 **SHA-256 指纹**（64 位十六进制，可含冒号）。不会自动把未知证书当作可信证书。

TUN 模式首次连接会触发 Windows UAC。应用在取得管理员权限后重新启动，由 Xray 创建 `lumaroute_tun` 接口和 IPv4/可选 IPv6 默认路由；此模式不同时修改 Windows HTTP 系统代理。断开或退出时关闭核心，Wintun 接口及其路由随核心释放。局域网直连和 MTU 可在设置 → 常规中调整。

网卡流量显示是连接期间的估算，包含其他应用；“本地代理已就绪”不代表远端连通性已确认。详细协议范围及验证边界见 [迁移说明](docs/MIGRATION.md)。

## 安装包

- `src-tauri/target/release/bundle/nsis/LumaRoute_0.3.3_x64-setup.exe`
- `src-tauri/target/release/bundle/msi/LumaRoute_0.3.3_x64_zh-CN.msi`

EXE 安装器面向当前用户，提供简体中文/英文；MSI 为简体中文。缺少 WebView2 时安装器联网下载。直接运行 release 应用时需同时保留旁边的 `cores/` 资源目录，不能只复制单个主程序。

## 数据与结构

数据位于 Tauri 当前用户数据目录（Windows 通常为 `%APPDATA%\com.lumaroute.desktop`），可在设置 → 外观中打开。配置原子保存，并保留上一次有效备份；读取损坏配置时保护原文件。节点和订阅含凭据，保存在本机，请妥善保管。

```text
src/
  components/     # 标题栏、Radix 弹窗与提示
  pages/          # 概览、节点、订阅、日志、设置
  hooks/          # Tauri 状态事件、主题与外观存储
  lib/            # IPC、TypeScript 类型
  assets/         # 标志
src-tauri/
  src/            # 解析、配置、存储、核心进程、服务、Windows 集成
  resources/      # 双核心与数据文件、许可证
  capabilities/   # Tauri 窗口权限
scripts/          # 固定版本核心准备、原生 UI 测试、Tauri 入口
tests/fixtures/  # 本地测试证书
docs/            # 迁移、验证、第三方组件说明
```

Tailwind 4 使用 `@tailwindcss/vite` 和 CSS `@import "tailwindcss"`，无重复 PostCSS/Tailwind 配置。依赖由 npm/Cargo 锁文件固定，核心由独立资源锁文件固定。

## 0.2.1 排版修正与源码包

取消标题负字距并增加左右留白。详见 [0.2.1 发布说明](docs/RELEASE-0.2.1.md)。可通过 `scripts/package-source.ps1` 生成包含双核心资源的源码包。GitHub 自动生成的 Source code 压缩包不包含核心资源，需要运行 `npm run setup:cores`。

## 0.2.2 简洁模式与启动订阅更新

新增可切换的简洁模式、订阅全部更新，以及启动更新后再自动连接。完整说明见 [0.2.2 发布说明](docs/RELEASE-0.2.2.md)。

## 0.2.3 简洁模式订阅与测速

简洁模式现可直接添加订阅并立即获取节点，支持当前或全部节点的 HTTP/TCP 延迟测试，并放大默认窗口和主要控件。完整说明见 [0.2.3 发布说明](docs/RELEASE-0.2.3.md)。

## 0.2.4 Windows 凭据保护

本地配置使用当前 Windows 用户的 DPAPI 加密保存。现有明文配置可直接读取，并会在下一次保存时自动迁移为密文；备份同样加密。完整说明见 [0.2.4 发布说明](docs/RELEASE-0.2.4.md)。

## 0.2.5 快捷订阅、托盘与窗口记忆

新增剪贴板和二维码订阅导入、重复地址提醒、完整托盘快捷操作，以及按模式独立保存的窗口位置/尺寸/最大化状态。完整说明见 [0.2.5 发布说明](docs/RELEASE-0.2.5.md)。

## 0.3.0 LumaRoute 品牌更新

LumaRoute 采用“光线沿路径抵达”的 L 形路线标志；首次启动默认进入简洁模式。完整说明见 [0.3.0 发布说明](docs/RELEASE-0.3.0.md)。

## 0.3.1 界面细节优化

提高完整模式与简洁模式的文字、按钮和输入控件可读性，强化卡片层级、连接状态和深浅主题反馈，并修复日志来源换行。完整说明见 [0.3.1 发布说明](docs/RELEASE-0.3.1.md)。

## 0.3.2 连接稳定性与诊断

增加休眠恢复、网络变化和核心异常后的当前节点恢复，细化连接状态，并将一键诊断扩展为核心、端口、节点、代理、DNS、系统代理和出口 IP 七项检查。完整说明见 [0.3.2 发布说明](docs/RELEASE-0.3.2.md)。

## 0.3.3 网络诊断与自动发布

补充 DNS 泄漏配置风险、IPv6 与双栈连接诊断，增强 Clash Meta 和 sing-box JSON 订阅兼容性，使用 Tauri 事件实时同步状态，并加入标签触发的 Windows 自动构建与发布。完整说明见 [0.3.3 发布说明](docs/RELEASE-0.3.3.md)。

## 许可证

项目源码使用 [GPL-3.0-only](LICENSE)。捆绑核心的版本、许可证及对应上游源码链接见 [第三方组件说明](docs/THIRD_PARTY.md)。
