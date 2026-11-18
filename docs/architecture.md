# 技术方案

## HShell 0.3.0 的选择

Ubuntu 改用 **Rust + GTK 3 + VTE + 系统 OpenSSH**。Windows 0.3.4 使用 Tauri / xterm.js 前端；两者共享不依赖 GUI 的 `ssh-core` 和 `hshell-sftp`，连接配置和凭据格式相同。当前原生 GTK 前端针对 Ubuntu amd64，Windows 11 已通过回环 SSH、SFTP 和凭据库检查，具体范围见 [Windows 客户端](windows.md)。

0.1.4 使用系统 WebView，安装包小，但运行时仍有 WebKit、渲染与网络进程。在同一 Ubuntu 24 X11 桌面、1280×820 空窗口、相同共存应用下，旧发布版两轮中位数约 142 MiB PSS / 440 MiB RSS。关闭 WebKit 合成、关闭硬件加速、调整页面缓存后，PSS 仍约 142 MiB。主要开销是运行时本身，继续微调 CSS 或字体无法实现同等幅度的下降。

VTE 已解决 VT 解析、Unicode、输入法、选择、搜索、滚动和终端尺寸同步，避免自行实现终端内核。GTK 3 / VTE 0.68 是 Ubuntu 22.04 的共同基线。运行时动态链接系统库，安装包无需 WebKit 依赖。PSS 会随着系统库的共享程度改变，不能把某台桌面测得的 19 MiB 当成所有环境的固定占用；验证记录同时保留 RSS 和 Ubuntu 22 Xvfb 数据。

## 数据路径

```text
Ubuntu 原生 GTK 界面（连接、标签、搜索、剪贴板）
  ├─ VTE（VT 解析、渲染、3,000 行回滚）
  │   └─ 直接拥有 PTY master 的独立文件描述符
  └─ Rust ssh-core / DirectSession（校验、PTY、子进程、askpass）
      └─ 系统 OpenSSH（协议、认证、known_hosts、ssh_config）

Windows：TypeScript / xterm.js → Tauri IPC → ssh-core → ConPTY → 系统 ssh.exe
两端 SFTP：后台 hshell-sftp → 独立 OpenSSH 管道 → SFTP 子系统
```

原生输出不经过 Rust 字符串转换、JSON 序列化、WebView IPC 或 JS 事件循环。VTE 直接读取 PTY，主线程负责 GTK 事件；配置、凭据库和启动进程在工作线程执行。每个会话只保留等待 SSH 退出的阻塞线程；保存密码的 askpass 接收线程使用阻塞 accept，关闭时唤醒，不做固定周期轮询。

最多 12 个终端，每个 3,000 行回滚；单次粘贴不超过 1 MiB，多行内容先确认。隐藏标签仍处理输出。空闲不创建 VTE、无光标闪烁、无动画定时刷新、无 HTTP 服务、无终端日志或遥测。限制回滚不能代替终端整体内存测量，VTE、字体和系统库另有占用。

主机列表复用主机行和分组标题，只更新改变的文本、图标和排序键；编辑回调查找当前配置，避免捕获旧数据。新增行每批最多 16 条或 6 ms，在主循环空闲时继续；保存和退出会取消尚未完成的批次。初次填充可能更久，换取窗口尽早绘制与处理输入；大量行的 GTK 布局仍可能有停顿。搜索及收藏筛选使用大小写归一化字符串，按需要切换已有行与分组标题的可见性。终端中的同一查询复用 VTE 已持有的正则，不在上一处/下一处时重新编译。

## 生命周期

原生窗口直接处理 GTK delete-event，运行中的会话先确认，取消后保留连接，再次关闭仍可操作；确认等待期间拦截重复请求。配置保存期间延后退出，避免写入被截断。关闭标签销毁对应 VTE、释放 PTY、终止 SSH；自然退出保留输出并禁用输入，释放会话后端。异步回调通过弱引用和关闭标记避免操作已销毁终端。

Ubuntu 24 的 VTE 0.76 在测试中对 `vte_terminal_set_pty(terminal, NULL)` 发生崩溃；关闭采用 GTK widget 的内部销毁路径释放 PTY，不调用这个空指针 setter。Ubuntu 22 的 VTE 0.68 没有新版的 `get_text_selected`，复制使用共同支持的 `copy_clipboard_format`。两种兼容问题都由真实多标签测试覆盖。

终端关闭后（包括仍有其他会话时），合并安排 500 ms 延迟任务，在后台线程调用 glibc `malloc_trim(0)` 归还空闲页；连续关闭会合并为一次；期间重新连接或退出会取消尚未执行的任务。不在输入、输出、滚动路径中清理内存，不清除存活终端的历史。它只能回收分配器可释放的页，不能保证回到初始内存数值。`native-probe` 的关闭开关仅用于同一二进制 A/B 测量，发布版不含该测试入口。

## 认证与配置

OpenSSH 参数逐项传给子进程，不通过 shell 拼接。主机地址拒绝选项前缀和 shell 语法；用户名、端口留空时沿用用户 SSH 配置。保留别名、agent、私钥、ProxyJump，强制主机校验、禁用 LocalCommand 和连接复用主进程，一个标签对应一个可清理的 SSH 进程。

HShell 保留 `io.qizhan.ssh` 配置路径和 `io.qizhan.ssh.password` 凭据服务名，使改名升级继续读写原有数据。Linux 的可执行名、桌面文件、图标名与包名改为 `hshell`，显示名为 HShell。Windows 保留 Tauri 安装标识，显示名与主程序名改为 HShell。

连接 JSON 采用同目录临时文件、同步、原子替换；Linux 目录 0700 / 文件 0600。损坏配置禁止覆盖。最多 1,000 条连接，不额外引入数据库。原生连接准备复用经过校验的 Arc 快照，Unix 下每次检查路径、dev/inode、ctime、mtime 和文件大小；读前后复核，变化时重新解析，损坏时返回错误。Windows 保持即时读取，等待同等文件身份检查的实机验证。快照不存密码，每次连接仍读取系统凭据并检查目标绑定。

保存密码使用 Linux Secret Service；Windows 核心保留 Credential Manager。JSON 只存保存状态，凭据绑定 host / username / port。修改目标须重输密码，删除连接删除凭据，配置写入失败时回滚凭据。不会降级到明文文件。

保存密码的连接使用当前程序的无界面 askpass helper：每会话独立回环 socket、随机 capability，口令不进入 argv、环境变量、磁盘或 PTY 自动输入。只对 `ssh -G` 解析出的目标 password 提示自动填一次；首次 SHA256 指纹仍须用户确认，密钥变化拒绝。跳板机和 MFA 不自动匹配。未保存密码时直接在终端认证。多语言与跳板机实机验收仍是平台边界。

## 界面与打包

石墨灰 / 冷蓝桌面布局；主机栏、会话标签、快速连接、完整终端画布。GTK 符号图标，不使用后台管理表格、装饰卡片、渐变或远程字体。IBM Plex Sans / JetBrains Mono 的更名衍生 TTF 通过 Fontconfig 应用接口加载，中文回退 Noto CJK；许可随包分发。

Linux 快捷键使用 Ctrl+Shift 组合，终端保留 Ctrl+B / Ctrl+K 给 tmux 和 Shell。Windows 前端独立维护，在跨平台可用性与 Linux 资源占用之间保留明确边界。

Ubuntu 发布版在 22.04 构建，不带 `native-probe` 测试入口，安装命令为 `hshell`，替换旧 `qizhan-ssh` 包。`package_native.py` 使用基线发行版的 `dpkg-shlibdeps` 计算共享库依赖，并拒绝包含测试入口或 WebKit 的产物。CI 定义 Ubuntu 22/24 原生包与 Windows Tauri 包；工作流定义不等于远端 CI 已运行。

尚未实现目录同步、远程编辑、云同步、自动重连、自动更新或 Windows 原生终端前端。优先做好日常连接和多标签，继续以真实测量约束后续功能。

## SFTP 文件传输

共享 `hshell-sftp` 封装真实 SFTP v3 协议，系统 OpenSSH 提供认证和加密管道；浏览、传输及终端各自独立。GTK 文件栏按需创建，有界目录批次与限频进度避免占用主线程；空闲时停止轮询。并发、固定缓冲、刷盘检查点、前缀核对及原子发布策略见 [SFTP 设计](sftp-design.md)。Linux 关闭传输时回收独立进程组，主循环退出后给工作线程有限清理时间；不改变终端 PTY 路径。
