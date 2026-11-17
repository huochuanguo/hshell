# 验证记录

日期：2026-11-17。下方保留各版本地专项记录；自动发布的最新 Ubuntu amd64 版本及安装包见 [GitHub 发行版](https://github.com/huochuanguo/hshell/releases)，发布门槛见 [发布说明](releasing.md)。

## 0.3.1 文件类型的颜色区分

原生文件栏新增六类图标配色：目录柔金、链接青绿、普通文件灰、代码/配置冷蓝、压缩包淡紫、图片灰绿。目录与链接名称同步着色；其他文件名保持浅灰。目录同时使用文件夹形状及稍高字重，选中时名称沿用选中前景色。分类只读取已有条目与扩展名，不增加 SFTP 请求。按显示缩放加载六个图标并缓存为 Cairo surface，每行共享引用，绘制时不加载或重新着色。

Ubuntu 22 release 探针、Ubuntu 24 debug 探针均通过真实 SFTP 外观验证：八个条目覆盖全部类型，六个图标缓存完整，保存普通、选中目录和 800×580 窗口截图。图标及文字在默认与选中背景下的计算对比度均不低于 4.5:1。实际截图：[完整窗口](client-0.3.1-files.png)、[选中目录](client-0.3.1-files-selected.png)、[小窗口](client-0.3.1-files-small.png)。原始记录：[Ubuntu 22](files-appearance-0.3.1-ubuntu22.json)、[Ubuntu 24](files-appearance-0.3.1-ubuntu24.json)。

Ubuntu 22 的相同 release 探针通过双路 512 MiB 传输、暂停/继续、传输中退出确认、重启恢复和完整 SHA256 比对；10,000 项目录加载、传输期间终端输入与滚动、收起空闲文件栏后停止轮询均通过，无残留夹具 SSH。输入回显 P95 16.65 ms，滚动调度 P95 17.22 ms，大目录加载调度 P95 18.52 ms；为合成 GTK 事件至 VTE 解析/主循环调度，不是物理屏幕延迟，也不据此声称较 0.3.0 提速。见 [界面回归](sftp-gui-0.3.1-ubuntu22.json)。

`cargo check -p hshell-native --all-features`、`cargo clippy -p hshell-native --all-targets --all-features --locked -- -D warnings`、Rust 格式和修改后的 Python 编译检查通过。只改原生文件栏显示及对应验证入口，未修改终端/传输协议实现或 Web 样式；未重新执行核心故障注入、小时级压力或 Windows 实机验证，也未重新测量正式程序空闲内存。Ubuntu 24 外观探针为 debug 构建，其内存采样不能作为正式发布程序指标。

正式包在 Ubuntu 22 构建，路径 `target/native-ubuntu22/release/bundle/deb/hshell_0.3.1_amd64.deb`，1,181,676 字节；SHA256 `90718c42d261ecfc77898fce5aca54662d68a138c7cd3552a21aaa9b9a556ecc`。包内 ELF 与正式构建逐字节一致，最高 GLIBC_2.34，无测试探针和 WebKit/JavaScriptCore 依赖，见 [产物核查](release-native-0.3.1.json)。Ubuntu 22 一次性容器实际安装、运行并通过系统关闭协议退出（18 ms）；Ubuntu 24 使用独立 DBus 与临时配置运行同一包中的 ELF，关闭通过（14 ms）。首次复用宿主总线时程序转发至已有单实例，未将该次未生成测试窗口的结果计作通过。独立总线中的系统 portal 服务出现桌面后端与 PipeWire 提示，不影响客户端关闭。未安装或关闭用户本机客户端；旧 0.3.0 包校验值保持不变。

## 0.3.0 SFTP、断点续传与终端响应

新增共享 `hshell-sftp`，Ubuntu 右侧原生文件栏按需创建。浏览与传输使用独立 OpenSSH 子系统，文件数据不进入 GTK；最多两路传输、有界批次和固定缓冲。传输进度约 200 ms 唤醒一次，空闲不轮询。具体恢复与发布约定见 [设计](sftp-design.md)。

### 真实协议与故障恢复

一次性 Docker OpenSSH 服务仅发布回环端口，使用独立配置、密钥和 known_hosts，未更改用户客户端、服务器、SSH 配置或凭据。Ubuntu 24 / Ubuntu 22 均实际执行：

- 64 MiB 双向传输的完整 SHA256 核对；暂停后由新进程恢复上传、下载。
- 部分文件被篡改、源文件身份改变时拒绝恢复；同名目标默认保留，明确确认后原子替换。
- 完成发布后、删除记录前发生退出的恢复路径；空文件、中文、空格、特殊字符路径。
- 1 MiB 受限 tmpfs 上的磁盘写满、目标父目录不存在、下载临时路径变为符号链接时返回错误，保留最终目标。
- 冻结服务端后触发 30 秒无进展超时，重连恢复 256 MiB 上传，最终 SHA256 一致。
- 10,000 项目录，协议侧每批不超过 128 项；应用退出后未留下夹具 SSH 进程。

原始记录：[Ubuntu 24 核心](sftp-core-0.3.0.json)、[Ubuntu 22 完整专项](sftp-ubuntu22-0.3.0.json)。同机回环单次吞吐为 Ubuntu 24 上传约 102 / 下载 147 MiB/s，Ubuntu 22 约 109 / 173 MiB/s；包含本地写盘与发布，不代表真实公网、低速存储或其他服务器的吞吐。

### 原生界面与资源

两路 512 MiB 上传/下载同时运行时，通过真实 SSH、GTK 键盘与滚轮事件检查 VTE 回显和调度。另验证暂停/继续、传输中取消退出、再次确认退出、重开程序继续传输；三个 512 MiB 上传结果与下载结果做完整 SHA256 比较。800×580 窗口会收起主机栏，终端实际可用宽度 437 像素。目录与进度由有界事件队列处理，收起空闲文件栏后不保留轮询源。两平台 stderr 未出现 GTK WARNING / CRITICAL。

| 本次探针场景 | Ubuntu 24 X11 | Ubuntu 22 Xvfb |
| --- | ---: | ---: |
| 两路传输期间输入回显 P95 | 54.51 ms | 20.45 ms |
| 两路传输期间滚动调度 P95 | 17.22 ms | 18.01 ms |
| 10,000 项目录加载时调度 P95 | 19.70 ms | 18.10 ms |
| 传输阶段进程树 PSS 采样峰值 | 38.87 MiB | 96.37 MiB |
| 同阶段 RSS 采样峰值 | 109.11 MiB | 144.52 MiB |

输入指标从合成 GTK 按键至 VTE 已解析回显，滚动指标包含 16 ms 探针等待；均不是物理键盘或屏幕端到端延迟。PSS 与系统库共享程度有关，容器/Xvfb 的绝对值不能与桌面直接比较。Ubuntu 24 传输前单终端基线中位数 22.27 MiB PSS，Ubuntu 22 为 78.52 MiB；文件大小不决定应用缓冲大小。本次不将保留的分配器页称为内存泄漏，也不声称关闭文件栏后回到初始占用。

原始记录：[Ubuntu 24 界面](sftp-gui-ubuntu24-0.3.0.json)、[Ubuntu 22 界面](sftp-gui-ubuntu22-0.3.0.json)。截图：[完整窗口](client-0.3.0-sftp.png)、[800 像素窗口](client-0.3.0-sftp-small.png)。

终端专项回归另外通过 14 次连接、8 次额外开关、UTF-8 搜索/复制、粘贴确认、PTY 缩放、自然退出和反复窗口关闭；输入 P95 36.31 ms、持续输出时 45.15 ms，见 [终端回归](native-vte-0.3.0-ubuntu24.json)。系统钥匙串独立测试和 OpenSSH 密码、保存密码、指纹变化测试通过。SFTP 另行通过首次指纹确认、已保存口令自动提交、未保存口令提示和错误口令后再次输入，见 [SFTP 认证](sftp-auth-0.3.0.json)。

### 正式包与边界

Ubuntu 22.04 构建的无探针包：`target/native-ubuntu22/release/bundle/deb/hshell_0.3.0_amd64.deb`，1,181,196 字节，SHA256 `19556e3c206d40dff86ed641276349d5a4ce00a301ee03f991eb408205a52447`。包内 ELF 最高 GLIBC_2.34，不包含 `native-probe` 入口，不依赖 WebKit / JavaScriptCore，见 [产物核查](release-native-0.3.0.json)。

在一次性 Ubuntu 22 容器实际安装、运行、关闭通过，关闭约 18 ms；Ubuntu 24 从包中提取的同一 ELF 关闭约 13 ms。正式包程序两轮空窗口测量中位数为 17.55 MiB PSS / 61.13 MiB RSS，采样期间 CPU 为 0%，见 [正式程序空闲测量](memory-native-0.3.0-idle.json)。未覆盖用户现有安装；旧 0.2.3 包校验值未变。

`cargo fmt --all -- --check`、相关三个 crate 的 `cargo test --all-features --locked` 与 Clippy `-D warnings` 通过；其中常规单测的 OpenSSH 夹具为 ignored，已由 `scripts/ssh_smoke.py` 单独执行通过。SFTP 使用真实协议集成探针，不将零项 SFTP 单元测试声称为功能验证。新增 CI 步骤尚未在远端运行。

本次未重跑小时级 SFTP 长时测试；未测试 Windows SFTP 界面、Wayland、实际公网/跳板机 SFTP、目录递归传输或超大/非 UTF-8 路径。Windows 继续保留适配接口。目录同步与远程编辑按用户要求暂不加入。


## 0.2.3 列表增量更新、配置快照与兼容性验证

本轮产品代码集中在原生主机列表和本地连接准备。列表复用现有行、分组和检索字符串，按需更新文本、收藏图标和排序键；新行每批最多 16 条 / 6 ms，在空闲回调中继续。保存和退出取消旧批次；编辑按钮读取当前配置。配置快照只保存已校验的连接资料，Unix 每次复核文件身份和时间属性，外部替换、原位修改、损坏、删除、重建均会被检测；密码仍从系统凭据库读取，命名空间不变。

### 改动前后的同机测量

| 场景 | 原策略 | 优化后 |
| --- | ---: | ---: |
| 1,000 主机，收藏/改名分组/替换/恢复四项更新中位数 | 991.59 ms | 18.15 ms |
| 每次更新重建已有主机行 | 1,000 | 0 |
| 1,000 主机，首次 GTK draw dispatch 中位数（各 3 轮） | 1933.93 ms | 222.43 ms |
| 全部主机行填充 | 1101.57 ms | 1798.46 ms |
| 1,000 条配置，重复准备 1,000 次 | 1190.24 ms | 2.70 ms |

首次完整填充变慢是批处理让出 GTK 主循环的取舍；draw dispatch 不是物理屏幕呈现。空配置窗口约 201 / 203 ms，没有显著差异，约 14 ms 的字体注册不是本轮瓶颈，因此保留原字体加载。配置快照结果包含首次读取，平均每次只节约约 1.2 ms 的本地准备，不能称为 SSH 握手提速。

原始对照：`hosts-update-0.2.3-before.json` / `hosts-update-0.2.3-after.json`、`startup-0.2.3-before.json` / `startup-0.2.3-after.json`、`config-cache-0.2.3.json`。最终版本再次通过控件复用/排序/选择检查，四项更新中位数 41.53 ms、最大 92.01 ms（当时同机有其他压力测试）；见 `hosts-update-0.2.3-final.json`。

加载中途搜索/保存的最终回归在只创建 7 行时介入，最终仍为正确的 1,000 行、111 搜索结果，已有行重建 0。该场景紧接大批行填充与可见性变化，首次操作观察到 765.53 ms 主循环停顿；大型 GTK 列表布局仍有提升空间，不能把 18 ms 宣称为所有交互的延迟。见 `hosts-interleave-0.2.3.json` / `hosts-interleave-0.2.3-final.json`。

### 持续输出采样与真实程序

系统 `perf_event_paranoid=4` 阻止 perf 采样，未修改内核设置，使用临时提取的用户态 Callgrind。两个真实回环 SSH 会话输出期间清零/截取计数，关闭频繁文本提取和输入探针以减少诊断干扰。self instruction 中 VTE 62.64%、Cairo 12.86%；这是指令比例，不能当作正常运行的 wall-time CPU 占比。未发现应用层重复输出转换，未据此更改 Unicode、双向文本或回滚行为。诊断构建不参与正常延迟/内存预算。

数据：`output-profile-0.2.3.json`、`profiles/output-0.2.3.callgrind`、`output-profile-summary-0.2.3.json`。分析器排除 `calls=0` 后的非零 inclusive cost，并以末尾 `totals:` 校验 self cost 总和；两个持久回归用例通过。

Ubuntu 22 / VTE 0.68 / Xvfb 与 Ubuntu 24 / VTE 0.76 / X11 均通过真实 sshd、tmux 3.2a 和 vim.tiny 检查：通过 GTK 分发的 Ctrl+B、C 新建第二个 tmux 窗口；vim 中文粘贴，连续三次窗口缩放后逐字节验证保存的 UTF-8 文件；160 个 GTK 滚轮事件实际移动回滚视口，之后输入、PTY 尺寸和自然退出均正常，退出无残留测试 SSH。见 `native-programs-0.2.3-ubuntu22.json` / `native-programs-0.2.3-ubuntu24.json`。

Ubuntu 24 的独立 DBus / IBus / libpinyin 实际组词提交三次“你好”，确认拼音预编辑不进入 SSH，空格提交后的解析回显约 5.63 / 21.42 / 21.57 ms；见 `native-ime-0.2.3-ubuntu24.json`。测试先在其拥有的临时 IBus context 初始化引擎，再连接真实 GTK/VTE 输入上下文，避免在无 context 时调用 SetGlobalEngine 失败。未切换用户的日常输入法。上述键盘、滚动和输入法均为程序化事件，不能代替物理键盘到显示器的延迟验收；Ubuntu 22 的输入法、Wayland 和 Windows 实机仍待独立验证。

### 长时测试与当前发布状态

原 8 小时计划按用户要求缩短为 2～3 小时。Ubuntu 22 容器 / Xvfb / VTE 0.68 的最终 0.2.3 测试候选，同一进程已观察到 **10,503.71 秒（约 2 小时 55 分钟）** 的 12 会话持续输出。源文件哈希与独立补测的二进制哈希均与启动清单一致；原计划及修订范围见 `native-soak-0.2.3-manifest.json`。

351 条有效资源记录的采样间隔不超过 30.02 秒。排除前 10 分钟预热后，PSS 为 **110.68～110.77 MiB**，RSS 为 **208.79～208.88 MiB**；10～40 分钟与最后 30 分钟各 60 条样本的中位数均只增加 **0.09 MiB**。稳定阶段主进程句柄保持 **68**、线程保持 **15**，应用及 SSH 进程树 CPU 中位数约单核 **33.43%**。这是 12 会话持续输出的固定负载，不能当作空闲占用或所有桌面的内存承诺。容器和宿主的 PSS 受系统库共享影响，不能直接横比。

缩短时通过实际 WM_DELETE_WINDOW 和退出确认关闭测试窗口，测试应用及 SSH 全部退出，见 `native-soak-0.2.3-shortened-close.json`。原固定时长探针没有自然走到收尾，**没有完整长时输入延迟直方图**；关闭脚本约 10.46 秒的记录包含开发探针等待中断条件的时间，不能当作正式包关闭延迟。原始 JSONL 最后一条位于关闭后，SSH 子进程消失使累计 CPU 计数下降，产生负值；保留原始记录并在汇总中明确排除，未将其混入负载统计。

随后用**同一二进制**独立完成 30 秒 / 12 会话 / 64 次开关回归，共 **76 条连接**，隐藏终端输出全部完成、3 次合并回收、退出无残留。补测输入回显 P95 **39.5 ms**、回收期间 **18.25 ms**，GTK 调度 P95 **28 ms**、最大 **53.36 ms**；这些只代表补测。保留一会话时 PSS 84.03 → 83.87 MiB、句柄 13 → 13、线程 4 → 4；全部关闭后句柄 8、线程 3。见 `native-soak-0.2.3-lifecycle.json`。

`validate_native_soak.py` 已按最低 7,200 秒验证资源、来源一致性、真实关闭和独立生命周期结果，汇总见 `native-soak-0.2.3-summary.json`。6 项验证器回归通过，覆盖不足时长、采样断档、运行中负 CPU、内存/句柄/线程增长、不同二进制及未完成清理等拒绝条件。后续标准长时命令改为 10,800 秒，可一次自然完成 3 小时及开关收尾，不需要沿用本次中途缩短流程。

Ubuntu 22/24 各自的 14 连接保存密码完整回归通过，覆盖密码保留/替换/清除、指纹、表单、搜索复制、粘贴确认、尺寸同步、自然退出、取消/重复关闭和测试连接清理，见 `native-0.2.3-ubuntu22.json` / `native-0.2.3-ubuntu24.json`。工作区单测在 Ubuntu 22 容器通过：core 15 通过、协议夹具 1 项默认忽略并由 `ssh_smoke.py` 的 7 场景实际通过；原生默认 3 项通过，Ubuntu 24 的全部功能 4 项通过；系统钥匙串生命周期独立检查通过。workspace Clippy 和原生/core 全部功能 Clippy、Rust 格式、Web 构建/11 项测试/格式检查通过。宿主首次 workspace 检查缺少 WebKit/Soup pkg-config 开发文件，随后在具有真实开发依赖的 Ubuntu 22 容器完成，无全局安装改动。

无探针正式候选包已在 Ubuntu 22 构建：`target/native-ubuntu22/release/bundle/deb/hshell_0.2.3_amd64.deb`，929,276 字节；SHA256 `6af4c83ad1ed39059e677919434754061e1d59d0fbb5de350afbd1b237a2e424`。最高 GLIBC_2.34，包内 ELF 与构建产物一致，不含 native-probe，不链接 WebKit/JavaScriptCore；见 `release-native-0.2.3.json`。Ubuntu 24 从包内提取的正式程序通过窗口关闭，约 14 ms；两个实际映射空窗口采样约 16.16 MiB PSS / 59.95 MiB RSS，见 `memory-native-0.2.3-idle.json`，不将系统库共享波动称作本轮内存优化。

Ubuntu 22 一次性容器以普通用户启动实际安装后的程序，完成 `qizhan-ssh 0.2.0 → HShell 0.2.1 → 0.2.2 → 0.2.3` 升级，配置 JSON 字节不变，旧包/旧命令/旧桌面入口移除，4 个字体、8 个 PNG、SVG 和 2 份字体许可完整。容器默认排除文档文件，验证时仅对容器 dpkg 启用该包文档的 path-include。实际关闭约 17 ms，已映射空窗口约 47.46 MiB PSS / 98.62 MiB RSS；容器和宿主的系统库共享程度不同，不能直接横比。见 `release-0.2.3-upgrade.json`。apt 提示未安装 apt-utils，不影响安装；宿主没有执行安装。

Ubuntu 24 最终三分钟 / 12 会话 / 64 次开关回归通过，共 76 条连接。持续输出输入回显 P95 36.25 ms，回收期间 53.0 ms；GTK 调度 P95 17.75 ms、最大 48.96 ms；关闭到一会话后 PSS 25.94 → 26.14 MiB，主进程句柄 21 → 21、线程 7 → 7，全部关闭后句柄 16、线程 6，SSH 残留 0。见 `native-stress-0.2.3-ubuntu24.json`；不是 8 小时完成证明。

本轮未改布局/配色，未新增浏览器视觉验收。CI 已加入主机更新、批次交错、分析器和长时验证器回归，远端 CI 未触发。本次按修订范围收尾，不代表原 8 小时探针自然完成；完整进度见 `optimization-progress.md`。缩短验收未修改产品源码，安装包校验值不变；未安装到用户本机，未关闭用户应用；目录无 Git 元数据，未提交或推送。

## 0.2.2 多会话资源回收与持续输出回归

本次产品改动集中在 GTK 前端的标签关闭路径：**关闭部分标签后，即使仍有连接存活，也延迟合并回收空闲堆页**。连续关闭共用一个 500 ms 任务；新连接和退出取消尚未执行的任务；实际 `malloc_trim` 在后台线程执行。保留原有 3,000 行回滚、12 标签上限及 VTE 直接读取 PTY 的路径。没有修改 SSH 认证、配置格式或凭据命名空间。

### 先测量再修改

为原有 0.2.1 产品路径增加独立开发压力探针：12 条真实回环 SSH 同时持续输出中文彩色日志，活动终端交替输入及滚动；结束后关闭 11 个标签，保留 1 个，再反复打开、填充、关闭连接，最后关闭全部标签。监测进程树 PSS/RSS、CPU、主进程文件句柄和线程数；探针代码不进入发布包。

原始 20 秒、32 次开关的基线没有发现线程/句柄累积，但保留一会话后的空闲堆页尚未归还。最小回归（12 标签、5 秒、1 次开关）使用本机诊断预算 28 MiB PSS：

```text
python3 scripts/native_vte.py target/hshell-stress-baseline-0.2.1 \
  --stress --tabs 12 --seconds 5 --cycles 1 --max-retained-pss 28 \
  --output /tmp/hshell-reclaim-partial-before.json
AssertionError: Retained memory exceeds 28.0 MiB: 30.29 MiB
```

同一测试流程在改动后通过，实测 25.29 MiB。该预算用于相同本机环境的诊断，不能作为所有桌面的固定内存承诺。对照文件为 `stress-0.2.2-partial-before.json` / `stress-0.2.2-partial-after.json`。

随后重跑完整 20 秒、32 次开关流程，保持相同测试代码、负载与桌面环境，只改变关闭后的回收策略：

| 阶段 | 原 0.2.1 策略 PSS MiB | 新策略 PSS MiB |
| --- | ---: | ---: |
| 空窗口 | 18.13 | 18.05 |
| 12 会话填满回滚 | 47.15 | 47.48 |
| 关闭到剩 1 会话 | 30.34 | 25.21 |
| 继续开关 32 次 | 30.85 | 25.35 |
| 关闭全部 | 22.25 | 22.16 |

主要收益是部分关闭后的回收，减少约 **5.13 MiB / 17%**；没有将空窗口、峰值占用或输入延迟的运行波动宣称为提升。文件句柄在单会话阶段均为 21，全部关闭后回到 16。原始数据为 `stress-0.2.2-before.json` / `stress-0.2.2-after.json`，均为带同一开发探针的完整客户端，不是最小终端原型。

### Ubuntu 24 的最终压力结果

以最终 0.2.2 测试构建运行 12 条 SSH、180 秒持续输出、64 次开关，并在部分关闭后的回收期间保持另一个会话输入/输出。服务端累计发送约 **344.8 MiB** 压力日志、共完成 **76 条 SSH 连接**：

- 稳定阶段（20–170 秒）的进程树 PSS 为 **47.11–47.19 MiB**，RSS 约 172.8–172.9 MiB；CPU 中位数约单核 27.5%。PSS 按共享页分摊，RSS 重复计算共享页，不含 GPU 设备显存或独立系统钥匙串服务。这些是固定夹具负载下的短时记录。
- 12 会话时主进程句柄 76 个；保留 1 会话后为 21；再开关 64 次仍为 21；全部关闭后回到 16。单会话阶段线程数 7 → 7，全部关闭后为 6，没有随打开次数累积。
- 保留一会话后 PSS 25.40 MiB，64 次开关后 25.56 MiB，全部关闭后 22.28 MiB。分配器和字体缓存仍保留部分内存，不宣称回到冷启动数值。
- 整个流程实际完成 **3 次**堆页回收，验证连续关闭和快速重连时的任务合并/取消，未按每个关闭标签重复执行。
- 多会话输出中输入回显 P95 **35.66 ms**，部分关闭后仍持续输入/输出时 **51.96 ms**；GTK 16 ms 调度回调 P95 17.72 ms，最大 32.16 ms。输入指标截止 VTE 解析回显，包含探针轮询；调度指标不是屏幕帧时长。这些指标不能替代物理键盘、输入法或显示延迟测量。
- 隐藏终端均收到结束标记，回滚上限保持 3,000 行，退出后无残留测试 SSH 进程和服务端连接。

完整数据：`native-stress-0.2.2-ubuntu24.json`。这次是三分钟持续输出与有限次数回归，不能据此证明多小时或所有生产环境均稳定。

### 兼容与功能回归

- Ubuntu 22 / VTE 0.68 / Xvfb 完成 12 会话、30 秒输出、32 次开关，共 44 条 SSH 连接。稳定单会话的句柄数 13 → 13、线程 4 → 4，结束后句柄恢复 8；同样完成 3 次合并回收。输入回显 P95 40.78 ms，回收期间 13.85 ms；不同发行版、VTE 和显示环境下的指标不直接横比。数据见 `native-stress-0.2.2-ubuntu22.json`。
- Ubuntu 22/24 各通过一次 14 连接的保存密码完整工作流：系统钥匙串密码保留/替换/清除、首次指纹、表单增改删、多标签 UTF-8、终端搜索、复制、多行粘贴取消/接受、PTY 尺寸、自然退出、重复关闭和取消后再次关闭；均无残留测试 SSH。见 `native-0.2.2-ubuntu22.json` / `native-0.2.2-ubuntu24.json`。
- `cargo test --workspace --locked`：原生 3 项、核心 12 项通过；默认忽略的协议夹具通过 `scripts/ssh_smoke.py` 单独执行，7 种真实 OpenSSH 场景通过；`scripts/keyring_smoke.py` 的系统钥匙串生命周期通过。
- workspace Clippy 与原生/核心全部功能 Clippy、Rust 格式、Python 编译检查通过；Web 前端生产构建、11 项测试和格式检查通过。Ubuntu 前端布局/样式没有变化，本轮没有新增浏览器或人工物理输入验证。
- 新压力回归已加入 CI 定义；远端 CI 未触发。探针在开发构建中统计回收次数，发布版不含计数器、压力命令或测试服务器入口。

### 正式包、升级与测量校验

正式包在 Ubuntu 22.04 构建，不启用 `native-probe`。安装包为 `target/native-ubuntu22/release/bundle/deb/hshell_0.2.2_amd64.deb`，918,120 字节（约 0.88 MiB），SHA256 `92869378de385b05d2b734d5d5821ff6f5bc409b3de587c2cda1b60280317753`。包内 ELF SHA256 为 `8ea4e33ac75fc4ea189484105af98f3d360786b38378eb706b65bbecabeccd25`，最高 glibc 要求为 GLIBC_2.34，不依赖 WebKit / JavaScriptCore，不含压力测试入口。旧 0.2.0 / 0.2.1 安装包校验值未变，详见 `release-native-0.2.2.json`。

一次性 Ubuntu 22 容器完成 `qizhan-ssh 0.2.0 → HShell 0.2.1 → 0.2.2` 实际安装升级，原配置 JSON 字节不变；旧命令和旧桌面文件移除，新桌面入口、8 个 PNG 尺寸、SVG、4 个字体和 2 份许可完整。普通用户通过 Xvfb 启动安装后的 `/usr/bin/hshell`，系统窗口关闭至正常进程退出约 17 ms。Ubuntu 24 运行相同包提取的实际程序，也通过系统关闭协议退出，约 14 ms。耗时是短时记录，不是跨机器承诺。容器升级记录见 `release-0.2.2-upgrade.json`。

Ubuntu 24 实际正式 ELF 的两轮空窗口采样约 **18 MiB PSS / 61 MiB RSS**，均核对了属于测试子进程的 1280×820 已映射主窗口。原始结果见 `memory-native-0.2.2-idle.json`。Ubuntu 22 Xvfb 正式包为 **75.59 MiB PSS / 100.31 MiB RSS**；系统库共享程度不同，不能直接横比或承诺所有环境固定占用。短采样窗的 CPU ticks 增量为零，受计数粒度限制，不代表持续零 CPU。

内存测量脚本现在必须验证测试 PID 的已映射 X11 主窗口，在采样前后再次确认窗口存在；仅进程存活不足以认定启动成功。没有主窗口的进程会被拒绝且不生成测量文件，负例检查已通过。首次将独立 DBus 与宿主运行目录混用时，portal / GVFS 服务冲突导致正式包未生成窗口；该次约 9 MiB 的启动前记录已废弃，以上结果来自正常桌面会话、隔离连接配置的重新验证。没有修改全局桌面服务。

本机 apt 安装模拟确认替换现有旧包且无需新增依赖；未替用户安装或关闭其客户端。配置路径及系统凭据服务名保持稳定，既有密码的读写兼容由 Ubuntu 22/24 的隔离钥匙串功能回归覆盖。隔离测试会话可能出现 portal / PipeWire / DBus 服务提示，容器 apt 会提示未安装 apt-utils，实际客户端正式关闭检查无 stderr。Windows 实机、Wayland、物理键盘/输入法、生产 sshd / vim / tmux、不同显卡和多小时压力尚未覆盖。本目录不是 Git 仓库，未提交或推送。

## 0.2.1 HShell 名称、图标与进一步优化

显示名统一为 **HShell**，Linux 包名、命令和桌面文件为 `hshell`；Windows 预留前端显示名及主程序名同步调整。图标将 H 与终端提示符结合，使用石墨灰、浅灰白和冷蓝；矢量源 `src/assets/hshell.svg`，提供 8 种 PNG 尺寸、SVG 与 Windows 多尺寸 ICO。预览见 `hshell-icon.png`，原生截图见 `client-native-0.2.1.png` / `client-native-0.2.1-small.png`。

### 同机对照测量

在 Ubuntu 24 X11 上，对**同一个测试二进制**分别启用/禁用关闭全部终端后的延迟堆页回收，均完成 14 条回环 OpenSSH 连接、4 标签填满回滚、8 次打开关闭循环：

| 关闭全部终端后 | 禁用回收 | 启用回收 |
| --- | ---: | ---: |
| PSS MiB | 31.69 | 24.31 |
| RSS MiB | 82.33 | 74.94 |

本次 PSS 少约 7.38 MiB（23%）。空闲堆页回收只在最后一个标签关闭 500 ms 后由后台线程运行，重新连接会取消尚未执行的任务，不清除存活会话的回滚。输入、持续输出下输入、滚动调度与退出检查均通过，不能将两次运行的延迟波动当作本改动带来的提速。两份同二进制原始数据为 `perf-0.2.1-reclaim-control.json` / `perf-0.2.1-reclaim-enabled.json`。

主机过滤在修改前每次重建 GTK 控件；修改后只在配置变更时重建，搜索/收藏切换复用已创建的行和检索字符串。1,000 条临时连接、5 个查询、3 轮切换，修改前/后同步工作中位数 **98.19 / 2.58 ms**，最大 **1129.29 / 16.38 ms**。清空搜索时不再重建 1,000 行。这些数字测量过滤调用，不包括搜索框的 debounce、下一次布局与绘制，不能当成完整输入延迟或 FPS。初次创建 1,000 行的开销仍然存在。原始数据为 `perf-0.2.1-hosts-before.json` / `perf-0.2.1-hosts-after.json`。

终端查询未变化时复用 VTE 持有的编译结果；连续上一处/下一处不再重复编译。原生回归实际执行 100 次向后与 100 次向前查询，确认滚动位置推进、中文匹配、未命中和清除；没有单独发布此项的速度增益数字。

### 最终版本资源与回归

实际 Ubuntu 22 发布包中的 ELF 在 Ubuntu 24 桌面使用隔离配置，两轮空窗口中位数 **17.79 MiB PSS / 60.53 MiB RSS**，见 `memory-native-0.2.1-idle.json`。约 3 秒采样窗的 CPU ticks 增量为零，受计数粒度限制，不代表恒为零 CPU。多会话测试构建在同一桌面：1 条保存密码 SSH 为 23.35 / 73.68 MiB PSS/RSS，4 条填满回滚为 34.24 / 106.84。

Ubuntu 22 Xvfb 的实际发布包空闲为 **75.72 / 100.56 MiB PSS/RSS**；其不同系统库共享程度使 PSS 高于当前桌面。不能把本机 18 MiB 宣称为所有机器的固定占用。统计包括客户端和 SSH 子进程，排除独立钥匙串服务及 GPU 设备显存。

- `cargo test --workspace --locked`：原生 3 项、核心 12 项通过；默认忽略的协议夹具通过 `scripts/ssh_smoke.py` 单独执行，7 种真实 OpenSSH 认证/指纹场景通过。`scripts/keyring_smoke.py` 保存/读取/替换/删除通过。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`、原生和核心的 `--all-features --all-targets`、`cargo fmt --all -- --check` 通过。宿主机缺少 WebKit 开发链接名，测试使用项目外临时链接目录中的系统 `.so.0`，没有安装或改动本机系统包。
- `npm run build`、11 项 `npm run test:frontend`、`npm run format:check`、Python 编译检查通过。
- Ubuntu 24 密钥认证和保存密码、Ubuntu 22 保存密码工作流均完成 14 条真实 OpenSSH 连接；密码夹具计数 14 次成功密码认证和 1 次指纹确认。表单增改删、密码保留/替换/清除、4 个终端中文彩色输出、持续输出下输入/滚动、尺寸变化、Unicode 复制、粘贴确认、自然退出、取消后重新关闭及无残留 SSH 全部通过。
- Ubuntu 24 保存密码场景输入 P95 36.52 ms、持续输出时 36.33 ms；Ubuntu 22 分别 32.10 / 38.70 ms。指标截止 VTE 解析回显，含探针轮询，不是物理键盘到屏幕延迟。滚动 GTK 定时调度 P95 约 17.2 ms，不代表显示帧时长。完整原始数据为 `native-ubuntu24-0.2.1.json`、`native-ubuntu22-0.2.1.json`。
- 主机过滤夹具在 Ubuntu 22/24 均通过 1,000 条旧格式记录加载、大小写搜索、未命中、清空搜索、精确匹配和 500 条收藏筛选；文件沿用 `io.qizhan.ssh` 路径、不含后加的 `passwordSaved` 字段，过滤前后文件未改变。见 `perf-0.2.1-hosts-final.json` / `perf-0.2.1-hosts-ubuntu22.json`。
- 浏览器实际查看 HShell 品牌图标、窗口/页面标题和关于窗口 0.2.1 版本，未出现控制台 warn/error。原生 GTK 截图核对品牌、图标与字体。样式复查无装饰性顶部色条、渐变、发光轮廓或 emoji 图标。

### 改名升级与最终产物

配置路径 `io.qizhan.ssh`、系统凭据服务名 `io.qizhan.ssh.password` 及 Tauri 安装标识保持稳定；内部历史标识用于延续原有连接/密码。安装包通过 `Conflicts` / `Replaces` 替换旧 `qizhan-ssh`。

在一次性 Ubuntu 22 容器中先安装旧 0.2.0，再通过 `apt-get install` 安装 HShell 0.2.1：旧包、旧命令与旧桌面入口移除，新命令、桌面文件、8 个 PNG 尺寸和 SVG 存在，4 个字体和 2 份 OFL 许可完整，原有连接 JSON 字节不变。容器默认排除文档安装，测试在一次性容器中为本包添加文档包含规则后验收许可；发布包本身始终包含两份许可。普通用户 Xvfb 启动 `/usr/bin/hshell` 后通过系统关闭协议正常退出，约 17 ms。见 `release-0.2.1-upgrade.json`。

同一包提取的实际 ELF 在 Ubuntu 24 通过系统关闭协议正常退出，约 14 ms。本机 `apt-get -s install` 显示移除旧包、安装 HShell、无新增依赖；没有替用户安装、关闭旧客户端或读取其保存密码。宿主机已安装的包仍为 `qizhan-ssh 0.2.0`。

安装包：`target/native-ubuntu22/release/bundle/deb/hshell_0.2.1_amd64.deb`，918,024 字节（约 0.88 MiB）。SHA256：`6fbcbd4ce6a0b9f8d68d71ac919fed960fbd0c51056d0f4a0418565aba0745cf`。包内 ELF SHA256：`12e724132cc0ce418f06ac5170e76e1d6b27af9dc0dfa5b7358a8df93af627ed`；最高 glibc 符号 GLIBC_2.34，无 WebKit/JavaScriptCore 依赖、无开发探针入口；旧 0.2.0 包的校验值不变。见 `release-native-0.2.1.json`。

隔离 DBus 的桌面 portal 仍会报告测试环境缺少 PipeWire，退出时可能报告 bus 断开；被测客户端捕获的 stderr 为空。容器 apt 提示未安装 apt-utils，安装仍成功。远端 CI、Windows 实机、Wayland、物理输入法、不同显卡、生产 sshd/vim/tmux 与多小时多会话压力尚未覆盖。当前工作目录不是 Git 仓库，未提交或推送。

## 0.2.0 原生终端与资源占用

Ubuntu 改为 GTK 3 + VTE + Rust + 系统 OpenSSH。窗口、连接编辑器、主机栏、标签、终端、搜索、认证和剪贴板均走原生路径；Windows 保留原有 Tauri 前端和共享核心。没有修改本机已安装的 0.1.4 或用户连接数据。

### 内存反馈与实测

旧发布版用隔离配置、1280×820 空窗口测量进程树。下述命令以 120 MiB PSS 为诊断阈值，实际约 141.81 MiB，按预期失败：

```bash
python3 scripts/native_memory.py /tmp/qizhan-release-0.1.4/usr/bin/qizhan-ssh \
  --runs 2 --warmup 4 --samples 6 --max-pss 120 \
  --output /tmp/qizhan-memory-idle-baseline.json
```

同一桌面分别禁用 WebKit 合成、禁用硬件加速、改变缓存策略，PSS 仍在约 141.7–142.0 MiB。测试 shim 和最小 C 原型只用于诊断，没有进入产品。由此确认本轮主要瓶颈是 WebKit 运行时，采用原生终端；没有将最小 VTE 原型的约 12 MiB 占用冒充完整客户端。

以 Ubuntu 22.04 构建的实际发布包提取 ELF 后，在同一 Ubuntu 24.04 X11 桌面以相同窗口、采样参数复测：

| 场景 | PSS MiB | RSS MiB |
| --- | ---: | ---: |
| 0.1.4 发布版，空闲，两轮中位数 | 141.81 | 440.04 |
| 0.2.0 发布版，空闲，两轮中位数 | 18.45 | 60.48 |
| 0.2.0 开发探针，1 条密钥认证 SSH | 22.67 | 72.77 |
| 0.2.0 开发探针，4 条 SSH，均写入 4,000 行彩色中文 | 33.69 | 105.98 |
| 0.2.0 开发探针，1 条保存密码的 SSH | 23.38 | 73.96 |
| 0.2.0 开发探针，4 条保存密码的 SSH，填满回滚 | 34.50 | 107.15 |

同场景空闲 PSS 下降约 87%。表中后四项是测试构建，包含相同产品 UI / PTY 路径和开发测量代码；正式包没有测试入口。统计包括客户端及 SSH 子进程，不包括独立系统钥匙串服务；PSS 按共享页分摊，RSS 累加会重复统计共享页，不含 GPU 设备显存。不同连接负载和共存进程不能直接作同比。空闲两轮各约 3 秒采样 CPU ticks 增量为 0，受 10ms 计数粒度限制，不能推导所有情况下 CPU 恒为零。

Ubuntu 22.04 容器 Xvfb 的保存密码探针：空闲 72.92 / 100.33 MiB PSS/RSS，1 条 SSH 为 81.38 / 113.07，4 条填满回滚为 94.28 / 145.53。它加载另一套发行版系统库且共享较少，不能与本机桌面的 18 MiB 直接横比。无完整辅助功能服务的无头测试显式设置 `NO_AT_BRIDGE=1`，未关闭产品的辅助功能支持。

原始数据：`memory-webkit-0.1.4-idle.json`、`memory-native-0.2.0-idle.json`、`native-ubuntu24-probe.json`、`native-ubuntu24-credentials.json`、`native-ubuntu22-credentials.json`。截图 `client-native-0.2.0.png` / `client-native-0.2.0-small.png` 来自测试客户端自身的 GTK/Cairo 绘制，内容为回环夹具，不含用户服务器信息。

### 实际通过的回归

- Ubuntu 24 密钥、Ubuntu 24 保存密码、Ubuntu 22 保存密码各运行一次完整工作流，每次 14 条真实 OpenSSH 连接。密码夹具拒绝公钥并计数 14 次成功密码认证，避免“有私钥所以密码未实际使用”的假通过。首次信任须出现一次含预期 SHA256 指纹的确认窗口。
- 实际编辑器验证无效字段拒绝、创建、保存、密码保留/替换/清除/重加、删除及凭据删除；密码测试创建独立 DBus 和临时 GNOME Keyring，不访问用户凭据。
- 四标签分别写入彩色中文、填满 3,000 行回滚，验证隐藏标签解析、Unicode 搜索与复制；持续输出 5,000 行时滚动并测试输入回显。
- Ubuntu 24 密钥场景输入回显 P95 36.68ms、持续输出时 39.68ms；保存密码场景分别 32.24ms / 51.36ms；Ubuntu 22 分别 32.19ms / 43.10ms。指标截止 VTE 解析回显，包含探针轮询和文本提取；不代表物理键盘到屏幕延迟，也不与旧版不同夹具的延迟作同比。
- GTK 主循环在滚动下 16ms 定时回调 P95 约 17.1–17.4ms。这是调度响应指标，不是显示帧时长或合成器 FPS；没有将它冒充真实滚轮绘制延迟。
- 实际窗口缩放 1280×820 → 800×580 → 1280×820，服务端核对 PTY 尺寸变化；真实剪贴板多行粘贴取消/接受；自然退出保留输出且释放后端。
- 8 次连接打开/关闭循环后，可比单连接阶段 PSS 增长分别 0.15、0.38、0.29 MiB，没有随每次打开持续累积整个终端缓冲。经历压力、复制与窗口变化后分配器/字体缓存仍会保留部分内存，不宣称关闭标签立即回到冷启动占用。
- GTK 窗口关闭测试覆盖确认期间重复请求、取消保留会话、再次接受、窗口真正销毁、进程正常退出；三次完整工作流均无残留测试 SSH 子进程或服务端连接。
- `cargo test --workspace --locked`：原生解析 3 项、核心 12 项通过；默认忽略的协议夹具另以 `python3 scripts/ssh_smoke.py` 执行通过，覆盖密码、错误密码后回退、取消指纹、私钥、主机密钥变化与清理。`python3 scripts/keyring_smoke.py` 的系统钥匙串生命周期通过。
- `cargo clippy --workspace --all-targets --locked -- -D warnings` 和原生/核心 `--all-features` 检查通过；`cargo fmt --all -- --check`、Python 编译检查通过。
- 保留的 Web 前端 `npm run build`、`npm run test:frontend`（11 项）、`npm run format:check` 通过。此次原生 UI 用实际 GTK 工作流与绘制验证，没有新增浏览器验证。
- 样式扫描没有装饰性色条、渐变、发光轮廓或 emoji 图标。

### 发布包验证

Ubuntu 22.04 release 构建，不启用 `native-probe`。包内 ELF SHA256 为 `bff76e7f1f909964b4383a9973a3b4b10b14d92f504c865369634a6e974115bc`，glibc 最高要求 GLIBC_2.34；动态链接 GTK/VTE，不含 WebKit 或测试探针入口。TTF 字体与两份 OFL 许可已随包分发。

安装包：`target/native-ubuntu22/release/bundle/deb/qizhan-ssh_0.2.0_amd64.deb`，898,552 字节，约 0.857 MiB。SHA256：`37803d3341958e502ee907e163c3d5df719785858d5fd6682c4c11413128b1b2`。

Ubuntu 22 容器安装成功，以普通用户运行实际 `/usr/bin/qizhan-ssh` 并通过系统 `WM_DELETE_WINDOW` 关闭，退出码 0，约 18ms。Ubuntu 24 本机运行包内提取 ELF，同一路径两次正常关闭约 19ms / 16ms。发布版内存采样也使用该实际 ELF。耗时只是短时记录，不是跨机器承诺。`apt-get -s install` 在本机确认 0.1.4 → 0.2.0 升级，0 个新增依赖；没有替用户执行本机安装。最终包校验见 `release-native-0.2.0.json`。

隔离 DBus 中桌面 portal 启动时会报告缺少 PipeWire，结束时附带 bus 断开信息；它们来自测试会话服务，不是客户端 GTK 警告，客户端捕获的 stderr 为空。构建与最终打包无未处理警告。远端 CI 尚未实际运行。Windows、Wayland、真实物理输入/输入法、各种显卡、生产 sshd / vim / tmux 和长时间压力仍待实机验收；Ubuntu 22 的 Xvfb 不等同完整桌面人工验收。

## 0.1.4 配色与字体

正式客户端采用石墨灰与冷蓝配色，主机树、标签、快速连接、表单、认证窗口、错误和终端共用语义颜色。终端 ANSI 16 色也从同一份 CSS 变量读取。基础界面字号 13px；IBM Plex Sans 400/500 用于界面，JetBrains Mono 400/600 用于终端，终端默认 15px、1.36 倍行高并关闭连字。中文由系统 Noto CJK / Windows 字体回退。四个本地 WOFF2 子集共 106,228 字节，随应用嵌入，无远程字体请求；两份 OFL 许可随包分发。创建终端前等待字体加载，避免先用回退字形测量字符网格。

本轮实际验证：

- `npm run build`、`npm run test:frontend`（11 项）、`npm run format:check`、`cargo fmt --all -- --check` 通过。
- `cargo test -p ssh-core --locked`：12 项通过、1 项协议夹具按设计忽略；本轮另用下述真实 OpenSSH 回环测试覆盖输入和退出，没有重跑密码、MFA 或钥匙串场景。
- `cargo clippy --workspace --all-targets --locked -- -D warnings` 通过，无警告。
- 浏览器检查实际客户端的 1280×820 和 800×580 窗口，验证新建连接表单、小窗口表单滚动、中文及易混字符显示、终端搜索高亮。终端实际计算字体为 Qizhan Mono、15px；浏览器无 warn/error 日志。浏览器样例不建立 SSH 连接、不保存密码，只存在内存中。截图见 `docs/client-0.1.4.jpg`。
- Ubuntu 24.04 本机 X11：测试专用构建经原生 WebKit/Tauri/PTY/系统 OpenSSH 完成回环交互。两条密钥认证连接成功，48 个逐字符回显全部核对，压力阶段发送 813,600 字节中文彩色日志。常规字符回显 P95 13ms、持续日志下 39ms；滚轮绘制 P95 分别 34ms / 24ms。通过原有预算，没有改变渲染器、IPC 背压及回滚限制。
- 本地 PTY 测试：Ubuntu 24 标签切换 P95 17ms、输入回显 P95 11ms、1,040,000 字节往返 1,064ms；Ubuntu 22 容器 Xvfb 分别为 27ms、8ms、853ms。两套原生 WebKit 均报告两种终端字重加载成功，15px、1.36 行高。原始性能结果见 `docs/performance-0.1.4.json`。
- 退出回归：Ubuntu 24 X11 与 Ubuntu 22 容器 Xvfb 各通过 6 场景，包括无连接、两条连接、取消后重试、重复关闭、连接编辑弹窗、信息弹窗；每个平台 10 条真实 SSH 连接均已断开，无遗留测试 SSH 进程。原始结果见 `docs/window-close-0.1.4.json`。elapsedMs 包含启动与连接，不是纯关闭延迟。
- 最终样式已扫描：保留的 `border-top` 只作区域分隔，未添加装饰色条、渐变或发光边框。

上述性能为每场景一次短时回归结果，不能用来断言相对 0.1.2 的性能提升，也不能代表真实网络、物理键盘/输入法或长时间多会话表现。逐字符指标截至回显解析完成，滚动指标使用合成 wheel 经实际 xterm 处理与重绘。Ubuntu 22 的 Xvfb 结果不等同完整桌面人工验收；Windows、Wayland、各类显卡、生产 sshd / vim / tmux 和长期压力仍需实机验证。

测试探针仅保留在开发构建中；发布前已重新构建不带 `VITE_PERF_PROBE` 的前端，并使用不带 `perf-probe` 的 Rust 功能构建。正式包：`target/ubuntu22/release/bundle/deb/Qizhan SSH_0.1.4_amd64.deb`，2,907,378 字节，约 2.77 MiB。SHA256：`bb8bfb003f714d6934db0e2032ab5ae925c353e2cc9594b6bda434a5627dc880`。

- 以 Ubuntu 22.04 为基线的 release 构建通过；Ubuntu 22 容器内 `dpkg -i` 安装成功，普通用户在 Xvfb 中启动，正式程序通过系统 `WM_DELETE_WINDOW` 关闭，退出码 0，本次关闭至退出 66ms。
- Ubuntu 24 本机从最终 `.deb` 提取实际程序，以独立配置启动并通过相同关闭协议退出，退出码 0，本次 68ms。上述耗时仅为该次短时记录，不是跨机器性能承诺。
- 已检查实际包内 ELF 包含最终 JS、CSS 及四个 WOFF2 文件名，不含测试环境变量、测试 IPC 名称或探针模块。安装包中的两份字体许可证与源文件逐字节一致。
- Ubuntu 24 本机 `apt-get -s install` 显示可从已安装的 0.1.3 升级为 0.1.4，无新增依赖。本轮没有替用户安装或替换正在运行的本机客户端。apt 提示已有 NVIDIA 固件包可自动清理，与本应用无关，本轮未执行清理。
- 0.1.3 安装包的 SHA256 已核对保持不变，历史产物未覆盖。

### 0.1.4 补充：本机运行内存

在 Ubuntu 24.04.4 X11 上，直接只读采样用户正在运行的正式客户端，核对执行文件与 0.1.4 包内程序的 SHA256 一致；没有读取终端内容、连接地址或凭据。当前进程树包含主程序、WebKit 网络进程、WebKit 页面进程和 1 个 OpenSSH 子进程，共 4 个进程。连续约 31.6 秒采集 31 个样本，PSS 中位数 216.92 MiB，范围 216.92–216.92 MiB；RSS 合计中位数 493.66 MiB。

末次 PSS 分摊：主程序 71.21 MiB、WebKit 网络进程 15.54 MiB、WebKit 页面进程 127.28 MiB、OpenSSH 2.89 MiB。原始记录见 `docs/memory-0.1.4.json`。RSS 合计来自 `/proc/stat` 的近似计数，单进程明细来自 `smaps_rollup`，两者可能略有差异。

PSS 按比例分摊共享物理页，更适合说明应用进程组的 RAM 份额；RSS 会重复计算共享页，两项都不能完整代表 GPU 显存。这是当前单 SSH 进程使用状态的短时结果，不是空闲首页、多个连接、连续日志或长时间压力结论。当前版本不能宣传为几十 MB 内存占用。

## 0.1.3 关闭窗口修复

根因是 0.1.2 注册了 `onCloseRequested`，但只授权了 `allow-close`；Tauri 的 JavaScript 监听器最终会调用 `destroy()`，缺少 `core:window:allow-destroy` 导致窗口留在原地。对本机已安装的 0.1.2 启动独立配置实例，通过 X11 `WM_DELETE_WINDOW` 发送系统关闭请求，5 秒后仍存活，测试按预期失败；该测试只结束自己启动的实例。

修复仅向本地主窗口增加销毁权限，关闭改为一次明确的「确认 → 清理会话 → 销毁窗口」，不再递归调用 `close()`。关闭开始就阻止默认处理并防止重入；取消或出错会释放重入保护，失败会显示提示并允许重试。监听器在配置和 OpenSSH 环境检测前注册。原生窗口销毁及进程退出继续执行后端会话清理。

已通过的检查：

- `npm run test:frontend`：11 项通过，其中新增 7 项关闭回归，覆盖实际 Tauri API 与真实权限配置、确认/取消、清理顺序、重复关闭、销毁失败重试、单会话清理失败以及已退出会话。移除销毁权限可使权限边界测试失败。
- `npm run build`、`npm run format:check`、`cargo fmt --all -- --check` 通过。
- `cargo test -p ssh-core --locked`：12 项通过，原有协议集成测试仍默认忽略；本轮真实 SSH 使用下述关闭夹具验证，没有重跑密码/MFA/钥匙串验证。
- `cargo clippy --workspace --all-targets --locked -- -D warnings` 及启用 `qizhan-ssh/perf-probe` 的同项检查通过，无警告。
- 原生关闭夹具通过实际前端监听器、GTK/WebKit/Tauri 和系统 OpenSSH，使用临时公私钥及仅回环监听的 Paramiko；场景为无连接、两条连接、取消后再次退出、连续 5 次关闭请求、连接编辑弹窗打开、信息弹窗打开。检查原生 `Destroyed` 事件、正常进程退出、服务端连接断开，以及没有残留的测试 SSH 进程。

Ubuntu 24 本机 X11 与 Ubuntu 22 容器 Xvfb 各通过上述 6 场景，共 12 场景。每个平台各建立 10 条真实 SSH 连接，全部正常回收。原始记录见 `docs/window-close-0.1.3.json`；其中 elapsedMs 包括启动、连接与测试交互，不代表纯关闭耗时。

关闭夹具复测（开发专用构建，不可发布）：

```bash
VITE_PERF_PROBE=1 npm run build
cargo build -p qizhan-ssh --release --features tauri/custom-protocol,perf-probe --locked
python3 scripts/native_close.py target/release/qizhan-ssh --output /tmp/qizhan-close.json
```

发布前必须用不含测试功能的前端和 Rust 重新构建，再运行 `python3 scripts/release_close.py /path/to/qizhan-ssh`。该脚本只向指定测试子进程的窗口发送系统关闭协议，不调用强制退出 API；5 秒内不退出即失败。X11/Xvfb 验证不能替代 Wayland、Windows 和长期连接压力验证。

0.1.3 正式包：`target/ubuntu22/release/bundle/deb/Qizhan SSH_0.1.3_amd64.deb`，2799348 字节，约 2.67 MiB。SHA256：`892a023655b0bbb3989ba47badbe137a0a61ae306195215ddedb04dcad391a94`。

- Ubuntu 22.04 容器内 `dpkg -i` 成功，以普通用户在 Xvfb 中启动包内程序，通过系统窗口关闭协议退出，退出码 0，本次关闭至退出约 72 ms。
- Ubuntu 24.04 本机从正式 `.deb` 提取实际程序，独立配置启动后通过相同系统窗口关闭协议退出，退出码 0，本次约 68 ms。0.1.2 在同一脚本下 5 秒不退出，按预期失败。关闭耗时仅为短时记录，不是跨机器性能承诺。
- 包内二进制包含最终生产 JS 文件名，未包含测试场景环境变量、测试 IPC 命令或探针前端模块。打包器写入 DEB 类型标记，因此包内 ELF 与构建目录 ELF 的标记字节不同；已对实际包内程序复测。
- Ubuntu 24 的 `apt-get -s install` 显示可将已安装的 0.1.2 升级为 0.1.3，无新增依赖。未替用户执行本机升级；Wayland 与 Windows 关闭仍待实机验证。

## 0.1.2 密码保存与卡顿修复

- 新建/编辑连接增加密码与保存选项，支持保留、替换、清除；密码进入系统凭据库，JSON 只有保存状态。目标变化删除旧凭据，配置写入失败恢复凭据，钥匙串不可用则报错，不回退明文。
- OpenSSH askpass 使用每会话独立的随机端口与随机 capability 的本地通道，同一二进制在 GTK 初始化前运行 helper。自动密码仅匹配解析后的目标；ProxyJump / ProxyCommand 转人工输入，不自动给跳板机提供目标口令。
- 已复现卡顿原因：标签隐藏时销毁 WebGL，上屏时反复创建 WebGL 字体图集，阻塞 WebKit。当前改为 DOM 渲染并跳过重复可见性更新；输出从 JSON/base64 改为原始二进制 Channel，仍保留 128 KiB 背压窗口。

原生性能检查使用测试专用 `perf-probe` 构建，在真实 WebKit 窗口中创建两个终端、切换 30 次、回显 40 条输入，并将约 1 MB 文本通过真实 PTY 与生产 IPC 链路往返。生产包移除测试模块与命令，不存在测试 shell 入口。三次测量原始数据见 `docs/performance-0.1.2.json`。

| 指标 | 修改前/中间对照 | 0.1.2 三次实测 |
| --- | --- | --- |
| 标签切换 P95 | 原 WebGL 生命周期 219 ms | 约 17 ms |
| 切换及输入/输出期间帧 P95 | 原切换阶段 198 ms | 19–20 ms |
| 最长帧 | 原切换阶段 220 ms | 25–30 ms |
| 本地 PTY 输入回显 P95 | DOM + base64 对照 30 ms | 11–12 ms |
| 约 1 MB 本地 PTY/IPC 往返 | DOM + base64 对照 3064 ms | 1035–1118 ms |

这些是同机短时测量，不能代表所有显卡、Wayland、远程网络或多小时多会话压力。旧版原始切换测试未测 PTY 回显；输入/输出比较使用相同 DOM 渲染下的 base64 与二进制传输，不能混为同一个版本基线。保留 WebGL 的中间实验也将切换 P95 降到 18 ms，但第一次激活仍有长帧，所以最终默认采用 DOM。

本次实际验证：

- 前端严格类型检查/构建、格式检查、4 项快速连接测试通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 包含 Tauri 桌面目标，无警告通过。
- 12 项核心测试通过；覆盖凭据目标绑定、明文排除、更新/删除、写入失败回滚、钥匙串失败无虚假保存状态、随机 capability 拒绝和跳板隔离。
- 本地 Paramiko + 真实 OpenSSH 的 7 场景通过：保存密码、错误后手动重试、取消主机信任、终端密码、私钥、普通连接/保存密码连接均拒绝变化的主机密钥。随后服务端正常关闭，退出码及中文往返通过。
- Ubuntu 24 上独立 DBus / 临时 GNOME Keyring 的真实保存、读取、替换、清除测试通过，不读取用户钥匙串。
- 浏览器查看了新版连接表单和密码保存选项。浏览器模式明确禁止保存密码；这不能替代原生钥匙串测试。
- 原生性能脚本三次通过预算检查（切换及本地回显 P95 < 60 ms；约 1 MB 往返 < 5 s）。

复测（开发测试专用，不能用此构建发布）：

```bash
VITE_PERF_PROBE=1 npm run build
cargo build -p qizhan-ssh --release --features tauri/custom-protocol,perf-probe --locked
python3 scripts/native_perf.py target/release/qizhan-ssh --output /tmp/qizhan-perf.json
```

发布前必须重新执行不带 `VITE_PERF_PROBE` 的 `npm run build`，并构建不含 `perf-probe` 的程序。Ubuntu 22/24 包验证结果见本节后续记录。Windows 凭据管理器与 askpass 实机、真实生产 sshd/MFA、Wayland/各类显卡及长期资源压力仍未验证。

0.1.2 安装包：`target/ubuntu22/release/bundle/deb/Qizhan SSH_0.1.2_amd64.deb`，约 2.67 MiB。SHA256：`cad8bea4fe22bcdb8465ddade5d8c9e624e6f19cfbb3dc16c7643ce45b2f4e8a`。

- Ubuntu 22.04 容器：生产 release 构建、`dpkg -i` 安装成功；普通用户 Xvfb 启动 8 秒存活后按测试计划由 timeout 结束（124），无 stderr。
- Ubuntu 24.04：同一生产二进制启动 6 秒存活后由 timeout 结束（124），无 stderr；`apt-get -s install` 显示可从已安装的 0.1.1 升级到 0.1.2，无新增依赖。
- 包中声明 OpenSSH / WebKitGTK / GTK 依赖，推荐 GNOME Keyring。校验二进制嵌入的是最终生产 JS 文件，前端没有 perf-probe / WebGL 测试模块。
- 未替用户执行本机安装升级；上述启动检查不等于完成原生窗口中的全部人工验收。

### 0.1.2 补充：连接后输入与滚动

用户确认卡顿反馈来自 0.1.1，尚未试用 0.1.2。本轮未修改生产输入/渲染逻辑，也未更换安装包；增加了测试专用的真实 SSH 回环检查。测试模式在独立配置中启动两个系统 OpenSSH 进程，向仅回环监听的 Paramiko 固定回显服务器认证；使用临时公私钥和 known_hosts，不执行任意远端 shell。加载 4000 行带 ANSI 颜色的中文日志，填满回滚区，然后持续发送日志并同时测字符回显与滚轮渲染。

| 指标（P95） | 两次测量 |
| --- | --- |
| 常规字符回显解析 | 13 / 14 ms |
| 持续日志下字符回显解析 | 94 / 91 ms |
| 空闲时滚轮重绘 | 34 / 35 ms |
| 持续日志下滚轮重绘 | 32 / 30 ms |
| 全程帧间隔 | 29 / 30 ms |
| 全程最长帧 | 43 / 43 ms |

每次都有两条真实 SSH 连接通过密钥认证，服务端核对全部 48 个测试字符回显，压力阶段实际发送约 1.33 MB 中文日志。数据追加在 `docs/performance-0.1.2.json`。两次均通过预设预算：常规字符回显 P95 < 80 ms、持续日志下 < 100 ms、空闲/压力滚轮渲染 P95 < 60/80 ms。这里的预算是本机回归检查阈值，不是跨机器性能承诺。

测量边界：字符以无需 Enter 的逐字符输入触发生产 onData/IPC/OpenSSH 链路；服务端在回显字节之后附加测试专用 OSC 解析确认，避免日志快速滚出最近行时产生漏检。字符指标到达解析完成，未包含物理键盘、输入法及最终屏幕扫描延迟。滚动以合成 wheel 事件经过 xterm 的实际滚轮处理及 onRender/下一帧，采样避开回滚边界。此测试不能替代用户鼠标/触控板、真实网络、远端 shell/vim/tmux、各类显卡及 Wayland 实机验收。持续日志下字符回显仍接近 0.1 秒，不能据此宣称所有负载完全无延迟。

复测需先按上文构建 `perf-probe`，然后运行：

```bash
python3 scripts/ssh_interaction_perf.py target/release/qizhan-ssh --output /tmp/qizhan-ssh-interaction.json
```

测试后已恢复生产前端构建；原 0.1.2 安装包 SHA256 保持不变。

## 0.1.1 客户端界面调整

界面统一为深色桌面客户端：工具栏、会话标签、快速连接栏、可折叠主机树和全尺寸终端。移除欢迎标语、主机管理表格、示意图及功能介绍区。增加快速连接、保存为主机、断开并保留终端输出、复制选区、Ctrl K 快连和 Ctrl B 主机栏切换。

本次验证：

- `npm run build`、`npm run format:check`、`cargo fmt --all -- --check` 通过。
- `npm run test:frontend` 的 4 项测试通过，覆盖别名配置默认值、用户名/IPv6/端口解析、非法参数及无效端口。
- `cargo test -p ssh-core --locked`：7 项核心测试通过；协议夹具测试仍按设计默认忽略，本次未重新执行，下方记录为 0.1.0 时的验证。
- 浏览器实际验证快速连接空值提示、IPv6 预填、保存与收藏、分组折叠、主机搜索、终端输入 help、搜索未命中及高亮、双标签切换和关闭、Ctrl B / Ctrl K / Alt 1 快捷键、主机删除确认。
- 1280×820 默认窗口及 800×580 最小窗口中的终端、主机栏和可滚动编辑器可用。窗口大小变化后终端行列跟随调整；浏览器无 error / warn 日志。
- 浏览器演示使用真实 xterm 渲染，仅运行本地 help / echo / clear / exit，不连接服务器。测试用主机已清理，预览入口只出现在浏览器中。原生完整 SSH 交互与 Windows 的待验收范围仍见下方。
- Ubuntu 22 release 构建通过，普通用户在 Xvfb 中启动后由 8 秒 timeout 结束（124），无 stderr；同一程序在 Ubuntu 24 启动 5 秒存活，无 stderr。

当前安装包：`target/ubuntu22/release/bundle/deb/Qizhan SSH_0.1.1_amd64.deb`。SHA256：`b128d2766bda793c63718b4b4a571b4cea12540c997a8c2b4fa519eb9e2bf088`。

截图：`docs/workspace-preview.jpg` 为新建会话页，`docs/client-terminal-preview.jpg` 为本地终端演示。下面的性能数字和详细测试环境为 0.1.0 基线，不能当作 0.1.1 的新测量。

## 0.1.0 历史基线

## 环境

- 主机：Ubuntu 24.04.4，Rust 1.97.1，Node.js 24.20.0，OpenSSH 9.6p1，WebKitGTK 2.52.6。
- Ubuntu 24 的 WebKit 开发头文件从 Ubuntu 官方软件包解压至项目外的 `/tmp/qizhan-native-deps`，构建通过临时 `PKG_CONFIG_PATH` 使用；没有修改本机系统安装状态。
- Ubuntu 22 构建采用独立 Jammy 容器与发行版内的开发依赖，复用相同 Rust 工具链与锁定依赖；前端使用 Vite 的正式构建产物。

## 已通过

| 检查 | 结果与范围 |
| --- | --- |
| `npm run build` | TypeScript 严格检查、Vite 生产构建通过 |
| `npm run format:check` | 前端源码格式检查通过 |
| `cargo fmt --all -- --check` | Rust 格式检查通过 |
| `cargo test --workspace --locked` | 7 项核心测试通过；原生应用测试目标成功编译，0 项 GUI 自动测试 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 包含 Tauri 桌面目标，无警告通过 |
| `python3 scripts/ssh_smoke.py` | 独立运行默认忽略的集成测试；3 种真实 OpenSSH 场景全部通过 |
| 浏览器交互 | 新增连接、分组与收藏、搜索无结果、收藏筛选、非法主机参数拦截、删除确认、预览模式连接提示 |
| 最小窗口 | 800×580 的连接列表及可滚动表单可用；临时视口已还原 |
| 浏览器日志 | 完成测试时无 error / warn 日志 |
| 原生 Ubuntu 24 构建 | Tauri release 及 `.deb` 生成成功；程序实际启动，5 秒存活检查通过，无 stderr |
| Ubuntu 22 构建 | Jammy 环境 release 编译通过，启用 `tauri/custom-protocol` 内嵌生产资源，动态依赖可解析 |
| Ubuntu 22 安装 / 启动 | 隔离容器内 `dpkg -i` 成功；普通用户在 Xvfb 中运行 8 秒后由测试 timeout 结束（124），无 stderr；这属于启动冒烟测试 |
| Ubuntu 22 产物在 Ubuntu 24 运行 | 同一二进制在 Ubuntu 24 动态依赖可解析、实际启动 5 秒存活，无 stderr |
| Ubuntu 24 安装依赖检查 | `apt-get -s install` 模拟安装通过，仅需新增 qizhan-ssh 包；未修改本机安装状态 |

核心测试覆盖：主机参数注入、IPv6 / SSH 别名合法性、重复连接 ID、零端口拒绝、配置损坏不覆盖、原子替换、Unix 0600 权限、PTY 输入/输出与尺寸调整、进程退出清理、未确认输出窗口及关闭时唤醒。

推荐试用的包为 `target/ubuntu22/release/bundle/deb/Qizhan SSH_0.1.0_amd64.deb`。主程序最高引用的 glibc 符号版本为 `GLIBC_2.34`；不能据此推导 Ubuntu 22 以外旧发行版也受支持，因为 WebKitGTK 等运行库还存在独立要求。

该包 SHA256：`6c4af15621c527be399d1553693f5998b59e4904fecf77d773b3136c47efdc82`。

集成测试使用本地 Paramiko 服务器与真实系统 `ssh`：先比对测试主机的 SHA256 指纹再确认信任，分别执行密码及私钥认证、UTF-8 往返、退出清理，并替换临时 known_hosts 后验证密钥变化会被拒绝。所有密钥和信任文件均为临时文件，没有修改用户的 SSH 配置。测试服务端不是生产 OpenSSH `sshd`。

## 首次性能基线

本机 Ubuntu 24 release，空连接工作台，启动后等待约 3 秒，再采样约 10 秒；统计应用及其 2 个 WebKit 子进程：

| 指标 | 实测 |
| --- | --- |
| `.deb` 包体 | 约 2.01 MiB，不包含系统运行库 |
| RSS 合计 | 444.36 MiB，共享页会被重复计算 |
| PSS 合计 | 184.66 MiB，共享页按比例分摊 |
| 空闲 CPU | 0.20%，按单核 100% 计算 |
| 首页 JavaScript | 约 42.5 KB，gzip 约 14.4 KB |
| 终端模块 | 约 369.1 KB，gzip 约 94.9 KB，首次连接时加载 |
| WebGL 模块 | 约 113.4 KB，gzip 约 31.0 KB，活动终端按需加载 |

这些是一次本机短时测量，不是资源占用保证、跨平台平均值或多会话性能结论。PSS 比 RSS 更适合这里的共享库场景，但也不能完整代表 GPU 显存。包体小与进程内存小是两个指标。本版不能宣传“几十 MB 内存”。

复测现有桌面进程（不读取终端内容）：

```bash
python3 scripts/measure_linux.py <qizhan-ssh的PID> --seconds 30
```

脚本汇总当前 PID 树；已退出子进程的 CPU 不计入，沙箱进程不可读时 PSS 会显示 null。应在相同硬件、桌面会话、分辨率、字体和 SSH 负载下比较。

## 尚未验证与下一轮验收

- Windows 只完成跨平台接口、ConPTY 路径及 NSIS/CI 配置，尚未实机运行或生成经验证的 Windows 安装包。
- GitHub Actions 工作流已编写，尚未在远端触发执行。
- 原生窗口中的完整人工 SSH 操作、中文输入法、剪贴板、多行粘贴、终端搜索、WebGL 与 DOM 回退尚未逐项实机验收；浏览器验证不等价于 WebKit 原生终端端到端测试。
- 真实服务器、ProxyJump、ssh-agent、加密私钥、MFA、网络中断、挂起恢复，仍需部署环境验证。
- Ubuntu 22/24 的 Wayland/X11、不同显卡和 DPI、ARM64，以及干净系统安装需要进一步覆盖。
- 尚无持续高吞吐、多小时、多会话的内存及键盘延迟基准。建议测试 1/4/8 会话、连续日志、vim/tmux、断网后退出、重复开关标签，观察内存是否稳定和进程是否回收。
- 首版无安装包签名和自动更新。

界面源码已扫描装饰性顶部色条、渐变和伪元素；现有上缘边线是中性分隔线，不是卡片装饰条。当前工作目录不是 Git 仓库，未进行提交或推送。
