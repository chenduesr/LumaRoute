# MyRay Lite 0.2.1

## 本次变更

- 取消一级标题的负字距，让中文标题使用正常字距。
- 页面标题增加左右 2px 留白，减轻首字贴边的视觉感受。
- 应用、界面与安装器版本同步为 0.2.1。

## 已完成验证

- 在 125% 像素缩放的排版预览中检查“节点”标题，字形完整、字距正常。
- TypeScript 检查与 Vite 生产构建通过。
- 0.2.0 的业务验收记录保留于 VERIFICATION.md；本次未重复操作真实节点或系统代理。

## 源码包

源码压缩包包含 React/TypeScript 与 Rust 源码、依赖锁文件、图标、测试夹具、构建脚本、文档，以及经过 SHA-256 验证的 Xray/sing-box 核心资源。

不包含 node_modules、Rust target、Git 历史、本机应用配置或测试运行数据。测试证书及私钥是公开的本地测试夹具，不能用于生产服务器。

解压后在项目目录运行：

```powershell
npm ci
npm run setup:cores
npm run tauri dev
```

核心已经随源码包提供，setup:cores 校验一致后会跳过下载。构建 Windows 安装包使用 npm run tauri build。

EXE 与 MSI 是两种安装方式，选择其中一种即可。安装包仍未配置代码签名。

## 最终产物检查

- npm run tauri build 成功生成 0.2.1 的 Windows x64 EXE 与 MSI，exit code 0。
- MSI 数据库只读检查通过：ProductVersion 为 0.2.1，包含主程序、Xray、sing-box、libcronet.dll、路由数据与许可证。
- 源码 ZIP 由 scripts/package-source.ps1 按明确目录清单生成，并逐文件解压计算 SHA-256，与当前源码/资源比对。
- 安装包大小与 SHA-256 见 artifacts.json；交付目录另附 SHA256SUMS.txt。

本次没有执行安装覆盖或安装/卸载测试；真实节点、系统代理切换及 Windows 10 独立机器验收仍未进行。
