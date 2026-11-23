import { invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { version as appVersion } from "../package.json";
import {
  createElement,
  Plus,
  Minus,
  Search,
  Server,
  Star,
  ArrowRight,
  TerminalSquare,
  X,
  ChevronDown,
  Folder,
  KeyRound,
  MoreHorizontal,
  RotateCw,
  ChevronUp,
  Check,
  Command,
  PanelLeft,
  Plug,
  Bookmark,
  CircleHelp,
  Square,
  Copy,
} from "lucide";
import type { Environment, Profile } from "./types";
import type { TerminalView } from "./terminal";
import { parseQuickConnection } from "./connection";
import { createCloseHandler } from "./window-close";
import { FilesPane } from "./files";
import { bindFilesResize } from "./files-resize";
import { bindSnippetLibrary } from "./snippets";
import { copyText } from "./clipboard";
import {
  DEFAULT_TERMINAL_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
  MAX_TERMINAL_FONT_SIZE,
  getTerminalFontSize,
  onTerminalFontSizeChange,
  setTerminalFontSize,
} from "./terminal-font";
import "./style.css";
import "./files.css";
import brandIcon from "./assets/hshell.svg?url";

const iconSet = {
  Plus,
  Minus,
  Search,
  Server,
  Star,
  ArrowRight,
  TerminalSquare,
  X,
  ChevronDown,
  Folder,
  KeyRound,
  MoreHorizontal,
  RotateCw,
  ChevronUp,
  Check,
  Command,
  PanelLeft,
  Plug,
  Bookmark,
  CircleHelp,
  Square,
  Copy,
};
const icon = (name: keyof typeof iconSet, size = 18) => {
  const [tag, attributes, children] = iconSet[name];
  return createElement([
    tag,
    {
      ...attributes,
      width: size,
      height: size,
      "stroke-width": 1.65,
      "aria-hidden": "true",
    },
    children,
  ]).outerHTML;
};
const escape = (value: string) =>
  value.replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ]!,
  );
const $ = <T extends HTMLElement = HTMLElement>(selector: string) =>
  document.querySelector<T>(selector)!;
const desktop = isTauri();
let profiles: Profile[] = [];
let filter = "all";
let query = "";
let active = "home";
let selectedHost: string | null = null;
let saving = false;
let configHealthy = true;
let editing: string | null = null;
let environment: Environment | undefined;
let toastTimer = 0;
const sessions = new Map<string, TerminalView>();
let files: FilesPane;

$("#app").innerHTML = `
  <header class="app-toolbar">
    <button class="brand" id="brand" aria-label="新建会话"><img src="${brandIcon}" alt="" width="24" height="24"><strong>HShell</strong></button>
    <div class="toolbar-actions"><button id="new-connection" class="tool-button">${icon("Plus", 15)} 新建连接</button><button id="toggle-sidebar" class="tool-button" aria-label="切换主机栏" aria-expanded="true">${icon("PanelLeft", 15)}<span>主机栏</span></button></div>
    <div class="toolbar-right"><button id="open-snippets" class="tool-button" ${desktop ? "" : "hidden"}>${icon("Command", 15)} 命令片段</button><button id="toggle-files" class="tool-button" aria-expanded="false" aria-controls="files-pane">${icon("Folder", 15)} 文件</button><span id="platform-name">SSH 客户端</span><button id="settings" class="icon-button" aria-label="客户端信息与快捷键">${icon("CircleHelp", 17)}</button></div>
  </header>
  <aside class="sidebar" aria-label="主机与会话">
    <div class="sidebar-heading"><h2>主机</h2><span id="all-count" class="count">0</span><button class="icon-button" id="open-connection-backup" aria-label="导入或导出连接" title="导入或导出连接" ${desktop ? "" : "hidden"}>${icon("MoreHorizontal", 16)}</button><button class="icon-button" id="sidebar-add" aria-label="添加主机">${icon("Plus", 16)}</button></div>
    <label class="sidebar-search">${icon("Search", 14)}<input id="host-search" placeholder="搜索主机…" aria-label="搜索连接"><kbd>/</kbd></label>
    <nav class="filters" aria-label="连接筛选"><button data-filter="all" class="selected">全部</button><button data-filter="favorites">${icon("Star", 12)} 收藏 <span id="favorite-count">0</span></button></nav>
    <div id="host-list" class="host-list"></div>
    <section class="session-list-section"><div class="section-label"><span>打开的会话</span><span id="sidebar-session-count">0</span></div><div id="open-sessions" class="open-sessions"></div></section>
    <div class="sidebar-footer">${icon("KeyRound", 14)}<span id="engine-state">正在检查 OpenSSH…</span><button class="icon-button" id="show-shortcuts" aria-label="快捷键">${icon("Command", 14)}</button></div>
  </aside>
  <main class="workspace">
    <header class="tab-bar"><div id="tabs" class="tabs" role="tablist" aria-label="终端会话"></div><button class="home-tab active" id="home-tab" title="新建会话">${icon("Plus", 15)}<span>新建会话</span></button><span class="tab-bar-spacer"></span><button id="tab-search" class="icon-button" aria-label="搜索当前终端" disabled>${icon("Search", 15)}</button></header>
    <form id="quick-connect" class="quick-connect" aria-label="快速连接">
      <span class="protocol-label">${icon("Plug", 14)} SSH</span><input id="quick-host" aria-label="快速连接地址" placeholder="用户名@主机地址，或 SSH 别名" autocomplete="off" spellcheck="false"><span class="port-label">端口</span><input id="quick-port" aria-label="快速连接端口" type="number" min="1" max="65535" placeholder="配置 / 22"><button class="quick-submit" type="submit">连接 ${icon("ArrowRight", 14)}</button><button id="quick-save" type="button" class="icon-button" aria-label="保存为主机" title="保存为主机">${icon("Bookmark", 15)}</button>
    </form>
    <p id="quick-error" class="quick-error" role="alert" hidden></p>
    <div class="workspace-body"><div class="session-area"><section class="session-start" id="home" aria-label="新建 SSH 会话">
      <div class="start-content"><span class="start-terminal-symbol">${icon("TerminalSquare", 43)}</span><h1>新建 SSH 会话</h1><p>输入上方地址，或双击左侧主机连接</p><div class="start-actions"><button class="primary" id="start-connect">${icon("Plug", 15)} 快速连接 <kbd>Ctrl Shift K</kbd></button><button class="secondary" id="start-save">${icon("Plus", 15)} 添加主机</button></div><div class="saved-shortcuts" id="saved-shortcuts" hidden></div><button id="preview-terminal" class="preview-terminal" ${desktop ? "hidden" : ""}>${icon("TerminalSquare", 14)} 体验终端界面 <span>仅本地演示</span></button></div>
      <div class="start-footnote"><span>SSH 配置与密钥由本机 OpenSSH 管理</span><span>新建连接 <kbd>Ctrl Shift T</kbd></span></div>
    </section>
    <section id="terminal-workspace" class="terminal-workspace" hidden>
      <div class="terminal-toolbar"><div>${icon("TerminalSquare", 13)}<span id="terminal-address"></span><span class="tiny-dot" id="session-dot"></span><span id="session-status"></span></div><div><div class="terminal-font-controls" role="group" aria-label="终端字号"><button id="terminal-font-decrease" class="icon-button" aria-label="缩小终端字号" title="缩小终端字号 Ctrl+−">${icon("Minus", 13)}</button><button id="terminal-font-reset" class="terminal-font-value" title="恢复默认字号 Ctrl+0"><span id="terminal-font-size" aria-live="polite"></span></button><button id="terminal-font-increase" class="icon-button" aria-label="放大终端字号" title="放大终端字号 Ctrl++">${icon("Plus", 13)}</button></div><button id="terminal-tab-key" class="dark-button" aria-label="向远程终端发送 Tab" title="发送 Tab，由远程 Shell 补全" disabled>Tab</button><button id="terminal-copy" class="icon-button" aria-label="复制选中内容" title="复制选中内容">${icon("Copy", 14)}</button><button id="terminal-search-button" class="icon-button" aria-label="搜索终端内容" title="搜索 Ctrl+Shift+F">${icon("Search", 14)}</button><button id="reconnect" class="dark-button" hidden>${icon("RotateCw", 14)} 重连</button><button id="disconnect" class="dark-button" title="断开 SSH 会话">${icon("Square", 11)} 断开</button></div></div>
      <div class="terminal-search" id="terminal-search" hidden><input id="terminal-query" aria-label="搜索终端内容" placeholder="查找终端内容"><span id="search-result"></span><button class="icon-button" id="search-prev" aria-label="上一个结果">${icon("ChevronUp", 16)}</button><button class="icon-button" id="search-next" aria-label="下一个结果">${icon("ChevronDown", 16)}</button><button class="icon-button" id="search-close" aria-label="关闭搜索">${icon("X", 16)}</button></div><div id="terminal-panes" class="terminal-panes"></div><div id="exit-banner" class="exit-banner" role="status" hidden></div>
    </section></div><div id="files-resizer" class="files-resizer" role="separator" aria-orientation="vertical" aria-label="文件栏宽度" aria-controls="files-pane" title="拖动或按左右方向键调整文件栏宽度" tabindex="0"></div><aside id="files-pane" class="files-pane" aria-label="SFTP 文件与传输" hidden></aside></div>
  </main>
  <footer class="status-bar"><div><span class="tiny-dot" id="app-state-dot"></span><span id="app-state">就绪</span><span class="status-divider"></span><span id="session-count">0 个运行会话</span></div><div><span id="terminal-size"></span><span>UTF-8</span><span>xterm-256color</span><button id="status-help" aria-label="查看快捷键">${icon("Command", 12)} 快捷键</button></div></footer>
  <dialog id="profile-dialog" aria-labelledby="editor-title"><form id="profile-form"><div class="dialog-heading"><div><span class="eyebrow">CONNECTION</span><h2 id="editor-title">新建连接</h2></div><button type="button" class="icon-button" data-close="profile-dialog" aria-label="关闭">${icon("X", 20)}</button></div><p class="dialog-description">保存常用服务器。密码可加密保存在本机系统凭据库。</p><label>连接名称<input name="name" required maxlength="80" placeholder="例如：开发服务器" autocomplete="off"></label><label>主机地址或 SSH 别名<input name="host" required maxlength="253" placeholder="192.168.1.10 或 my-server" spellcheck="false" autocomplete="off"></label><div class="form-row"><label>用户名 <span>可选</span><input name="username" maxlength="128" placeholder="沿用 SSH 配置" autocomplete="off" spellcheck="false"></label><label class="port-field">端口 <span>可选</span><input name="port" type="number" min="1" max="65535" placeholder="配置 / 22"></label></div><label>登录密码 <span>可选</span><input name="password" type="password" autocomplete="new-password" maxlength="4096" placeholder="输入要保存的密码"><small id="password-hint">勾选下方选项后保存到系统凭据库。</small></label><label class="checkbox-label remember-password"><input name="rememberPassword" type="checkbox">保存密码到本机系统凭据库</label><label>私钥路径 <span>可选</span><input name="identityFile" placeholder="~/.ssh/id_ed25519" maxlength="4096" spellcheck="false" autocomplete="off"><small>留空使用 ssh-agent、默认密钥或密码认证。</small></label><div class="form-row"><label>分组 <span>可选</span><input name="group" maxlength="40" placeholder="例如：工作" list="groups"></label><label class="checkbox-label"><input name="favorite" type="checkbox">${icon("Star", 16)} 收藏连接</label></div><datalist id="groups"></datalist><p id="form-error" class="form-error" role="alert"></p><div class="dialog-actions"><button type="button" class="danger-text" id="delete-profile" hidden>删除连接</button><span></span><button type="button" class="secondary" data-close="profile-dialog">取消</button><button class="primary" id="save-profile" type="submit">${icon("Check", 16)} 保存连接</button></div></form></dialog>
  <dialog id="confirm-dialog" aria-labelledby="confirm-title"><div class="dialog-heading"><h2 id="confirm-title"></h2></div><p id="confirm-message" class="dialog-description"></p><div class="dialog-actions"><span></span><button class="secondary" id="confirm-cancel">取消</button><button class="primary" id="confirm-ok">确定</button></div></dialog>
  <dialog id="connection-backup" aria-labelledby="backup-title"><div class="dialog-heading"><h2 id="backup-title">连接配置迁移</h2><button class="icon-button" id="backup-close" aria-label="关闭">${icon("X", 20)}</button></div><p class="dialog-description">导出为新的 JSON 文件，包含主机、用户名、端口、分组和收藏。密码、私钥及其路径留在本机。导入时追加新连接，跳过重复项。</p><p class="form-error" id="backup-error" role="alert"></p><div class="dialog-actions"><span></span><button type="button" class="secondary" id="backup-import">导入连接</button><button type="button" class="primary" id="backup-export">导出连接</button></div></dialog>
  <dialog id="info-dialog" aria-labelledby="info-title"><div class="dialog-heading"><div><span class="eyebrow">WORKSPACE</span><h2 id="info-title">HShell</h2></div><button class="icon-button" data-close="info-dialog" aria-label="关闭">${icon("X", 20)}</button></div><div class="shortcut-list"><div><span>快速连接</span><kbd>Ctrl Shift K</kbd></div><div><span>显示 / 隐藏主机栏</span><kbd>Ctrl Shift B</kbd></div><div><span>新建连接</span><kbd>Ctrl Shift T</kbd></div><div><span>关闭当前终端</span><kbd>Ctrl Shift W</kbd></div><div><span>查找终端内容</span><kbd>Ctrl Shift F</kbd></div><div><span>复制 / 粘贴</span><kbd>Ctrl Shift C / V</kbd></div><div><span>终端字号放大 / 缩小</span><kbd>Ctrl + / −</kbd></div><div><span>恢复默认终端字号</span><kbd>Ctrl 0</kbd></div><div><span>切换终端</span><kbd>Alt 1 … 9</kbd></div><div><span>搜索主机（新建会话页）</span><kbd>/</kbd></div></div><div class="environment-details" id="environment-details"></div><p class="info-note">首次连接时，请核对主机指纹。已保存密码的连接会自动登录；其他连接在终端内输入密码。多行粘贴会先提示确认。</p><span class="version-label">HShell · ${appVersion}</span></dialog>
  <div id="toast" class="toast" role="status" hidden></div>
`;

function notify(message: string) {
  $("#toast").textContent = message;
  $("#toast").hidden = false;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    $("#toast").hidden = true;
  }, 6500);
}

let pendingConfirm: ((value: boolean) => void) | undefined;
function confirmAction(
  title: string,
  message: string,
  action = "确定",
): Promise<boolean> {
  if (pendingConfirm) return Promise.resolve(false);
  $("#confirm-title").textContent = title;
  $("#confirm-message").textContent = message;
  $("#confirm-ok").textContent = action;
  $("#confirm-dialog").classList.toggle(
    "paste-confirm",
    title.includes("粘贴"),
  );
  $<HTMLDialogElement>("#confirm-dialog").showModal();
  $("#confirm-cancel").focus();
  return new Promise((resolve) => {
    pendingConfirm = resolve;
  });
}
function resolveConfirm(value: boolean) {
  $<HTMLDialogElement>("#confirm-dialog").close();
  pendingConfirm?.(value);
  pendingConfirm = undefined;
}
$("#confirm-ok").onclick = () => resolveConfirm(true);
$("#confirm-cancel").onclick = () => resolveConfirm(false);
$("#confirm-dialog").addEventListener("cancel", (event) => {
  event.preventDefault();
  resolveConfirm(false);
});

function filtered() {
  const needle = query.toLocaleLowerCase();
  return profiles.filter(
    (p) =>
      (filter !== "favorites" || p.favorite) &&
      `${p.name} ${p.host} ${p.username} ${p.group}`
        .toLocaleLowerCase()
        .includes(needle),
  );
}
const address = (p: Profile) =>
  `${p.username ? `${p.username}@` : ""}${p.port && p.host.includes(":") ? `[${p.host}]` : p.host}${p.port ? `:${p.port}` : ""}`;

const collapsedGroups = new Set<string>();
function renderProfiles() {
  const items = filtered();
  $("#all-count").textContent = String(profiles.length);
  $("#favorite-count").textContent = String(
    profiles.filter((p) => p.favorite).length,
  );
  document.querySelectorAll<HTMLElement>("[data-filter]").forEach((button) => {
    button.classList.toggle("selected", button.dataset.filter === filter);
    button.setAttribute(
      "aria-pressed",
      String(button.dataset.filter === filter),
    );
  });
  const groups = [...new Set(items.map((p) => p.group || "未分组"))];
  $("#host-list").innerHTML = items.length
    ? groups
        .map(
          (group) => `
    <details class="host-group" data-group="${escape(group)}" ${query || !collapsedGroups.has(group) ? "open" : ""}>
      <summary>${icon("ChevronDown", 12)}${icon("Folder", 13)}<span>${escape(group)}</span><small>${items.filter((p) => (p.group || "未分组") === group).length}</small></summary>
      ${items
        .filter((p) => (p.group || "未分组") === group)
        .map(
          (p) =>
            `<div class="host-item" data-profile="${escape(p.id)}"><button class="host-connect" data-select-host="${escape(p.id)}" title="双击或按 Enter 连接 ${escape(address(p))}">${icon("Server", 15)}<span><strong>${escape(p.name)}</strong><small>${escape(address(p))}</small></span></button><button class="host-favorite icon-button ${p.favorite ? "is-favorite" : ""}" data-favorite="${escape(p.id)}" aria-label="${p.favorite ? "取消收藏" : "收藏"} ${escape(p.name)}" aria-pressed="${p.favorite}">${icon("Star", 12)}</button><button class="host-edit icon-button" data-edit="${escape(p.id)}" aria-label="编辑 ${escape(p.name)}">${icon("MoreHorizontal", 15)}</button></div>`,
        )
        .join("")}
    </details>`,
        )
        .join("")
    : `<div class="sidebar-empty">${icon(profiles.length ? "Search" : "Server", 22)}<strong>${profiles.length ? "没有匹配的主机" : "还没有主机"}</strong><p>${profiles.length ? "试试其他关键词或筛选条件" : "添加常用服务器，<br>下次直接打开终端。"}</p>${profiles.length ? "" : '<button id="empty-add" class="text-button">添加主机</button>'}</div>`;
  document
    .querySelectorAll<HTMLDetailsElement>("[data-group]")
    .forEach((details) => {
      details.addEventListener("toggle", () => {
        const group = details.dataset.group!;
        if (details.open) collapsedGroups.delete(group);
        else collapsedGroups.add(group);
      });
    });
  $("#empty-add")?.addEventListener("click", () => openEditor());
  const shortcuts = [...profiles]
    .sort((a, b) => Number(b.favorite) - Number(a.favorite))
    .slice(0, 4);
  $("#saved-shortcuts").hidden = !shortcuts.length;
  $("#saved-shortcuts").innerHTML =
    `<span class="shortcuts-label">常用主机</span>${shortcuts.map((p) => `<button data-connect="${escape(p.id)}">${icon("Server", 14)}<span>${escape(p.name)}</span><small>${escape(p.host)}</small>${icon("ArrowRight", 13)}</button>`).join("")}`;
  $("#groups").innerHTML = [
    ...new Set(profiles.map((p) => p.group).filter(Boolean)),
  ]
    .map((g) => `<option value="${escape(g)}"></option>`)
    .join("");
  updateHostHighlights();
}
const backupDialog = $<HTMLDialogElement>("#connection-backup");
const backupImport = $<HTMLButtonElement>("#backup-import");
const backupExport = $<HTMLButtonElement>("#backup-export");
const backupClose = $<HTMLButtonElement>("#backup-close");
let transferringBackup = false;
$("#open-connection-backup").onclick = () => {
  $("#backup-error").textContent = "";
  backupDialog.showModal();
  backupImport.focus();
};
backupClose.onclick = () => backupDialog.close();
backupDialog.oncancel = (event) => {
  if (transferringBackup) event.preventDefault();
};
backupImport.onclick = () => void transferBackup("import");
backupExport.onclick = () => void transferBackup("export");

async function transferBackup(action: "import" | "export") {
  if (transferringBackup || saving) return;
  transferringBackup = saving = true;
  backupImport.disabled = backupExport.disabled = backupClose.disabled = true;
  const error = $("#backup-error");
  error.textContent = "";
  const filters = [{ name: "连接备份", extensions: ["json"] }];
  try {
    if (action === "import") {
      const source = await open({ title: "选择连接备份", filters });
      if (!source) return;
      const result = await invoke<{
        profiles: Profile[];
        added: number;
        skipped: number;
      }>("import_connections", { source });
      profiles = result.profiles;
      configHealthy = true;
      renderProfiles();
      notify(
        `已导入 ${result.added} 个连接，跳过 ${result.skipped} 个重复项。`,
      );
    } else {
      const destination = await save({
        title: "导出连接（请使用新的文件名）",
        defaultPath: "hshell-connections.json",
        filters,
      });
      if (!destination) return;
      const count = await invoke<number>("export_connections", { destination });
      notify(`已导出 ${count} 个连接。`);
    }
    backupDialog.close();
  } catch (failure) {
    error.textContent = String(failure);
  } finally {
    transferringBackup = saving = false;
    backupImport.disabled =
      backupExport.disabled =
      backupClose.disabled =
        false;
    if (backupDialog.open)
      (action === "import" ? backupImport : backupExport).focus();
  }
}

function updateHostHighlights() {
  const profileId = sessions.get(active)?.profile.id;
  $("#host-list")
    .querySelectorAll<HTMLElement>("[data-profile]")
    .forEach((row) => {
      const selected = row.dataset.profile === selectedHost;
      row.classList.toggle("selected", selected);
      row.classList.toggle("active", row.dataset.profile === profileId);
      row
        .querySelector(".host-connect")
        ?.setAttribute("aria-current", String(selected));
    });
}

function connectSavedHost(id: string) {
  const profile = profiles.find((p) => p.id === id);
  if (profile) void connect(profile);
}

async function persist(
  next: Profile[],
  passwordUpdate?:
    | { action: "set"; id: string; password: string }
    | { action: "remove"; id: string },
): Promise<boolean> {
  if (!configHealthy) {
    reportSaveError("配置读取失败，请修复配置文件后重启，避免覆盖原有连接。");
    return false;
  }
  if (saving) return false;
  saving = true;
  try {
    profiles = desktop
      ? await invoke<Profile[]>("store_profiles", {
          profiles: next,
          passwordUpdate: passwordUpdate ?? null,
        })
      : next;
    renderProfiles();
    return true;
  } catch (error) {
    reportSaveError(`保存失败：${error}`);
    return false;
  } finally {
    saving = false;
  }
}

function reportSaveError(message: string) {
  if ($<HTMLDialogElement>("#profile-dialog").open) {
    $("#form-error").textContent = message;
    $("#form-error").scrollIntoView({ block: "nearest" });
  } else {
    notify(message);
  }
}

function openEditor(profile?: Profile, asNew = false) {
  editing = asNew ? null : (profile?.id ?? null);
  const form = $<HTMLFormElement>("#profile-form");
  form.reset();
  for (const key of [
    "name",
    "host",
    "username",
    "port",
    "identityFile",
    "group",
  ] as const) {
    (form.elements.namedItem(key) as HTMLInputElement).value = String(
      profile?.[key] ?? "",
    );
  }
  (form.elements.namedItem("favorite") as HTMLInputElement).checked =
    profile?.favorite ?? false;
  const password = form.elements.namedItem("password") as HTMLInputElement;
  const remember = form.elements.namedItem(
    "rememberPassword",
  ) as HTMLInputElement;
  password.value = "";
  password.placeholder =
    profile?.passwordSaved && editing
      ? "已保存；留空保持原密码"
      : "输入要保存的密码";
  remember.checked = !!(editing && profile?.passwordSaved);
  $("#password-hint").textContent = !desktop
    ? "浏览器预览不能保存密码，请使用桌面客户端。"
    : profile?.passwordSaved && editing
      ? "已保存到系统凭据库。输入可替换；取消勾选可清除。"
      : "勾选后存入系统凭据库，连接配置中不写入明文。";
  password.disabled = !desktop;
  remember.disabled = !desktop;
  $("#editor-title").textContent = editing ? "编辑连接" : "新建连接";
  $("#delete-profile").hidden = !editing;
  $("#form-error").textContent = "";
  $<HTMLDialogElement>("#profile-dialog").showModal();
  (form.elements.namedItem("name") as HTMLInputElement).focus();
}

$("#profile-form").addEventListener("submit", (event) => {
  event.preventDefault();
  void saveEditor();
});
async function saveEditor() {
  const form = new FormData($<HTMLFormElement>("#profile-form"));
  const val = (key: string) => String(form.get(key) ?? "").trim();
  const profile: Profile = {
    id: editing ?? crypto.randomUUID(),
    name: val("name"),
    host: val("host"),
    username: val("username"),
    port: val("port") ? Number(val("port")) : null,
    identityFile: val("identityFile"),
    group: val("group"),
    favorite: form.has("favorite"),
  };
  if (!profile.name || !/^[a-zA-Z0-9_:][a-zA-Z0-9._:-]*$/.test(profile.host)) {
    $("#form-error").textContent =
      "请输入名称和有效的主机地址或 SSH 别名；用户名请填在单独的字段中。";
    return;
  }
  if (/^[\s-]|\s|[\x00-\x1f\x7f]/.test(profile.username)) {
    $("#form-error").textContent = "用户名不能含空白、控制字符或以 - 开头。";
    return;
  }
  const old = profiles.find((p) => p.id === editing);
  let password = String(form.get("password") ?? "");
  const remember = form.has("rememberPassword");
  const sameTarget =
    old &&
    old.host === profile.host &&
    old.username === profile.username &&
    old.port === profile.port;
  if (remember && !password && !(old?.passwordSaved && sameTarget)) {
    $("#form-error").textContent =
      "请输入要保存的密码；连接目标改变后，需要重新输入密码。";
    return;
  }
  if (password && !remember) {
    $("#form-error").textContent =
      "如需保存密码，请勾选“保存密码到本机系统凭据库”。";
    return;
  }
  const passwordUpdate =
    remember && password
      ? { action: "set" as const, id: profile.id, password }
      : old?.passwordSaved && !remember
        ? { action: "remove" as const, id: profile.id }
        : undefined;
  const next = editing
    ? profiles.map((p) => (p.id === editing ? profile : p))
    : [...profiles, profile];
  $<HTMLButtonElement>("#save-profile").disabled = true;
  if (await persist(next, passwordUpdate)) {
    $<HTMLDialogElement>("#profile-dialog").close();
    (
      $<HTMLFormElement>("#profile-form").elements.namedItem(
        "password",
      ) as HTMLInputElement
    ).value = "";
    notify(desktop ? "连接已保存。" : "连接已添加到浏览器预览；刷新后清空。");
  }
  password = "";
  if (passwordUpdate?.action === "set") passwordUpdate.password = "";
  $<HTMLButtonElement>("#save-profile").disabled = false;
}
$("#delete-profile").onclick = async () => {
  const id = editing;
  if (
    await confirmAction(
      "删除这条连接？",
      "将删除连接及其系统凭据库中的密码。已打开的会话会继续运行。",
      "删除连接",
    )
  ) {
    if (await persist(profiles.filter((p) => p.id !== id)))
      $<HTMLDialogElement>("#profile-dialog").close();
  }
};

async function connect(profile: Profile, preview = false) {
  if (!desktop && !preview) {
    notify(
      "当前是界面预览。真实 SSH 连接请使用桌面客户端（npm run desktop）。",
    );
    return;
  }
  if (sessions.size >= 12) {
    notify("最多保留 12 个终端标签，请先关闭不再使用的会话。");
    return;
  }
  const { TerminalView, loadTerminalFonts } = await import("./terminal");
  await loadTerminalFonts();
  // 模块和字体加载后再次检查，避免快速点击超过会话上限。
  if (sessions.size >= 12) return;
  const view = new TerminalView(
    profile,
    renderTabs,
    notify,
    (lines) =>
      confirmAction(
        "确认粘贴多行内容？",
        `内容包含 ${lines} 行，换行可能立即执行远端命令。请确认剪贴板内容可信。`,
        "粘贴到终端",
      ),
    preview,
  );
  sessions.set(view.id, view);
  view.mount($("#terminal-panes"));
  activate(view.id);
  await view.start();
}

function quickProfile(): Profile | null {
  try {
    const profile = parseQuickConnection(
      $<HTMLInputElement>("#quick-host").value,
      $<HTMLInputElement>("#quick-port").value,
    );
    $("#quick-error").hidden = true;
    return profile;
  } catch (error) {
    $("#quick-error").textContent =
      error instanceof Error ? error.message : String(error);
    $("#quick-error").hidden = false;
    $("#quick-host").focus();
    return null;
  }
}

async function quickConnect() {
  const profile = quickProfile();
  if (profile) await connect(profile);
}

async function disconnectSession() {
  const view = sessions.get(active);
  if (!view || view.preview || view.status === "exited") return;
  if (
    !(await confirmAction(
      "断开当前会话？",
      `将断开「${view.profile.name}」的 SSH 连接，终端输出会保留。`,
      "断开连接",
    ))
  )
    return;
  try {
    await invoke("close_session", { id: view.id });
  } catch (error) {
    notify(`断开失败：${error}`);
  }
}

function activate(id: string) {
  active = id;
  $("#home").hidden = id !== "home";
  $("#terminal-workspace").hidden = id === "home";
  $("#terminal-search").hidden = true;
  for (const [key, view] of sessions) view.setVisible(key === id);
  const view = sessions.get(id);
  files.setProfile(view && !view.preview ? view.profile : null);
  renderTabs();
}

function renderTabs() {
  $("#home-tab").classList.toggle("active", active === "home");
  $<HTMLButtonElement>("#tab-search").disabled = active === "home";
  updateHostHighlights();
  $("#sidebar-session-count").textContent = String(sessions.size);
  $("#open-sessions").innerHTML = sessions.size
    ? [...sessions.values()]
        .map(
          (view) =>
            `<button class="open-session ${view.id === active ? "active" : ""}" data-tab="${view.id}">${icon("TerminalSquare", 13)}<span>${escape(view.profile.name)}</span><span class="tiny-dot ${view.status === "exited" ? "ended" : ""}"></span></button>`,
        )
        .join("")
    : '<p class="no-sessions">暂无会话</p>';
  $("#tabs").innerHTML = [...sessions.values()]
    .map(
      (view) =>
        `<div class="terminal-tab ${active === view.id ? "active" : ""}"><button role="tab" aria-selected="${active === view.id}" data-tab="${view.id}"><span class="tiny-dot ${view.status === "exited" ? "ended" : ""}"></span><span>${escape(view.profile.name)}</span><small>${view.preview ? "演示" : "SSH"}</small></button><button class="tab-close" data-close-tab="${view.id}" aria-label="关闭 ${escape(view.profile.name)}">${icon("X", 13)}</button></div>`,
    )
    .join("");
  $("#session-count").textContent =
    `${[...sessions.values()].filter((s) => s.status !== "exited" && !s.preview).length} 个运行会话`;
  const view = sessions.get(active);
  $("#terminal-tab-key").hidden = !view || view.preview;
  $<HTMLButtonElement>("#terminal-tab-key").disabled =
    !view || view.preview || view.status !== "running";
  if (view) {
    const terminalAddress = $("#terminal-address");
    terminalAddress.title = terminalAddress.textContent = view.preview
      ? "本地终端预览"
      : address(view.profile);
    $("#terminal-size").textContent = `${view.term.cols} × ${view.term.rows}`;
    $("#session-status").textContent = view.preview
      ? "演示模式 · 未连接服务器"
      : view.status === "starting"
        ? "启动中"
        : view.status === "running"
          ? "会话运行中"
          : "会话已结束";
    $("#session-dot").classList.toggle("ended", view.status === "exited");
    $("#exit-banner").hidden = view.status !== "exited";
    $("#exit-banner").textContent = view.preview
      ? "本地演示已结束。可以关闭标签，或新建一个终端预览。"
      : (view.exitDetail ??
        `SSH 进程已结束${view.exitCode !== null ? `（退出码 ${view.exitCode}）` : ""}。终端输出已保留，可以点击右上角重新连接。`);
    $("#reconnect").hidden = view.status !== "exited" || view.preview;
    $("#disconnect").hidden = view.status === "exited" || view.preview;
  } else {
    $("#terminal-size").textContent = "";
  }
}

async function closeTab(id: string, ask = true) {
  const view = sessions.get(id);
  if (!view) return;
  if (
    ask &&
    view.status !== "exited" &&
    !view.preview &&
    !(await confirmAction(
      "关闭这个会话？",
      `将断开「${view.profile.name}」的 SSH 连接。需要继续的远端任务建议先放入 tmux 或 screen。`,
      "断开连接",
    ))
  )
    return;
  sessions.delete(id);
  if (active === id) activate([...sessions.keys()].at(-1) ?? "home");
  else renderTabs();
  try {
    await view.dispose();
  } catch (error) {
    notify(`清理会话失败：${error}`);
  }
}

function searchTerminal(previous = false) {
  const value = $<HTMLInputElement>("#terminal-query").value;
  const found = sessions.get(active)?.find(value, previous);
  $("#search-result").textContent = value && !found ? "未找到" : "";
}
function showSearch() {
  if (!sessions.has(active)) return;
  $("#terminal-search").hidden = false;
  $<HTMLInputElement>("#terminal-query").focus();
}
function showInfo() {
  $("#environment-details").innerHTML = environment
    ? `<strong>${escape(environment.sshVersion)}</strong><span>连接配置：${escape(environment.configPath)}</span>`
    : `<strong>${desktop ? "OpenSSH 不可用，请检查安装" : "浏览器界面预览"}</strong><span>${desktop ? "Ubuntu：sudo apt install openssh-client" : "预览中的连接仅保存在内存中，桌面版才会连接 SSH 并持久保存配置。"}</span>`;
  $<HTMLDialogElement>("#info-dialog").showModal();
}

document.addEventListener("click", (event) => {
  const target = (event.target as Element).closest<HTMLElement>("button, a");
  if (!target) return;
  if (target.dataset.close)
    $<HTMLDialogElement>(`#${target.dataset.close}`).close();
  if (target.dataset.selectHost) {
    selectedHost = target.dataset.selectHost;
    updateHostHighlights();
    if (event.detail === 0) connectSavedHost(selectedHost);
  }
  if (target.dataset.connect) connectSavedHost(target.dataset.connect);
  if (target.dataset.edit) {
    const p = profiles.find((p) => p.id === target.dataset.edit);
    if (p) openEditor(p);
  }
  if (target.dataset.favorite)
    void persist(
      profiles.map((p) =>
        p.id === target.dataset.favorite ? { ...p, favorite: !p.favorite } : p,
      ),
    );
  if (target.dataset.tab) activate(target.dataset.tab);
  if (target.dataset.closeTab) void closeTab(target.dataset.closeTab);
  if (target.dataset.filter) {
    filter = target.dataset.filter;
    renderProfiles();
  }
});
$("#host-list").addEventListener("dblclick", (event) => {
  const button = (event.target as Element).closest<HTMLButtonElement>(
    "[data-select-host]",
  );
  if (button?.dataset.selectHost) connectSavedHost(button.dataset.selectHost);
});
for (const id of ["new-connection", "sidebar-add", "start-save"])
  $(`#${id}`).onclick = () => openEditor();
for (const id of ["settings", "show-shortcuts", "status-help"])
  $(`#${id}`).onclick = showInfo;
function newSession() {
  activate("home");
  $("#quick-host").focus();
}
$("#home-tab").onclick = newSession;
$("#brand").onclick = newSession;
$("#start-connect").onclick = () => $("#quick-host").focus();
$("#toggle-sidebar").onclick = () => {
  const hidden = $("#app").classList.toggle("sidebar-hidden");
  $("#toggle-sidebar").setAttribute("aria-expanded", String(!hidden));
};
$("#tab-search").onclick = showSearch;
$("#quick-connect").onsubmit = (event) => {
  event.preventDefault();
  void quickConnect();
};
$("#quick-save").onclick = () => {
  const profile = quickProfile();
  if (profile) openEditor(profile, true);
};
for (const id of ["quick-host", "quick-port"])
  $(`#${id}`).oninput = () => {
    $("#quick-error").hidden = true;
  };
$("#preview-terminal").onclick = () =>
  void connect(
    {
      id: "local-preview",
      name: "终端预览",
      host: "preview",
      username: "",
      port: null,
      identityFile: "",
      group: "",
      favorite: false,
    },
    true,
  );
$("#terminal-copy").onclick = () => {
  const selection = sessions.get(active)?.term.getSelection();
  if (!selection) {
    notify("请先在终端中选中要复制的内容。");
    return;
  }
  void copyText(selection).catch(() => notify("复制失败，请检查剪贴板权限。"));
};
function renderTerminalFontSize(size: number) {
  $("#terminal-font-size").textContent = `${size} px`;
  $("#terminal-font-reset").setAttribute(
    "aria-label",
    `终端字号 ${size} 像素，恢复默认 ${DEFAULT_TERMINAL_FONT_SIZE} 像素`,
  );
  $<HTMLButtonElement>("#terminal-font-decrease").disabled =
    size <= MIN_TERMINAL_FONT_SIZE;
  $<HTMLButtonElement>("#terminal-font-increase").disabled =
    size >= MAX_TERMINAL_FONT_SIZE;
}
renderTerminalFontSize(getTerminalFontSize());
onTerminalFontSizeChange(renderTerminalFontSize);
for (const [id, delta] of [
  ["terminal-font-decrease", -1],
  ["terminal-font-reset", 0],
  ["terminal-font-increase", 1],
] as const) {
  $(`#${id}`).onclick = (event) => {
    setTerminalFontSize(
      delta ? getTerminalFontSize() + delta : DEFAULT_TERMINAL_FONT_SIZE,
    );
    if (event.detail > 0) sessions.get(active)?.term.focus();
  };
}
$("#terminal-tab-key").onclick = () => sessions.get(active)?.sendTab();
$("#disconnect").onclick = () => void disconnectSession();
$("#host-search").oninput = (event) => {
  query = (event.target as HTMLInputElement).value;
  renderProfiles();
};
$("#terminal-search-button").onclick = showSearch;
$("#terminal-query").oninput = () => searchTerminal();
$("#terminal-query").onkeydown = (event) => {
  if (event.key === "Enter") searchTerminal(event.shiftKey);
};
$("#search-prev").onclick = () => searchTerminal(true);
$("#search-next").onclick = () => searchTerminal();
$("#search-close").onclick = () => {
  $("#terminal-search").hidden = true;
  sessions.get(active)?.term.focus();
};
$("#reconnect").onclick = async () => {
  const view = sessions.get(active);
  if (!view || view.status !== "exited") return;
  const profile =
    profiles.find((p) => p.id === view.profile.id) ?? view.profile;
  await closeTab(view.id, false);
  await connect(profile);
};
document.addEventListener("keydown", (event) => {
  if (event.isComposing || document.querySelector("dialog[open]")) return;
  const key = event.key.toLowerCase();
  if (
    event.ctrlKey &&
    event.shiftKey &&
    !event.altKey &&
    ["t", "w", "f"].includes(key)
  ) {
    event.preventDefault();
    if (key === "t") openEditor();
    if (key === "w" && active !== "home") void closeTab(active);
    if (key === "f") showSearch();
  }
  if (
    event.ctrlKey &&
    event.shiftKey &&
    !event.altKey &&
    ["k", "b"].includes(key)
  ) {
    event.preventDefault();
    if (key === "k") $("#quick-host").focus();
    if (key === "b") $("#toggle-sidebar").click();
  }
  if (event.altKey && !event.ctrlKey && /^[1-9]$/.test(key)) {
    event.preventDefault();
    const id = [...sessions.keys()][Number(key) - 1];
    if (id) activate(id);
  }
  if (
    key === "/" &&
    active === "home" &&
    !(event.target instanceof HTMLInputElement)
  ) {
    event.preventDefault();
    $("#host-search").focus();
  }
  if (key === "Escape" && !$("#terminal-search").hidden)
    $("#search-close").click();
});

async function boot() {
  renderProfiles();
  if (!desktop) {
    $("#engine-state").textContent = "浏览器预览 · 未连接";
    $("#app-state").textContent = "界面预览 · SSH 连接请使用桌面客户端";
    $("#app-state-dot").classList.add("ended");
    return;
  }
  const appWindow = getCurrentWindow();
  await appWindow.onCloseRequested(
    createCloseHandler({
      sessions: () => sessions.values(),
      hasBackgroundWork: () => invoke<boolean>("sftp_busy"),
      cleanup: () => files.shutdown(),
      confirm: () =>
        confirmAction(
          "退出HShell？",
          "退出将断开 SSH 会话并暂停文件传输。未完成传输保留断点，下次可手动继续。",
          "断开并退出",
        ),
      destroy: () => appWindow.destroy(),
      reportError: notify,
    }),
  );
  await files.boot().catch((error) => notify(`读取传输任务失败：${error}`));
  const results = await Promise.allSettled([
    invoke<Profile[]>("get_profiles"),
    invoke<Environment>("environment"),
  ]);
  if (results[0].status === "fulfilled") {
    profiles = results[0].value;
    renderProfiles();
  } else {
    configHealthy = false;
    notify(String(results[0].reason));
  }
  if (results[1].status === "fulfilled") {
    environment = results[1].value;
    $("#platform-name").textContent =
      environment.platform === "windows" ? "本机 · Windows" : "本机 · Linux";
    $("#engine-state").textContent = environment.sshVersion.split(",")[0];
  } else {
    $("#engine-state").textContent = "未找到 OpenSSH";
    $("#app-state").textContent = "SSH 引擎不可用";
    $("#app-state-dot").classList.add("ended");
    notify(String(results[1].reason));
  }
}
files = new FilesPane($("#files-pane"), notify, confirmAction);
bindFilesResize($("#files-resizer"), $("#files-pane"));
bindSnippetLibrary($("#open-snippets"), notify, confirmAction, (command) => {
  const view = sessions.get(active);
  if (!view?.insertSnippet(command)) return false;
  requestAnimationFrame(() => {
    if (
      sessions.get(active) === view &&
      !document.querySelector("dialog[open]")
    )
      view.term.focus();
  });
  return true;
});
$("#toggle-files").onclick = () =>
  desktop ? files.toggle() : notify("SFTP 文件传输请使用桌面客户端。");
const bootReady = boot().catch((error) => notify(String(error)));
// @ts-ignore Vite 替换此编译期标记，正式构建不包含测试入口。
if (import.meta.env.VITE_PERF_PROBE === "1") {
  void bootReady
    .then(() => import("./perf-probe"))
    .then((p) =>
      p.runProbe({
        connect,
        sessions: () => [...sessions.values()],
      }),
    );
}

$("#profile-dialog").addEventListener("close", () => {
  (
    $<HTMLFormElement>("#profile-form").elements.namedItem(
      "password",
    ) as HTMLInputElement
  ).value = "";
});

const passwordInput = $<HTMLInputElement>(
  '#profile-form input[name="password"]',
);
const rememberPassword = $<HTMLInputElement>(
  '#profile-form input[name="rememberPassword"]',
);
passwordInput.oninput = () => {
  if (passwordInput.value) rememberPassword.checked = true;
};
rememberPassword.onchange = () => {
  if (!rememberPassword.checked) passwordInput.value = "";
};
