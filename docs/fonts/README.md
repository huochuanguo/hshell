# 字体设计样张资源

本目录保存 `docs/typography-preview.html` 使用的字体样张资源。0.1.4 已将选定的 IBM Plex Sans 400/500 与 JetBrains Mono 400/600 四个文件复制到 `src/assets/fonts/`，正式客户端引用该生产目录；其余样张字体不进入安装包。已发布的 0.1.3 安装包保持不变。

- JetBrains Mono、IBM Plex Sans、IBM Plex Mono：来自 Google Fonts 官方仓库 `google/fonts` 的 `ofl/jetbrainsmono`、`ofl/ibmplexsans`、`ofl/ibmplexmono`。保留各自的 OFL 版权与许可文件。
- Ubuntu Sans / Ubuntu Sans Mono：来自本机 Ubuntu `fonts-ubuntu` 软件包。保留软件包提供的版权与许可文件。
- 样张使用本地 WOFF2 字体，无运行时远程字体请求。仅保留拉丁字符、常见符号及框线，中文由本机 `Noto Sans CJK SC` / `Noto Sans Mono CJK SC` 回退；这不能替代 Windows 或缺少中文字体的环境验证。
- WOFF2 由 FontTools 实例化和子集化；为区分衍生文件，内部字体家族名改为 `QZ Design ...`，来源归属保持在原版权字段和许可文件中。UI 使用 400/500，终端使用 400/600。
- 推荐的 A 组合（IBM Plex Sans 400/500 与 JetBrains Mono 400/600）共 106,228 字节，约 104 KiB。这只是拉丁字体资源大小，不是客户端内存或性能测量，也不包含系统中文字体。
