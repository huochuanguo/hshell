import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  createElement,
  Folder,
  File,
  FileCode,
  FileArchive,
  Image,
  Link,
  Upload,
  Download,
  ArrowUp,
  RotateCw,
  FolderPlus,
  X,
} from "lucide";
import { requestAuthentication } from "./auth";
import type { Profile } from "./types";
import { remoteJoin, remoteParent, localBasename, downloadName } from "./paths";

interface Entry {
  name: string;
  directory: boolean;
  symlink: boolean;
  size: number | null;
  modified: number | null;
}
interface Task {
  id: string;
  name: string;
  host: string;
  direction: "Upload" | "Download";
  local: string;
  remote: string;
  partial: string;
  status: string;
  bytes: number;
  total: number;
  verifying: boolean;
  error: string | null;
}
type BrowseEvent =
  | { type: "ready" }
  | { type: "entries"; generation: number; entries: Entry[] }
  | {
      type: "listed";
      generation: number;
      path: string | null;
      error: string | null;
    }
  | { type: "changed"; error: string | null }
  | { type: "closed"; error: string }
  | { type: "auth"; request_id: string; prompt: string; confirm: boolean };
type TransferEvent =
  | { type: "task"; task: Task }
  | {
      type: "auth";
      id: string;
      request_id: string;
      prompt: string;
      confirm: boolean;
    };
const escape = (text: string) =>
  text.replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ]!,
  );
const icons = {
  Folder,
  File,
  FileCode,
  FileArchive,
  Image,
  Link,
  Upload,
  Download,
  ArrowUp,
  RotateCw,
  FolderPlus,
  X,
};
function icon(name: keyof typeof icons) {
  const [tag, attrs, children] = icons[name];
  return createElement([
    tag,
    { ...attrs, width: 16, height: 16, "aria-hidden": "true" },
    children,
  ]).outerHTML;
}
const size = (bytes: number | null) =>
  bytes === null
    ? "—"
    : bytes < 1024
      ? `${bytes} B`
      : bytes < 1024 ** 2
        ? `${(bytes / 1024).toFixed(1)} KiB`
        : `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
function kind(entry: Entry): keyof typeof icons {
  if (entry.symlink) return "Link";
  if (entry.directory) return "Folder";
  const ext = entry.name.split(".").at(-1)?.toLowerCase() ?? "";
  if (/^(zip|gz|tgz|bz2|xz|zst|tar|7z|rar|deb|rpm)$/.test(ext))
    return "FileArchive";
  if (/^(png|jpg|jpeg|gif|webp|svg|bmp|ico|avif)$/.test(ext)) return "Image";
  if (
    /^(sh|bash|py|rs|js|ts|jsx|tsx|go|c|h|cpp|java|json|yaml|yml|toml|ini|conf|xml|sql|css|html)$/.test(
      ext,
    )
  )
    return "FileCode";
  return "File";
}

export class FilesPane {
  private profile: Profile | null = null;
  private opened = false;
  private browser = "";
  private ready = false;
  private busy = false;
  private generation = 0;
  private directory = ".";
  private listedDirectory = "";
  private entries: Entry[] = [];
  private selected: Entry | undefined;
  private limit = 500;
  private tasks = new Map<string, Task>();
  private authAbort = new AbortController();
  private transferAuth = new Map<string, AbortController>();
  private paths = new Map<string, string>();
  private nameDialog: HTMLDialogElement;

  constructor(
    private root: HTMLElement,
    private notify: (message: string) => void,
    private confirm: (
      title: string,
      message: string,
      action?: string,
    ) => Promise<boolean>,
  ) {
    root.innerHTML = `
      <div class="files-heading"><strong>文件与传输</strong><button class="icon-button" data-action="close" aria-label="收起文件栏">${icon("X")}</button></div>
      <p class="files-host"></p>
      <form class="files-path"><button type="button" class="icon-button" data-action="up" aria-label="上一级目录">${icon("ArrowUp")}</button><input aria-label="远程 Unix 目录" placeholder="/home/user" spellcheck="false"><button type="submit" class="icon-button" aria-label="打开远程目录">${icon("RotateCw")}</button></form>
      <div class="files-actions"><button class="dark-button" data-action="upload">${icon("Upload")} 上传</button><button class="dark-button" data-action="download">${icon("Download")} 下载</button><button class="icon-button" data-action="mkdir" aria-label="新建远程目录">${icon("FolderPlus")}</button><button class="dark-button" data-action="connect">重新连接</button></div>
      <p class="files-message" role="status"></p>
      <div class="files-list" role="listbox" aria-label="远程文件" tabindex="0"></div>
      <div class="files-selection"><button class="dark-button" data-action="rename">重命名</button><button class="dark-button" data-action="remove">删除</button><span></span></div>
      <div class="transfers-heading"><strong>传输任务</strong><span>最多两路</span></div><div class="transfer-list" aria-label="传输任务"></div>
      <p class="files-note">收起文件栏后传输继续。退出保留断点，重新打开后手动继续。</p>`;
    this.nameDialog = document.createElement("dialog");
    this.nameDialog.className = "file-name-dialog";
    this.nameDialog.innerHTML = `<form><h2></h2><label>远程名称<input required maxlength="255" autocomplete="off" spellcheck="false"></label><p class="form-error" role="alert"></p><div class="dialog-actions"><button type="button" class="secondary">取消</button><button class="primary" type="submit">确定</button></div></form>`;
    document.body.append(this.nameDialog);
    root.addEventListener("click", (event) => {
      const button = (event.target as HTMLElement).closest<HTMLButtonElement>(
        "button[data-action]",
      );
      if (button && !button.disabled)
        void this.action(button.dataset.action!).catch((error) =>
          this.notify(String(error)),
        );
    });
    root.querySelector("form")!.onsubmit = (event) => {
      event.preventDefault();
      void this.list(this.input.value);
    };
    this.el(".files-list").addEventListener("keydown", (event) => {
      if (
        !["ArrowDown", "ArrowUp", "Enter"].includes(event.key) ||
        !this.entries.length
      )
        return;
      event.preventDefault();
      const index = this.selected ? this.entries.indexOf(this.selected) : -1;
      if (event.key === "Enter") {
        if (this.selected && (this.selected.directory || this.selected.symlink))
          void this.list(remoteJoin(this.directory, this.selected.name));
        return;
      }
      const next = Math.max(
        0,
        Math.min(
          this.entries.length - 1,
          index + (event.key === "ArrowDown" ? 1 : -1),
        ),
      );
      this.selected = this.entries[next];
      this.limit = Math.max(this.limit, next + 1);
      this.renderEntries();
      this.root
        .querySelector<HTMLElement>('[aria-selected="true"]')
        ?.scrollIntoView({ block: "nearest" });
    });
    this.renderTasks();
    this.controls();
  }

  private el<T extends HTMLElement = HTMLElement>(selector: string) {
    return this.root.querySelector<T>(selector)!;
  }
  private get input() {
    return this.el<HTMLInputElement>(".files-path input");
  }
  private message(text: string) {
    this.el(".files-message").textContent = text;
  }

  async boot() {
    await listen<TransferEvent>("sftp-transfer", ({ payload }) => {
      if (payload.type === "task") {
        const previous = this.tasks.get(payload.task.id);
        this.tasks.set(payload.task.id, payload.task);
        if (!["running", "pausing"].includes(payload.task.status)) {
          this.transferAuth.get(payload.task.id)?.abort();
          this.transferAuth.delete(payload.task.id);
        }
        if (
          previous?.status === payload.task.status &&
          previous?.verifying === payload.task.verifying &&
          previous?.error === payload.task.error
        )
          this.updateProgress(payload.task);
        else this.renderTasks();
        if (
          payload.task.status === "complete" &&
          previous?.status !== "complete" &&
          this.ready &&
          !this.busy &&
          this.profile?.host === payload.task.host
        )
          void this.list(this.directory);
      } else {
        let abort = this.transferAuth.get(payload.id);
        if (!abort) {
          abort = new AbortController();
          this.transferAuth.set(payload.id, abort);
        }
        void requestAuthentication(
          {
            name: this.tasks.get(payload.id)?.name ?? "SFTP 传输",
            prompt: payload.prompt,
            confirm: payload.confirm,
          },
          abort.signal,
        )
          .then(async (answer) => {
            if (abort.signal.aborted) return;
            if (answer === null)
              await invoke("sftp_task_action", {
                id: payload.id,
                action: "pause",
              });
            else
              await invoke("sftp_auth", {
                id: payload.id,
                requestId: payload.request_id,
                answer,
                transfer: true,
              });
          })
          .catch((error) => this.notify(String(error)));
      }
    });
    const tasks = await invoke<Task[]>("sftp_tasks");
    for (const task of tasks)
      if (!this.tasks.has(task.id)) this.tasks.set(task.id, task);
    this.renderTasks();
  }

  setProfile(profile: Profile | null) {
    if (
      this.profile?.id === profile?.id &&
      this.profile?.host === profile?.host
    )
      return;
    this.stopBrowser();
    this.profile = profile ? { ...profile } : null;
    this.directory = profile ? (this.paths.get(profile.id) ?? ".") : ".";
    if (this.opened) void this.startBrowser();
  }

  toggle(force = !this.opened) {
    this.opened = force;
    this.root.hidden = !force;
    const button = document.querySelector("#toggle-files");
    button?.setAttribute("aria-expanded", String(force));
    if (force) void this.startBrowser();
    else this.stopBrowser();
  }

  private stopBrowser() {
    const id = this.browser;
    this.browser = "";
    this.ready = false;
    this.busy = false;
    this.listedDirectory = "";
    this.authAbort.abort();
    this.authAbort = new AbortController();
    this.entries = [];
    this.selected = undefined;
    this.renderEntries();
    this.controls();
    if (id)
      void invoke("sftp_close", { id }).catch((error) =>
        this.notify(String(error)),
      );
  }

  private async startBrowser() {
    this.stopBrowser();
    this.el(".files-host").textContent = this.profile
      ? `${this.profile.name} · ${this.profile.host}`
      : "选择终端会话以浏览远程文件";
    if (!this.profile) {
      this.message("尚未选择服务器，已有传输任务仍可继续。");
      return;
    }
    const id = crypto.randomUUID();
    this.browser = id;
    const signal = this.authAbort.signal;
    const onEvent = new Channel<BrowseEvent>();
    onEvent.onmessage = (event) => {
      if (this.browser !== id) return;
      if (event.type === "ready") {
        this.ready = true;
        void this.list(this.directory);
      } else if (event.type === "entries") {
        if (event.generation === this.generation)
          this.entries.push(...event.entries);
      } else if (
        event.type === "listed" &&
        event.generation === this.generation
      ) {
        this.busy = false;
        if (event.error) {
          this.entries = [];
          this.message(event.error);
        } else {
          this.directory = event.path!;
          this.listedDirectory = event.path!;
          this.input.value = this.directory;
          if (this.profile) this.paths.set(this.profile.id, this.directory);
          this.entries.sort(
            (a, b) =>
              Number(b.directory) - Number(a.directory) ||
              a.name.localeCompare(b.name, "zh-CN", { numeric: true }),
          );
          this.message(
            this.entries.length ? `${this.entries.length} 项` : "目录为空",
          );
        }
        this.renderEntries();
        this.controls();
      } else if (event.type === "changed") {
        this.busy = false;
        if (event.error) {
          this.message(event.error);
          this.controls();
        } else void this.list(this.directory);
      } else if (event.type === "closed") {
        this.authAbort.abort();
        this.ready = false;
        this.busy = false;
        this.message(event.error);
        this.controls();
      } else if (event.type === "auth") {
        void requestAuthentication(
          {
            name: this.profile?.name ?? "SFTP",
            prompt: event.prompt,
            confirm: event.confirm,
          },
          signal,
        )
          .then(async (answer) => {
            if (this.browser !== id || signal.aborted) return;
            if (answer === null) {
              this.stopBrowser();
              this.message("已取消 SFTP 连接");
            } else
              await invoke("sftp_auth", {
                id,
                requestId: event.request_id,
                answer,
                transfer: false,
              });
          })
          .catch((error) => this.notify(String(error)));
      }
    };
    this.message("正在连接 SFTP…");
    try {
      await invoke("sftp_open", { id, profile: this.profile, onEvent });
      if (this.browser !== id) await invoke("sftp_close", { id });
    } catch (error) {
      if (this.browser === id) {
        this.message(String(error));
        this.controls();
      }
    }
  }

  private async list(path: string) {
    if (!this.ready || this.busy) return;
    this.busy = true;
    this.entries = [];
    this.selected = undefined;
    this.listedDirectory = "";
    this.limit = 500;
    this.renderEntries();
    this.controls();
    this.message("正在读取目录…");
    try {
      await invoke("sftp_request", {
        id: this.browser,
        request: { type: "list", generation: ++this.generation, path },
      });
    } catch (error) {
      this.busy = false;
      this.message(String(error));
      this.controls();
    }
  }

  private controls() {
    for (const button of this.root.querySelectorAll<HTMLButtonElement>(
      ".files-actions button, .files-path button, .files-selection button",
    )) {
      const action = button.dataset.action;
      button.disabled =
        action === "connect"
          ? !this.profile
          : !this.ready ||
            this.busy ||
            (action === "download" &&
              (!this.selected || this.selected.directory)) ||
            (["rename", "remove"].includes(action ?? "") && !this.selected);
    }
    this.input.disabled = !this.ready || this.busy;
  }

  private renderEntries() {
    const list = this.el(".files-list");
    list.replaceChildren();
    const fragment = document.createDocumentFragment();
    for (const entry of this.entries.slice(0, this.limit)) {
      const row = document.createElement("button");
      const category = kind(entry);
      row.className = `file-row kind-${category.toLowerCase()}`;
      row.type = "button";
      row.setAttribute("role", "option");
      row.setAttribute("aria-selected", String(this.selected === entry));
      row.tabIndex = -1;
      row.title = entry.modified
        ? `${entry.name}\n${new Date(entry.modified * 1000).toLocaleString()}`
        : entry.name;
      row.innerHTML = `${icon(category)}<span>${escape(entry.name)}</span><small>${entry.directory ? "目录" : size(entry.size)}</small>`;
      row.onclick = () => {
        this.selected = entry;
        for (const item of list.querySelectorAll('[role="option"]'))
          item.setAttribute("aria-selected", String(item === row));
        this.el(".files-selection span").textContent = entry.name;
        this.controls();
        list.focus({ preventScroll: true });
      };
      row.ondblclick = () => {
        if (entry.directory || entry.symlink)
          void this.list(remoteJoin(this.directory, entry.name));
      };
      fragment.append(row);
    }
    if (this.entries.length > this.limit) {
      const more = document.createElement("button");
      more.className = "dark-button";
      more.textContent = `显示更多（剩余 ${this.entries.length - this.limit} 项）`;
      more.onclick = () => {
        this.limit += 500;
        this.renderEntries();
      };
      fragment.append(more);
    }
    list.append(fragment);
    this.el(".files-selection span").textContent =
      this.selected?.name ?? "选择文件后操作";
    this.controls();
  }

  private updateProgress(task: Task) {
    const row = this.root.querySelector<HTMLElement>(
      `[data-transfer-id="${CSS.escape(task.id)}"]`,
    );
    if (!row) {
      this.renderTasks();
      return;
    }
    row.querySelector("progress")!.value = task.total
      ? Math.min(100, (task.bytes / task.total) * 100)
      : 0;
    row.querySelector("p")!.textContent =
      `${task.name} · ${size(task.bytes)} / ${size(task.total)}`;
  }

  private renderTasks() {
    const list = this.el(".transfer-list");
    list.replaceChildren();
    const status: Record<string, string> = {
      queued: "等待传输",
      running: "传输中",
      paused: "已暂停",
      pausing: "正在暂停",
      complete: "已完成",
    };
    for (const task of this.tasks.values()) {
      const row = document.createElement("div");
      row.className = "transfer-row";
      row.dataset.transferId = task.id;
      const progress = task.total
        ? Math.min(100, (task.bytes / task.total) * 100)
        : 0;
      row.innerHTML = `<div>${icon(task.direction)}<strong title="${escape(task.local)}">${escape(task.direction === "Upload" ? localBasename(task.local) : (task.remote.split("/").at(-1) ?? task.remote))}</strong><span>${task.verifying ? "校验断点" : (status[task.status] ?? task.status)}</span></div><progress max="100" value="${progress}" aria-label="传输进度"></progress><p>${escape(task.name)} · ${size(task.bytes)} / ${size(task.total)}</p><p class="transfer-error" ${task.error ? "" : "hidden"}></p><div class="transfer-actions"></div>`;
      row.querySelector(".transfer-error")!.textContent = task.error;
      const actions = row.querySelector(".transfer-actions")!;
      if (task.status !== "complete") {
        const button = document.createElement("button");
        button.className = "dark-button";
        const active = ["running", "queued", "pausing"].includes(task.status);
        button.textContent = active ? "暂停" : "继续";
        button.disabled = task.status === "pausing";
        button.onclick = () => {
          void invoke("sftp_task_action", {
            id: task.id,
            action: active ? "pause" : "resume",
          }).catch((error) => this.notify(String(error)));
        };
        actions.append(button);
      }
      const remove = document.createElement("button");
      remove.className = "dark-button";
      remove.textContent = "移除";
      remove.disabled = ["running", "pausing"].includes(task.status);
      remove.onclick = () => {
        void (async () => {
          if (
            task.status !== "complete" &&
            !(await this.confirm(
              "移除此传输任务？",
              `断点记录将删除，临时文件仍保留在：${task.partial}。可按需手动清理。`,
              "移除任务",
            ))
          )
            return;
          await invoke("sftp_task_action", { id: task.id, action: "remove" });
          this.tasks.delete(task.id);
          this.renderTasks();
        })().catch((error) => this.notify(String(error)));
      };
      actions.append(remove);
      list.append(row);
    }
    if (!this.tasks.size) {
      const empty = document.createElement("p");
      empty.className = "files-message";
      empty.textContent = "暂无传输任务";
      list.append(empty);
    }
  }

  private askName(title: string, initial = ""): Promise<string | null> {
    if (this.nameDialog.open) return Promise.resolve(null);
    const dialog = this.nameDialog,
      input = dialog.querySelector("input")!,
      form = dialog.querySelector("form")!,
      cancel = dialog.querySelector<HTMLButtonElement>(
        'button[type="button"]',
      )!,
      error = dialog.querySelector(".form-error")!;
    dialog.querySelector("h2")!.textContent = title;
    input.value = initial;
    error.textContent = "";
    return new Promise((resolve) => {
      const finish = (name: string | null) => {
        dialog.close();
        resolve(name);
      };
      form.onsubmit = (event) => {
        event.preventDefault();
        if (
          !input.value ||
          [".", ".."].includes(input.value) ||
          /[/\0]/.test(input.value)
        ) {
          error.textContent = "名称不能为空，不能包含斜杠或使用 .、..";
          return;
        }
        finish(input.value);
      };
      cancel.onclick = () => finish(null);
      dialog.oncancel = (event) => {
        event.preventDefault();
        finish(null);
      };
      dialog.showModal();
      input.focus();
      input.select();
    });
  }

  private async action(action: string) {
    if (action === "close") {
      this.toggle(false);
      return;
    }
    if (action === "connect") {
      await this.startBrowser();
      return;
    }
    if (!this.ready || this.busy) return;
    if (action === "up") {
      await this.list(remoteParent(this.directory));
      return;
    }
    if (action === "upload") {
      if (!this.profile || this.listedDirectory !== this.directory) return;
      const profile = this.profile,
        browser = this.browser,
        directory = this.directory;
      const local = await open({
        title: "选择要上传的文件",
        multiple: false,
        directory: false,
      });
      if (
        !local ||
        this.browser !== browser ||
        this.directory !== directory ||
        this.busy
      )
        return;
      const name = await this.askName("上传到远程目录", localBasename(local));
      if (!name || this.browser !== browser || this.directory !== directory)
        return;
      const exists = this.entries.some((entry) => entry.name === name);
      if (
        exists &&
        !(await this.confirm(
          "替换远程文件？",
          `将替换 ${remoteJoin(directory, name)}。目录不能被文件替换。`,
          "替换文件",
        ))
      )
        return;
      await invoke("sftp_enqueue", {
        profile,
        direction: "Upload",
        local,
        remotePath: remoteJoin(directory, name),
        replace: exists,
      });
      return;
    }
    const entry = this.selected;
    if (action === "download" && entry && !entry.directory && this.profile) {
      const profile = this.profile,
        remote = remoteJoin(this.directory, entry.name);
      const local = await save({
        title: "保存下载文件（选择已有文件时将替换）",
        defaultPath: downloadName(entry.name),
      });
      if (local)
        await invoke("sftp_enqueue", {
          profile,
          direction: "Download",
          local,
          remotePath: remote,
          replace: true,
        });
      return;
    }
    let request: object | undefined;
    const id = this.browser;
    if (action === "mkdir") {
      const name = await this.askName("新建远程目录");
      if (name) request = { type: "mkdir", path: this.directory, name };
    }
    if (action === "rename" && entry) {
      const name = await this.askName("重命名远程文件", entry.name);
      if (name && name !== entry.name)
        request = {
          type: "rename",
          path: remoteJoin(this.directory, entry.name),
          name,
        };
    }
    if (
      action === "remove" &&
      entry &&
      (await this.confirm(
        "删除远程项目？",
        `将永久删除 ${remoteJoin(this.directory, entry.name)}${entry.directory ? "（仅支持空目录）" : ""}。`,
        "删除",
      ))
    )
      request = {
        type: "remove",
        path: remoteJoin(this.directory, entry.name),
        directory: entry.directory && !entry.symlink,
      };
    if (request && this.browser === id) {
      this.busy = true;
      this.controls();
      this.message("正在执行文件操作…");
      try {
        await invoke("sftp_request", { id, request });
      } catch (error) {
        this.busy = false;
        this.controls();
        throw error;
      }
    }
  }

  async shutdown() {
    this.stopBrowser();
    for (const controller of this.transferAuth.values()) controller.abort();
    await invoke("sftp_shutdown");
  }
}
