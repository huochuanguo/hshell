# HShell

面向日常服务器运维的轻量 SSH 桌面工作空间。首版聚焦连接管理与多标签终端，目标平台为 Ubuntu 22.04 / 24.04，预留 Windows 10 1809+ / Windows 11 适配。

Ubuntu 0.3.1 使用 **Rust + GTK 3 + VTE + 系统 OpenSSH**，终端直接读取 PTY，不启动 WebKit 或本地 HTTP 服务。Windows 继续保留 Tauri 2 / xterm.js 前端与共享 Rust 核心，尚待实机适配。密码可选存入系统凭据库。详细取舍见 [技术方案](docs/architecture.md)，实测边界见 [验证记录](docs/verification.md)。

## 已实现

- 连接新增、编辑、删除、分组、收藏、搜索；配置在本地原子写入。
- 桌面客户端布局：可收起的分组主机栏、可拖动会话标签、完整终端画布；石墨灰与冷蓝配色。
- 本地内置 IBM Plex Sans 界面字体与 JetBrains Mono 终端字体，中文使用系统字体回退；终端默认 15px。
- 快速连接支持 `用户名@主机` / SSH 别名 / IPv6，支持另存为主机。
- 多标签交互终端，尺寸同步、终端搜索、复制选中内容、断开并保留输出、手动重连。
- 系统 OpenSSH 密码 / 私钥 / ssh-agent 认证；用户名和端口留空时沿用 SSH 配置。
- 主机别名、ProxyJump 等能力直接复用 `~/.ssh/config`。
- 首次主机指纹由 OpenSSH 提示确认，指纹变化时拒绝连接。
- 多行粘贴确认，关闭会话及退出应用确认，退出时清理 SSH 进程。
- 每终端 3,000 行回滚、最多 12 个终端、单次粘贴上限 1 MiB。
- 主机列表按行增量更新，首次加载分批创建；配置校验快照在文件身份变化后刷新，避免每次连接重复解析。
- 主机搜索复用已有控件与检索字符串；终端搜索复用编译结果。关闭标签后延迟合并回收空闲堆页（包括仍有其他会话时），重新连接会取消尚未开始的回收。
- Ubuntu 终端由 VTE 直接解析、渲染、滚动，不经过 JS 或 IPC；隐藏标签继续处理输出。Tauri 前端保留二进制 IPC 与输出背压。
- 连接表单支持保存、替换、清除登录密码；Linux 使用 Secret Service，Windows 预留 Credential Manager。
- Ubuntu SFTP：右侧文件栏、目录导航、上传/下载、双向断点续传、暂停/继续、重启后恢复、新建目录、重命名及删除文件/空目录。最多两路传输，文件内容不进入界面线程。
- 文件栏按类型区分图标：目录柔金色、链接青绿色、代码冷蓝色、压缩包淡紫色、图片灰绿色；目录与链接名称同步着色，普通文件名保持浅灰。六类图标缓存复用，不增加远程查询。

在终端会话中点击「文件」打开 SFTP。收起文件栏会断开浏览连接，后台传输继续；退出应用保留断点，再次打开文件栏后点击「继续」。续传会在后台完整核对已传前缀，大文件可能有一段校验时间。移除未完成任务会明确保留临时文件路径，按需清理。断点记录不含密码。详见 [SFTP 设计](docs/sftp-design.md)。

没有实现目录同步、远程编辑、目录递归传输、独立密码保险库、自动重连、同步服务或自动更新。浏览器预览不建立 SSH 连接，预览中添加的数据只存在内存中。浏览器中的「体验终端界面」支持本地 help / echo / clear / exit 演示，明确显示未连接服务器，桌面版不显示演示入口。

## 开发运行

Ubuntu 原生版只需要当前 Rust stable 和原生开发依赖：

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libgtk-3-dev libvte-2.91-dev \
  libfontconfig1-dev openssh-client fonts-noto-cjk
cargo run -p hshell-native
```

Ubuntu 22.04 请启用官方 `universe` 仓库并安装当前更新。原生客户端最低 VTE 0.68，不需要 WebKitGTK。开发旧 Tauri 前端还需要 Node.js 24 LTS、`libwebkit2gtk-4.1-dev`、`libxdo-dev`、`libssl-dev`、`libayatana-appindicator3-dev`、`librsvg2-dev`，之后运行 `npm ci && npm run desktop`。

只看保留的 Web 前端界面：

```bash
npm run dev
# 打开 http://127.0.0.1:1420
```

## 打包与安装

0.2.3 的终端优化已按 2～3 小时要求完成验收：12 会话持续输出约 2 小时 55 分钟，另用同一测试构建完成 64 次连接开关和退出清理。长时资源与独立补测分别记录，完整进度见 [执行记录](docs/optimization-progress.md)。

0.3.0 新增 SFTP 的专项验收见 [验证记录](docs/verification.md)，未将旧版长时结果作为本版 SFTP 的长时证明。

Ubuntu 原生 **HShell 0.3.1** 的 amd64 包位于 `target/native-ubuntu22/release/bundle/deb/hshell_0.3.1_amd64.deb`，命令为 `hshell`。安装时自动替换旧 `qizhan-ssh` 包；请先退出旧客户端，再安装并从启动器打开 HShell。原连接配置、保存的密码及 OpenSSH 信任记录继续使用，无需导出迁移。

新图标将 H 与终端提示符结合，提供 Linux 多尺寸 PNG / SVG 和 Windows ICO。矢量源为 [hshell.svg](src/assets/hshell.svg)，运行 `python3 scripts/generate_icons.py` 可重新生成（开发时需 `rsvg-convert` 与 Pillow）。

安装本次构建产物：

```bash
sudo apt install ./target/native-ubuntu22/release/bundle/deb/hshell_0.3.1_amd64.deb
```

在本机重新构建：

```bash
cargo build -p hshell-native --release --locked
python3 scripts/package_native.py target/release/hshell \
  --output target/release/bundle/deb/hshell_0.3.1_amd64.deb
```

Ubuntu 22.04 的发布包必须在 Ubuntu 22.04 构建，再在 22.04 和 24.04 安装验收。**不能用 Ubuntu 24.04 构建结果宣称兼容 22.04**，因为 glibc 基线可能不兼容。CI 已定义两套 Ubuntu 构建及 Windows 构建，但添加工作流不代表已实际运行。

Windows 开发需要 MSVC C++ Build Tools、Rust MSVC 工具链、Node.js、WebView2 Runtime，以及 Windows「可选功能」中的 OpenSSH 客户端：

```powershell
npm ci
npm run desktop
npm run bundle -- --bundles nsis
```

Windows 使用保留的 Tauri 前端，通过 `portable-pty` 使用 ConPTY；Linux 原生 crate 在非 Linux 平台不加载 GTK 依赖。Windows 当前属于适配预留，尚需实机验证，尤其是中文输入、剪贴板、密钥路径和退出清理。

## 连接方式

1. 点击「新建连接」，填写名称与主机/IP；用户名、端口、私钥路径均可留空。
2. 使用 SSH 配置时，将主机填写为配置中的 `Host` 别名，其他覆盖项留空。
3. 如需保存密码，在「登录密码」填写并勾选「保存密码到本机系统凭据库」。首次连接仍需核对 SHA256 主机指纹；主机密钥变化时拒绝连接。
4. 已保存密码的连接通过本地 askpass 通道自动认证；密码不会进入命令行参数、环境变量、临时文件或终端输入流。密码错误后会弹出认证窗口。
5. 编辑已保存密码的连接，密码框留空保持原密码；输入新密码可替换，取消勾选可清除。更改主机、用户名或端口时须重新填写密码。删除连接同时删除凭据。

Linux 密码保存需要已解锁的 Secret Service（Ubuntu 桌面通常使用 GNOME Keyring）。不可用时会明确报错，可取消勾选继续只保存连接。最小化桌面可安装 `gnome-keyring` 并启用桌面会话服务；不会降级到明文文件。原配置兼容加载。

保存密码只自动处理目标主机的 SSH password 认证，不存储私钥口令，不自动回答 MFA。使用 ProxyJump / ProxyCommand 时，认证窗口要求手动输入，避免把目标服务器密码交给跳板机。未保存密码的连接继续使用原有终端认证。

用户名留空时，OpenSSH 使用 `Host` 配置或当前本地用户名，不默认使用 root。身份文件支持 OpenSSH 的 `~` 路径规则。IPv6 填写地址本身，例如 `2001:db8::1`，端口使用独立字段。

配置示例（由用户自行放入自己的 SSH 配置）：

```sshconfig
Host dev
    HostName 192.168.1.10
    User ubuntu
    IdentityFile ~/.ssh/id_ed25519

Host internal
    HostName 10.0.0.8
    User deploy
    ProxyJump dev
```

为兼容改名前的安装，内部配置及系统凭据命名空间保持稳定。连接配置路径：Linux 通常为 `~/.config/io.qizhan.ssh/profiles.json`，Windows 通常为 `%APPDATA%\io.qizhan.ssh\profiles.json`；应用的工作空间信息中展示实际位置。密钥本体仍由用户自行保管。信任记录使用 OpenSSH 的 `known_hosts`。

## 验证

原生交互测试仅在 `native-probe` 功能构建中启用，发布包脚本拒绝包含该测试入口的二进制。测试使用回环 SSH 夹具和临时配置；密码测试另起独立 DBus 和钥匙串，不访问用户凭据：

```bash
cargo build -p hshell-native --release --features native-probe --locked
python3 scripts/native_vte.py target/release/hshell --output /tmp/native.json
python3 scripts/native_vte.py target/release/hshell --credentials --output /tmp/native-keyring.json
# 12 会话持续输出、保留一会话时的回收、反复打开关闭及句柄检查：
python3 scripts/native_vte.py target/release/hshell --stress --partial-load --output /tmp/stress.json
# 三分钟持续输出：
python3 scripts/native_vte.py target/release/hshell --stress --seconds 180 --cycles 64 \
  --partial-load --output /tmp/stress-long.json
# 约 3 小时多会话测试（完成后才生成最终报告）：
python3 scripts/native_vte.py target/release/hshell --stress --seconds 10800 --cycles 64 \
  --partial-load --sample-interval 30 --output /tmp/soak.json
# 1,000 条连接：实际保存回调、控件复用与加载中途编辑：
python3 scripts/native_hosts.py target/release/hshell --updates --interleave \
  --max-replaced-rows 1 --output /tmp/hosts.json
# 独立 DBus / IBus / libpinyin 的组词提交（需 ibus-libpinyin / python3-gi）：
python3 scripts/native_vte.py target/release/hshell --ime --output /tmp/ime.json
# 真实 sshd / vim / tmux（使用一次性容器，只发布回环端口）：
docker build -f scripts/programs.Dockerfile -t hshell-test:programs-ubuntu22 .
python3 scripts/native_programs.py target/release/hshell --output /tmp/programs.json
# 测完后必须重新构建不带 native-probe 的发布版本。
```

长时验收最低要求为 2 小时。`scripts/validate_native_soak.py` 默认使用 7,200 秒门槛；可用 `--min-seconds 10800` 要求完整 3 小时。原计划运行中途缩短时，须同时提供关闭记录和同一二进制的独立完整生命周期报告，不会把补测的输入延迟当成长时延迟。本次约 2 小时 55 分钟的资源与补测汇总见 [验收记录](docs/native-soak-0.2.3-summary.json)。

Ubuntu 原生快捷键：`Ctrl Shift K` 快速连接、`Ctrl Shift B` 主机栏、`Ctrl Shift T/W/F` 新建/关闭/搜索、`Ctrl Shift C/V` 复制/粘贴、`Alt 1…9` 切换标签。终端内的 `Ctrl B/K` 保留给远端应用。

```bash
npm run build
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

不安装 GUI 开发包也可以独立测试后端：`cargo test -p ssh-core`。

真实 OpenSSH 协议验证（仅绑定回环地址，临时生成测试密钥，隔离用户 SSH 配置）：

```bash
python3 -m venv /tmp/hshell-test-venv
/tmp/hshell-test-venv/bin/pip install paramiko==3.5.1
/tmp/hshell-test-venv/bin/python scripts/ssh_smoke.py
```

测试覆盖真实 OpenSSH 的已保存密码登录、错误密码后手动重试、取消指纹信任、终端密码认证、私钥认证、指纹变化拒绝、UTF-8 往返及进程清理。系统钥匙串测试使用 `python3 scripts/keyring_smoke.py`（需要 `gnome-keyring` 和 `dbus-run-session`），创建独立 DBus 和临时钥匙串，不访问用户凭据。测试服务端是 Paramiko 协议夹具，不执行任意 shell 命令。另有独立的一次性 sshd / vim / tmux 夹具。用户实际生产服务器、跳板机、MFA、物理输入和各发行版桌面仍需独立验收。
