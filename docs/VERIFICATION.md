# 环境与验收报告

日期：2026-09-15。范围：Windows 双核心功能迁移，MyRay Lite 0.2.0。

## 环境检查

| 项目          | 最终版本 / 状态        | 本次操作                                 |
| ------------- | ---------------------- | ---------------------------------------- |
| Node.js       | 24.14.0                | 已安装，复用                             |
| npm           | 11.9.0                 | 已安装，复用                             |
| Git           | 2.45.1.windows.1       | 已安装，复用                             |
| WebView2      | 152.0.4191.66          | 已安装，复用                             |
| Rust / Cargo  | 1.98.1 / 1.98.1        | 官方 rustup 新安装并复查                 |
| rustup        | 1.29.1                 | 官方安装，stable-x86_64-pc-windows-msvc  |
| Visual Studio | Community 2026，18.9.2 | 复用已有 VS，补装 NativeDesktop 工作负载 |
| MSVC          | 14.51.36231，x64/x86   | 补装后通过 Tauri 检测与实际编译          |
| Windows SDK   | 10.0.26100.0           | 安装至 D:\Windows Kits\10                |

本机 Windows 版本：25H2，build 26200.9445，x64。系统注册表 ProductName 仍使用 Windows 10 Home China 字符串；这里只记录实际构建号，不把本机验收视为 Windows 10 与 Windows 11 双机测试。

## 项目版本

- Tauri Rust 2.11.5 / CLI 2.11.4 / JS API 2.11.1（同一兼容主版本）
- React 19.3.0 / TypeScript 6.0.3 / Vite 8.3.0
- Tailwind CSS 4.3.3，官方 Vite 插件
- Radix Dialog 1.1.23 / Dropdown Menu 2.1.24 / Tooltip 1.2.16 / Switch 1.3.7

## 当前通过的验证

- `npm run setup:cores`：固定版本资源逐文件 SHA-256 匹配，已安装资源跳过下载。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`：12 项测试通过。
- `npm run tauri dev`：实际启动原生 Windows 窗口。
- 原生端到端脚本：8 组流程通过，真实 Tauri IPC 与实际核心，无前后端 mock，无 JavaScript 运行错误。
- 旧项目 `../MyRay-Lite` 的 `git status --short` 为空，未修改旧源码。

### Rust 覆盖

解析：链接、Base64、嵌套 YAML、IPv6、SS 变体、无效条目与去重。

配置：自定义域名/IP 规则拆分与顺序、无节点直连、设置边界。由实际 Xray/sing-box 检查 VMess、VLESS、Trojan、SS、HTTP、SOCKS、Hysteria2、AnyTLS、TUIC；检查 WS、gRPC、HTTPUpgrade、XHTTP、KCP、REALITY、DNS/FakeDNS 配置。

实际本地链路：VLESS、AnyTLS、TUIC、Hysteria2 的 HTTP 请求、下载采样、连接/断开；自签名 Hysteria2 通过证书指纹验证。HTTPS 测速的负向测试确认不可用代理不会绕过代理直接访问目标。

故障：端口占用不残留连接、核心被终止后清理、订阅 HTTP 失败及取消保留节点、诊断包无测试凭据、配置损坏不覆盖、备份恢复基础逻辑、系统代理恢复所有权判断。

### 原生 UI 覆盖

1. 真实窗口与 Rust IPC，双核心和路由数据就绪。
2. 无效端口拒绝、有效设置保存。
3. 深浅主题和页面重载后的持久化。
4. 节点导入、选择、重命名、Radix Dialog/Dropdown/Tooltip、真实 HTTP 测速。
5. 点击连接、概览状态变化、点击断开；系统代理始终为关闭状态。
6. 本机订阅 HTTP 获取；服务返回 503 后原节点保留。
7. 诊断与 ZIP 导出；应用发布源为空时明确报错，避免使用旧 WPF 更新源。
8. 全流程无 JavaScript 运行错误。

原始报告：`test-artifacts/native-verification.json`；截图：`test-artifacts/native-*.png`。测试夹具和运行器保存在 `tests/fixtures/`、`scripts/test-native.mjs`。每次独立运行使用新的 `test-artifacts/native-run-*` 数据目录。

## 修复过的实际问题

- Xray 不接受 AnyTLS/TUIC 出站：增加 sing-box 本地桥接。
- Hysteria2 配置名称改变：使用当前 Xray 的 hysteria v2 结构。
- Xray 移除 allowInsecure：加入节点证书指纹配置与校验。
- HTTPS 测速可能绕过代理：显式代理所有 HTTP/HTTPS 请求，并增加负向回归。
- Windows TCP/UDP 端口可用性不同：连接前分别检查；隔离测试使用可同时绑定两种协议的端口。
- 日志轮转旧文件占用目标名称、状态快照锁持有时间、启动连接和更新检查互斥：已修正。
- 对话框操作失败时错误藏在遮罩后：错误同时显示在当前对话框内。
- 原生鼠标悬停测试受窗口焦点影响：以键盘焦点验证 Tooltip。
- TypeScript 目标不支持数组 at：改用索引，保留项目目标版本。

## 明确未进行的操作

未读取或迁移旧版用户数据，未使用真实订阅/节点，未实际切换 Windows 系统代理或登录启动项，未在 Windows 10 独立机器上验收。系统代理 API 已实现，真实切换仍按用户选择等待确认。

## 最终构建与发布版验收

`npm run tauri build` 成功退出（exit code 0），生成两种 Windows x64 安装包：

| 产物           | 字节数                     | 文件                                                                         |
| -------------- | -------------------------- | ---------------------------------------------------------------------------- |
| EXE 安装程序   | 39,335,897（约 37.51 MiB） | `src-tauri/target/release/bundle/nsis/MyRay Lite_0.2.0_x64-setup.exe`        |
| MSI 安装程序   | 56,647,680（约 54.02 MiB） | `src-tauri/target/release/bundle/msi/MyRay Lite_0.2.0_x64_zh-CN.msi`         |
| Release 主程序 | 8,157,696                  | `src-tauri/target/release/myray-lite-tauri.exe`（须保留旁边 cores 资源目录） |

SHA-256 和精确产物信息见 `artifacts-0.2.0.json`。构建日志为项目根目录 `migration-windows-build.log`。

`node scripts/test-native.mjs --release --check-core-updates`：**10 组检查通过**，包括原生业务流程、真实官方 Xray/sing-box 下载与 SHA-256 校验后更新、单独更新路由数据，以及 820×620 内容视口无横向溢出。所有更新发生在新建隔离目录中。该测试直接启动最终 Release 主程序，证实发布资源路径可用。

机器可读报告：`test-artifacts/native-release-verification.json`；完整输出：`migration-release-test.log`。

`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-installers.ps1`：MSI 数据库以只读方式打开，ProductVersion 为 0.2.0，包含主程序、xray.exe、sing-box.exe、libcronet.dll、geoip.dat、geosite.dat 和两份核心许可证。结果见 `test-artifacts/msi-contents.json`。

本轮没有在当前用户环境执行安装/卸载，因此不把“安装包生成及内容检查通过”表述为“安装/卸载回归通过”。Windows 10 独立机器测试仍未进行。

Rust Release 出现一条 MSVC 创建导入库/导出文件的 linker_messages 提示，不影响成功构建；TypeScript 检查及前端生产构建通过。

