# 原生客户端字体

这四个 TTF 文件由 `src/assets/fonts` 中已更名的 WOFF2 衍生字体通过 FontTools 解压得到。没有重新引入未更名的原字体，保留版权字段和 SIL Open Font License 1.1 许可。

- IBM Plex Sans 400 / 500：内部家族名 `QZ Design Sans A`，用于界面。
- JetBrains Mono 400 / 600：内部家族名 `QZ Design Mono A`，用于终端。
- 中文使用系统 Noto CJK / 等宽字体回退，安装包推荐 `fonts-noto-cjk`。

四个文件共 301,236 字节，通过 Fontconfig 的应用字体接口加载；不安装到系统字体目录，不发起网络请求。完整来源与衍生处理记录见 `src/assets/fonts/README.md`，两份 OFL 许可在 `LICENSES/`，安装包中位于 `/usr/share/doc/hshell/`。
