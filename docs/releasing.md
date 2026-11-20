# 发布 Ubuntu 与 Windows 安装包

Gitee `origin` 保存源码，GitHub 公开镜像运行构建与发布。Ubuntu 和 Windows 共用同一个版本号、标签和 GitHub Release。

## 发布流程

1. 同步 `crates/native-linux/Cargo.toml`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、npm 版本与两个锁文件，新增 `docs/releases/版本号.md`，完成必要检查并提交。
2. 在 Windows 打包，校验安装包内的版本和 SHA256。同步代码到两个远端后，为即将推送的版本标签创建 GitHub Release 草稿，上传 `.exe` 和 `.exe.sha256`。
3. 推送 GitHub `vMAJOR.MINOR.PATCH` 标签，触发 `Release HShell`。工作流核对标签与两个平台版本并检查发布说明，然后在 Ubuntu 22.04 构建，验证 SFTP 故障恢复、双向续传、双路传输和终端响应。
4. Ubuntu 22.04、24.04 下载同一 `.deb`，校验、安装并验证正式窗口关闭和进程退出。
5. 发布任务校验草稿里的 Windows 安装包以及 Ubuntu `.deb`，补齐 Ubuntu 两个附件，将草稿发布为正式 Release。任何检查失败都保留草稿，已正式发布的版本不能覆盖。

## 示例

以下以 `0.3.7` 为例。先推送包含版本和说明的提交，再创建草稿并上传 Windows 安装包。示例假设已配置 `github` 镜像 remote，且 GitHub CLI 已登录。

```bash
git push origin main
git push github main
gh release create v0.3.7 --repo huochuanguo/hshell --draft --target "$(git rev-parse HEAD)" \
  --title 'HShell 0.3.7' --notes-file docs/releases/0.3.7.md \
  target/release/bundle/nsis/HShell_0.3.7_x64-setup.exe \
  target/release/bundle/nsis/HShell_0.3.7_x64-setup.exe.sha256
git tag -a v0.3.7 -m '发布 HShell 0.3.7'
git push origin v0.3.7
git push github v0.3.7
```

构建状态见 [Actions](https://github.com/huochuanguo/hshell/actions)，正式安装包见 [Releases](https://github.com/huochuanguo/hshell/releases)。仅推送 Gitee 不会触发 GitHub Actions。安装包不提交到 Git，构建中间产物保留 7 天。

## 失败与重试

- 构建、兼容性或上传暂时失败时，保留草稿与已上传的 Windows 附件；修复临时故障后重跑同一次工作流。
- 如需修正发布代码，使用新的版本号和标签；不要移动已推送的标签或覆盖已发布的包。

## 早期发布记录

首轮 `v0.3.1` 的协议验证通过，但云端 512 MiB 传输未在界面探针原有的 10 秒等待内完成，因此没有上传正式发行版。该标签保留不动；`v0.3.2` 仅将大文件完成等待设为 60 秒，并同步调整外层验证器时限，输入 100 ms、滚动/目录调度 50 ms 的 P95 门槛保持不变。普通界面操作仍使用原有 10 秒等待上限。

第二轮 `v0.3.2` 的大文件传输已通过，但固定等待 1 秒后退出的探针可能在 SSH 连接完成、首次检查点落盘前停止传输，导致恢复检查没有有效进度。`v0.3.3` 在暂停和退出前直接确认真实检查点已保存，并注入 2 秒 SSH 启动延迟回归，仍要求恢复为暂停任务、存在非零断点并完成 SHA256 一致性检查。旧标签继续保留。

第三轮 `v0.3.3` 在对所有 SSH 启动注入延迟后，暴露了断网测试相同的计时假设：连接建立耗时计入 1 秒门槛，首次断点尚未落盘就触发断网。`v0.3.4` 将断网触发也改为核对实际持久化断点，并在超时后再次检查非零断点，再验证续传校验字节与最终 SHA256。

首次配置需要在浏览器登录 GitHub；浏览器登录与本机 Git HTTPS 认证相互独立。本机可使用 GitHub CLI 的设备登录或已配置的 SSH 密钥，勿把令牌写入仓库或命令行参数。
