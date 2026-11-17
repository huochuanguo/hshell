# 客户端字体

IBM Plex Sans 400/500 用于界面，JetBrains Mono 400/600 用于终端。来源为 Google Fonts 官方仓库 `google/fonts`：

- https://github.com/google/fonts/tree/main/ofl/ibmplexsans
- https://github.com/google/fonts/tree/main/ofl/jetbrainsmono

使用 FontTools 对原字体实例化、子集化为 WOFF2，保留拉丁字符、常用符号和框线；中文回退系统字体。内部衍生家族名为 `QZ Design Sans A` / `QZ Design Mono A`，CSS 加载别名为 `HShell Sans` / `HShell Mono`，保留原版权与归属字段。关闭终端连字以便逐字符辨认命令。

四个文件共 106,228 字节。没有运行时远程字体请求，没有向系统安装字体。许可为 SIL Open Font License 1.1，原文在根目录 `LICENSES/ibm-plex-sans-OFL.txt` 和 `LICENSES/jetbrains-OFL.txt`，Tauri 安装包将其包含在 `licenses/` 资源目录。
