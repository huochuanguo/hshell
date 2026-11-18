// 远程路径遵循 Unix 规则，本机路径仅在文件选择器返回后解析文件名。
export function remoteJoin(parent: string, name: string): string {
  if (
    !parent.startsWith("/") ||
    !name ||
    [".", ".."].includes(name) ||
    /[/\0]/.test(name)
  )
    throw new Error("远程路径或文件名无效");
  return `${parent.replace(/\/+$/, "")}/${name}`;
}

export function remoteParent(path: string): string {
  if (!path.startsWith("/")) throw new Error("远程路径必须为 Unix 绝对路径");
  return (
    path
      .replace(/\/+$/, "")
      .slice(0, path.replace(/\/+$/, "").lastIndexOf("/")) || "/"
  );
}

export function localBasename(path: string): string {
  return path.split(/[/\\]/).at(-1) ?? path;
}

export function downloadName(name: string): string {
  const value = name
    .replace(/[<>:"/\\|?*\x00-\x1f]/g, "_")
    .replace(/[. ]+$/, "");
  return !value || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(value)
    ? `_${value || "download"}`
    : value;
}
