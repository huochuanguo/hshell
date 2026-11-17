import type { Profile } from "./types";

export function parseQuickConnection(
  destination: string,
  portInput: string,
): Profile {
  const parts = destination.trim().split("@");
  if (parts.length > 2)
    throw new Error("请使用 用户名@主机地址，或直接填写主机地址。");
  const username = parts.length === 2 ? parts[0] : "";
  const host = parts.at(-1)!.replace(/^\[([^\]]+)\]$/, "$1");
  if (
    !host ||
    host.length > 253 ||
    !/^[a-zA-Z0-9_:][a-zA-Z0-9._:-]*$/.test(host)
  ) {
    throw new Error("请输入有效的主机地址、IP 或 SSH 别名。");
  }
  if (
    username.length > 128 ||
    /^[\s-]|\s|[\x00-\x1f\x7f]/.test(username) ||
    (parts.length === 2 && !username)
  ) {
    throw new Error("请输入有效的用户名，用户名不能包含空格或以 - 开头。");
  }
  if (host.includes(":") && !host.includes("::") && /^[^:]+:\d+$/.test(host)) {
    throw new Error("请将端口填入右侧端口栏；IPv6 地址可直接填写。");
  }
  const port = portInput.trim() ? Number(portInput) : null;
  if (port !== null && (!Number.isInteger(port) || port < 1 || port > 65535)) {
    throw new Error("端口须为 1–65535，留空沿用 SSH 配置。");
  }
  return {
    id: crypto.randomUUID(),
    name: host,
    host,
    username,
    port,
    identityFile: "",
    group: "",
    favorite: false,
  };
}
