# LumaRoute 0.3.0

**LumaRoute** 名称来自 Luma（光）与 Route（路径），对应新的 L 形路线标志：起点发光，路径转向后抵达目标节点。

## 品牌与界面

- 应用标题、侧栏、简洁模式、托盘、日志、诊断文件和安装程序统一使用 LumaRoute。
- 新标志提供 SVG、Windows ICO、PNG、Windows Store、Android 和 iOS 所需尺寸；16–32 像素下仍能辨认路线与两个节点。
- 包名、Rust crate、Release 可执行文件和源码包名称同步更名。

## 默认简洁模式

- 首次启动默认进入简洁模式。
- 用户切换到完整模式后仍会记住选择，不会在每次启动时强制切回。
- 简洁模式、完整模式分别保存窗口尺寸与位置，模式选择会持续保存。

## 应用标识

- Tauri 应用标识为 `com.lumaroute.desktop`，使用独立的数据目录和 Windows 安装记录。
- 登录启动项、DPAPI 配置头、测试隔离变量均统一使用 LumaRoute 名称。
- 不复制其他应用的数据，首次启动使用空白配置。
- 应用更新仓库为 `chenduesr/LumaRoute`。
