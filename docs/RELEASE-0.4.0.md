# LumaRoute 0.4.0

这一版为 Windows 增加 Xray 原生 TUN 流量接管，并强化桌面端状态同步与安装包验证。

## Windows TUN 模式

- 流量接管可在仅本地代理、Windows 系统代理和 TUN 模式之间选择。
- TUN 由 Xray 原生入口承载，首次连接通过 Windows UAC 获取所需权限。
- 随安装包分发并校验 Wintun DLL 及其许可证。
- 支持 IPv4、可选 IPv6、局域网绕过和 MTU 设置。
- 断开、退出或核心异常时关闭受管核心，由 Wintun 释放接口和相关路由。
- 托盘、简洁模式和完整模式使用同一套流量接管配置。

## 配置与兼容性

- 旧版 `systemProxy` 设置自动迁移到新的 `captureMode`，保留用户原有选择。
- TUN 模式不同时修改 Windows HTTP 系统代理。
- AnyTLS 和 TUIC 继续使用 sing-box 桥接，其余支持协议由 Xray 直接处理。
- 诊断页面可检查当前系统代理或 TUN 接口、默认路由和权限状态。

## 实时状态同步

- 前端先注册 Tauri 原生事件，再读取初始快照，消除启动监听窗口。
- 查询结果带有请求与事件版本保护，较晚返回的旧快照不会覆盖新状态。
- 连接、断开、后台任务和异常通过事件即时更新；30 秒查询只保留为无事件时的最终兜底。
- 周期健康检查在后台静默执行，不再让界面反复闪成“正在验证网络”。
- 连接时长在前端每秒本地更新。

## 验证

- TypeScript 类型检查与 Vite 生产构建通过。
- 16 项 Rust 单元和隔离集成测试通过。
- 浏览器 UI、Debug 原生端到端和 Release EXE 端到端测试通过。
- NSIS EXE 与 WiX MSI 构建成功。
- MSI 内容检查确认主程序、Xray、sing-box、Wintun、许可证和 Geo 数据完整。

安装包未包含代码签名，Windows SmartScreen 可能在首次运行时显示来源提示。
