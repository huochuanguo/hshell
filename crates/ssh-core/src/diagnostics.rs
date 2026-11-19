/// 根据最近的 SSH 终端输出给出排查提示；原始输出仍保留在终端。
/// 输出也可能来自远端，因此只返回固定文案，不能据此更改连接或信任设置。
pub fn ssh_failure_hint(output: &str) -> Option<&'static str> {
    let lower = output.to_ascii_lowercase();
    if lower.contains("remote host identification has changed")
        || lower.contains("host key verification failed")
        || lower.contains("host key for ") && lower.contains("has changed")
    {
        return Some("主机身份未通过验证。请通过可信渠道核对服务器指纹；确认无误后再处理 OpenSSH 的信任记录。");
    }
    lower.lines().rev().find_map(|line| {
        if line.contains("could not resolve hostname")
            || line.contains("temporary failure in name resolution")
        {
            Some("无法解析主机名。请检查地址、SSH 别名，以及当前网络的 DNS 或 VPN。")
        } else if line.contains("connection refused") {
            Some("SSH 连接被拒绝。请确认服务器上的 SSH 服务已启动，并核对地址和端口。")
        } else if line.contains("connection timed out")
            || line.contains("operation timed out")
            || line.contains("connection timeout")
        {
            Some("连接 SSH 超时。请检查网络、VPN、目标端口及防火墙。")
        } else if line.contains("no route to host") || line.contains("network is unreachable") {
            Some("当前网络无法到达服务器。请检查网络连接、VPN 或跳板机配置。")
        } else if line.contains("permission denied (")
            || line.contains("too many authentication failures")
            || line.contains("no supported authentication methods")
        {
            Some("SSH 认证未通过。请检查用户名、密码或私钥，并确认服务器允许该认证方式。")
        } else if line.contains("no matching host key type found")
            || line.contains("no matching key exchange method found")
            || line.contains("no matching cipher found")
        {
            Some("SSH 客户端与服务器的加密算法不兼容。请检查服务器和客户端的 OpenSSH 配置。")
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_connection_failures_without_echoing_terminal_text() {
        for (output, expected) in [
            (
                "ssh: Could not resolve hostname dev: Name or service not known",
                "无法解析主机名",
            ),
            (
                "ssh: connect to host 127.0.0.1 port 22: Connection refused",
                "连接被拒绝",
            ),
            (
                "ssh: connect to host 192.0.2.1 port 22: Connection timed out",
                "连接 SSH 超时",
            ),
            (
                "ssh: connect to host test port 22: Network is unreachable",
                "无法到达",
            ),
            ("git@test: Permission denied (publickey).", "认证未通过"),
            (
                "WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!\r\nHost key verification failed.",
                "主机身份未通过",
            ),
            (
                "Unable to negotiate with 127.0.0.1: no matching host key type found.",
                "算法不兼容",
            ),
        ] {
            assert!(
                ssh_failure_hint(output).is_some_and(|hint| hint.contains(expected)),
                "{output}"
            );
        }
        assert_eq!(ssh_failure_hint("cat: note.txt: Permission denied"), None);
        assert_eq!(ssh_failure_hint("unrecognized failure"), None);
    }
}
