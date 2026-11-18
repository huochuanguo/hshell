# Windows 客户端

Windows 使用 Tauri 2 / WebView2 + xterm.js + ConPTY，共享 Rust SSH 和 SFTP 核心。Windows 桌面端版本为 0.3.6；配置标识 `io.qizhan.ssh` 和凭据命名空间保持不变。

## 功能与系统要求

- Windows 10 1809+ / Windows 11，x64；Windows OpenSSH 客户端、WebView2 Runtime。
- 多标签终端、连接分组/收藏/搜索、快速连接、系统凭据库保存密码、主机指纹确认、终端搜索及原生剪贴板。
- 文件栏支持目录浏览、新建目录、重命名、删除普通文件/空目录、上传下载、最多两路传输、暂停继续及重启后恢复。
- 远程路径始终使用 `/home/user/file` 等 Unix 路径；本机文件由 Windows 文件选择器选择，支持中文、空格、盘符及 UNC 路径。
- 终端和 SFTP 进程加入 Windows Job Object；关闭会话或应用退出时回收 SSH 及其子进程。收起文件栏只关闭浏览连接，传输继续；退出保留断点，下次手动继续。
- 保存密码及 SFTP 需要 OpenSSH 8.4+ 的 askpass 支持；旧版会明确提示升级，仍可使用终端内手动认证。默认固定使用 `%SystemRoot%\System32\OpenSSH\ssh.exe`，避免 Git/MSYS 的同名程序干扰。

不包含目录递归传输、目录同步、远程编辑或自动重连。续传会完整核对已传前缀，不能仅凭文件大小判断安全性。下载时由系统保存对话框确认替换；普通远程重命名拒绝覆盖已有目标。远程非 UTF-8 文件名不在 Windows 支持范围内。

## 本机开发与打包

安装 Node.js 24 LTS、Rust stable MSVC 工具链、Visual Studio C++ Build Tools（含 Windows SDK），以及 Windows 可选功能中的 OpenSSH 客户端。WebView2 随多数 Windows 11 系统安装；NSIS 安装器在缺少运行时时下载并安装它。

```powershell
npm ci
npm run desktop
# Windows 默认选择 NSIS，无须附加 --bundles 参数
npm run bundle -- --ci
```

安装包输出到 `target/release/bundle/nsis/`。默认只安装到当前用户目录，界面语言提供简体中文和英文。当前未配置代码签名证书，构建产物不能视为已签名发行版。

完整本机验收及打包：

```powershell
./scripts/build_windows.ps1
# Python 命令不在 PATH 时可传入实际解释器路径
./scripts/build_windows.ps1 -Python C:/Python311/python.exe
```

脚本默认运行前端/Rust 检查、真实回环 SSH、SFTP 跨进程双向续传及 Windows 凭据管理器验证，然后打包。`-SkipIntegration` 或 `-SkipBundle` 仅用于明确缩小检查范围，不代表已通过被跳过的验收。

## 隔离验证

```powershell
python -m venv target/windows-test-venv
target/windows-test-venv/Scripts/python -m pip install paramiko==3.5.1
target/windows-test-venv/Scripts/python scripts/ssh_smoke.py
target/windows-test-venv/Scripts/python scripts/windows_sftp_smoke.py
cargo run -p ssh-core --example credentials_check --locked
```

SSH 测试使用临时密钥、临时 SSH 配置及临时 known_hosts，覆盖保存密码、错误密码手动重试、拒绝指纹、终端密码、私钥、指纹变化和 UTF-8 回显。Windows 私钥 ACL 仅修改夹具创建的文件。

夹具还覆盖活动会话主动关闭和正常退出后的回收；无 xterm 的核心测试自行回复 ConPTY 的初始光标查询。Windows 终端在 SSH 退出后主动关闭 ConPTY，再排空输出管道，避免读取线程一直等待 EOF。

SFTP 测试只监听回环地址，使用一次性 Paramiko 服务端和固定测试密码；32 MiB 文件上传及下载分别在独立进程重启后续传，核对前缀和 SHA256，并验证中文路径、目录操作及重命名拒绝覆盖。结果保存到 `target/windows-sftp.json`。它不代表公网网络、生产 sshd、跳板机或长时验收。

凭据管理器测试使用随机 UUID 创建、读取、替换并删除自己的测试条目，不访问已有连接密码。

## 界面夹具

运行 `npm run dev`，打开 `http://127.0.0.1:1420/scripts/windows-ui.html`。该页面使用 Tauri 官方 IPC mock 和真实应用界面，所有连接、目录、文件选择和传输均为模拟，不建立 SSH 连接，也不读写用户文件。发布构建只以根目录 `index.html` 为入口，夹具不进入安装包。

快捷键与 Linux 对齐：`Ctrl Shift K` 快速连接、`Ctrl Shift B` 主机栏、`Ctrl Shift T/W/F` 新建/关闭/搜索、`Ctrl Shift C/V` 复制粘贴、`Alt 1…9` 切换标签。终端中的 `Ctrl B/K` 保留给远端程序；中文输入法组词时不触发全局快捷键。

终端工具栏的 `−` 和 `+` 可将终端字号在 10–32 px 内逐级调整，点击字号恢复默认 15 px。焦点在终端内时也可使用 `Ctrl +`、`Ctrl −` 和 `Ctrl 0`。字号在所有终端标签间共享，并保存在本机。

## 当前验收状态

2026-11-18 已完成前端构建、15 项前端回归，以及模拟 Windows 界面的中文目录导航、新建目录、上传下载任务、暂停继续、收起后保留任务和 800×580 最小窗口布局检查。

用户手动关闭智能应用控制后，本机 Windows 11（build 26200）、Rust 1.100.0 MSVC 和系统 OpenSSH 9.5p2 已实际通过：Rust 编译、格式检查、工作区单元测试（13 项通过，1 项 SSH 集成夹具默认忽略后独立运行）、Clippy 全目标/全功能 `-D warnings`、9 类真实 SSH 认证及关闭场景、32 MiB SFTP 跨进程双向续传与完整 SHA256、Windows Credential Manager 随机条目的保存/读取/替换/删除。SSH 关闭覆盖正常退出、活动会话主动关闭及界面输出接收端提前销毁；最后一种场景会在终止 SSH 后完成 ConPTY 初始光标握手，避免控制台释放阻塞。SFTP 结果在 `target/windows-sftp.json`；SHA256 为 `dbd34077c44fc22c1545b06b95945cad4c5eb35376281a9cd7b0e8fbdc0120ec`。

字号调整前的 0.3.4 安装包（SHA256 `5be6675eeda06873f18a37fc53cfaf7180f1205e408814fea228aa16e7d9aa96`）实际以当前用户安装到工作区测试目录（禁用快捷方式），完成安装、启动、系统关闭和卸载；安装/卸载退出码均为 0，测试副本及卸载记录已清理。包内程序与最终 Release 程序仅有 Tauri 安装类型标记 `UNK → NSS` 的 3 字节差异，归一化后逐字节一致。原生窗口实测中文界面、文件栏、版本/引擎信息、主机栏及快速连接快捷键；原生程序未进行真实 SSH/SFTP 全界面端到端操作，真实协议与模拟界面采用分层验收。汇总保存于 `target/windows-acceptance.json`。产物未签名，不包含 `perf-probe` 测试入口。

包含字号调整的 0.3.4 安装包已重新构建，路径为 `target/release/bundle/nsis/HShell_0.3.4_x64-setup.exe`，校验值见同目录的 `.exe.sha256` 文件。本轮 20 项前端测试通过，浏览器夹具覆盖字号上下限、快捷键、终端行列重排、多个标签同步、重新加载保留设置，以及 800×580 窗口打开 SFTP 文件栏的布局。此轮未重新安装或重跑真实 SSH/SFTP 专项。

回环夹具在拒绝连接和关闭管道时打印 Paramiko 的连接重置提示，测试断言均通过。`npm ci` 仍报告已有的 4 项高危构建依赖告警（picomatch 依赖链），未强制降级 Vite。尚未验证 Windows 10、物理中文输入、生产 sshd / 跳板机、长时压力和本次共享传输修改的 Linux 专项；远端 CI 尚未运行。
