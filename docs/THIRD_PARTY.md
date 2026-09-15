# 捆绑的第三方组件

核心使用官方 Windows amd64 二进制，不修改核心代码。准确版本、下载 URL、压缩包和资源 SHA-256 见 `scripts/cores.lock.json`。

| 组件      | 版本     | 官方源代码 / 发布                                 | 随附许可                                                                       |
| --------- | -------- | ------------------------------------------------- | ------------------------------------------------------------------------------ |
| Xray-core | v26.3.27 | https://github.com/XTLS/Xray-core/tree/v26.3.27   | `src-tauri/resources/xray/LICENSE`，MPL 2.0                                    |
| sing-box  | v1.14.1  | https://github.com/SagerNet/sing-box/tree/v1.14.1 | `src-tauri/resources/singbox/LICENSE`，GPL 3.0 或更新版本，附名称/关联声明条件 |

Xray 的 geoip/geosite 与 sing-box 的 libcronet.dll 来自同一份经校验的官方发布压缩包。核心许可证随安装器资源一起分发。

- Xray 发布包：https://github.com/XTLS/Xray-core/releases/tag/v26.3.27
- sing-box 发布包：https://github.com/SagerNet/sing-box/releases/tag/v1.14.1
- Xray TLS 指纹格式：https://xtls.github.io/config/transports/tls.html
- sing-box AnyTLS：https://sing-box.sagernet.org/configuration/outbound/anytls/
- sing-box TUIC：https://sing-box.sagernet.org/configuration/outbound/tuic/

核心为独立子进程，通过本机 SOCKS 通信；界面与业务层使用本项目源码。分发包随附核心许可证，上表链接指向对应版本的上游源代码；重新分发时请保留这些许可证、通知和源码获取信息。本项目不隶属于 Xray 或 sing-box 上游项目，也不表示获得其背书。

`tests/fixtures/localhost-key.pem` 和证书仅是公开本地测试材料，不用于产品服务器，也不安装到 Windows 证书库。
