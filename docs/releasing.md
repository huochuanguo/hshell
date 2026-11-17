# 自动发布 Ubuntu 安装包

Gitee `origin` 保留为源码主仓库；GitHub `github` 为私有镜像和构建发布平台。Gitee Go 尚未开通，本流程不依赖它。GitHub Actions 使用账号现有可用额度，未设置付费升级。Windows 仍有独立构建检查，尚未纳入正式安装包发布。

## 发布流程

推送 GitHub `v*` 标签触发 `Release Ubuntu native`：

1. 校验标签严格符合 `vMAJOR.MINOR.PATCH`，且与 `crates/native-linux/Cargo.toml` 完全相同；对应版本说明必须存在。
2. Ubuntu 22.04 执行 Rust 格式、原生/共享核心测试和 Clippy；真实 OpenSSH SFTP 验证包括故障恢复、双向续传、双路传输及终端响应。
3. 重新构建无探针的正式程序，打包脚本检查不含测试入口、不依赖 WebKit，自动从原生版本生成安装包名和 SHA256 文件。
4. Ubuntu 22.04 / 24.04 下载同一产物，检查 SHA256、实际安装并验证正式窗口关闭和进程退出。
5. 全部通过后创建草稿发行版，上传安装包与校验文件，再发布为正式发行版；失败时不发布不完整附件。已经正式发布的版本不允许覆盖。

只有最后的发布任务使用仓库 `GITHUB_TOKEN` 的 `contents: write` 权限，其余任务只读。无需个人访问令牌、用户密码或新的仓库 Secrets。安装包在 Release 中长期保存，CI 中间产物只保留 7 天。

## 发布新版本

先修改原生版本并更新 `Cargo.lock`，新增 `docs/releases/版本号.md`，完成检查后提交。以下命令使用 `0.3.3` 为示例；下次发布必须使用新的版本号，不移动已发布标签：

```bash
# 新检出仓库时只需要添加一次镜像 remote。
git remote add github https://github.com/huochuanguo/hshell.git

git push origin main
git push github main
git tag -a v0.3.3 -m '发布 HShell 0.3.3'
git push origin v0.3.3
git push github v0.3.3
```

工作区当前已经配置 `github` 时，跳过 `remote add`。`main` 的普通 CI 不处理标签，发布流程不会重复运行 Windows 和 Web 打包任务。镜像同步通过上面的显式 Git 推送完成；仅向 Gitee 推送不会触发 GitHub Actions。

构建状态：[Actions](https://github.com/huochuanguo/hshell/actions)。安装包：[Releases](https://github.com/huochuanguo/hshell/releases)。均需要使用有权限的 GitHub 账号。

## 失败与重试

- 标签、原生版本或说明不匹配：修正版本资料后使用新标签。
- 构建或兼容性失败：在 Actions 中检查失败任务；基础设施瞬时失败可重跑同一次工作流，代码修正需要新版本标签。
- 上传失败：保留草稿，重跑成功后会补齐并校验附件；正式发布前才会更新最新版本标记。
- 已正式发布：工作流拒绝替换附件。通过新版本修复，不强推标签。

首轮 `v0.3.1` 的协议验证通过，但云端 512 MiB 传输未在界面探针原有的 10 秒等待内完成，因此没有上传正式发行版。该标签保留不动；`v0.3.2` 仅将大文件完成等待设为 60 秒，并同步调整外层验证器时限，输入 100 ms、滚动/目录调度 50 ms 的 P95 门槛保持不变。普通界面操作仍使用原有 10 秒等待上限。

第二轮 `v0.3.2` 的大文件传输已通过，但固定等待 1 秒后退出的探针可能在 SSH 连接完成、首次检查点落盘前停止传输，导致恢复检查没有有效进度。`v0.3.3` 在暂停和退出前直接确认真实检查点已保存，并注入 2 秒 SSH 启动延迟回归，仍要求恢复为暂停任务、存在非零断点并完成 SHA256 一致性检查。旧标签继续保留。

首次配置需要在浏览器登录 GitHub；浏览器登录与本机 Git HTTPS 认证相互独立。本机可使用 GitHub CLI 的设备登录或已配置的 SSH 密钥，勿把令牌写入仓库或命令行参数。
