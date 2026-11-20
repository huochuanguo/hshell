import { invoke } from "@tauri-apps/api/core";
import {
  ArrowUp,
  File,
  Folder,
  HardDrive,
  House,
  Monitor,
  Upload,
  createElement,
} from "lucide";

interface LocalEntry {
  name: string;
  path: string;
  directory: boolean;
}

interface LocalDirectory {
  path: string;
  parent: string | null;
  entries: LocalEntry[];
}

function icon([tag, attrs, children]: Parameters<typeof createElement>[0]) {
  return createElement([
    tag,
    { ...attrs, width: 16, height: 16, "aria-hidden": "true" },
    children,
  ]);
}

function rowLabel(symbol: SVGElement, name: string, type?: string) {
  const label = document.createElement("span");
  label.className = "upload-picker-name";
  const text = document.createElement("span");
  text.textContent = name;
  text.title = name;
  label.append(symbol, text);
  if (!type) return [label];
  const kind = document.createElement("span");
  kind.className = "upload-picker-kind";
  kind.textContent = type;
  return [label, kind];
}

class LocalItems {
  private entries: LocalEntry[] = [];
  private rows: HTMLElement[] = [];
  private selected = new Set<number>();
  private current = -1;
  private anchor = -1;
  private search = "";
  private lastKey = 0;

  constructor(
    private list: HTMLElement,
    private update: () => void,
    private open: (entry: LocalEntry) => void,
    private busy: () => boolean,
  ) {
    list.onclick = (event) => {
      const row = (event.target as Element).closest<HTMLElement>(
        "[data-index]",
      );
      if (!row || this.busy()) return;
      const index = Number(row.dataset.index);
      list.focus({ preventScroll: true });
      this.move(index, event.shiftKey, event.ctrlKey || event.metaKey, true);
    };
    list.ondblclick = (event) => {
      const row = (event.target as Element).closest<HTMLElement>(
        "[data-index]",
      );
      if (row && !event.ctrlKey && !event.metaKey && !event.shiftKey)
        this.openFolder(Number(row.dataset.index));
    };
    list.onkeydown = (event) => this.key(event);
  }

  replace(entries: LocalEntry[]) {
    this.entries = entries;
    this.rows = entries.map((entry, index) => {
      const row = document.createElement("div");
      const type = entry.directory ? "文件夹" : "文件";
      row.id = `upload-picker-item-${index}`;
      row.className = "upload-picker-row";
      row.dataset.index = String(index);
      if (entry.directory) row.dataset.directory = "";
      row.setAttribute("role", "option");
      row.setAttribute("aria-selected", "false");
      row.setAttribute("aria-label", `${entry.name}，${type}`);
      row.append(
        ...rowLabel(icon(entry.directory ? Folder : File), entry.name, type),
      );
      return row;
    });
    this.selected.clear();
    this.current = this.anchor = -1;
    this.search = "";
    const fragment = document.createDocumentFragment();
    for (const row of this.rows) fragment.append(row);
    this.list.replaceChildren(fragment);
    this.list.scrollTop = 0;
    if (entries.length) this.highlight(0, false);
    else this.list.removeAttribute("aria-activedescendant");
  }

  selection() {
    return this.selected.size === 0
      ? []
      : [...this.selected]
          .sort((a, b) => a - b)
          .map((index) => this.entries[index]);
  }

  private highlight(index: number, scroll: boolean) {
    this.rows[this.current]?.removeAttribute("data-focused");
    const row = this.rows[index];
    this.current = index;
    row.dataset.focused = "";
    this.list.setAttribute("aria-activedescendant", row.id);
    if (scroll) row.scrollIntoView({ block: "nearest", inline: "nearest" });
  }

  private select(next: Set<number>) {
    for (const index of this.selected)
      if (!next.has(index))
        this.rows[index].setAttribute("aria-selected", "false");
    for (const index of next)
      if (!this.selected.has(index))
        this.rows[index].setAttribute("aria-selected", "true");
    this.selected = next;
    this.update();
  }

  private move(
    index: number,
    range: boolean,
    extend: boolean,
    pointer = false,
  ) {
    const previous = this.current;
    this.highlight(index, !pointer);
    if (range) {
      if (this.anchor < 0) this.anchor = previous < 0 ? index : previous;
      const next = extend ? new Set(this.selected) : new Set<number>();
      for (
        let i = Math.min(index, this.anchor);
        i <= Math.max(index, this.anchor);
        i++
      )
        next.add(i);
      this.select(next);
    } else if (pointer && extend) this.toggle(index);
    else if (!extend) {
      this.anchor = index;
      this.select(new Set([index]));
    }
  }

  private toggle(index: number) {
    this.anchor = index;
    const next = new Set(this.selected);
    if (!next.delete(index)) next.add(index);
    this.select(next);
  }

  private openFolder(index: number) {
    const entry = this.entries[index];
    if (!this.busy() && entry?.directory && this.selected.size <= 1)
      this.open(entry);
  }

  private key(event: KeyboardEvent) {
    if (!this.rows.length || this.busy() || event.isComposing) return;
    const { key, shiftKey, altKey } = event;
    const extend = event.ctrlKey || event.metaKey;
    if (altKey) return;
    if (key === "Enter") this.openFolder(this.current);
    else if (key === " ") this.toggle(this.current);
    else if (extend && key.toLowerCase() === "a")
      this.select(new Set(this.rows.map((_, index) => index)));
    else {
      const page = Math.max(
        1,
        Math.floor(this.list.clientHeight / this.rows[0].offsetHeight) - 1,
      );
      const offset =
        key === "ArrowUp"
          ? -1
          : key === "ArrowDown"
            ? 1
            : key === "PageUp"
              ? -page
              : key === "PageDown"
                ? page
                : 0;
      let index =
        key === "Home"
          ? 0
          : key === "End"
            ? this.rows.length - 1
            : this.current + offset;
      if (!offset && key !== "Home" && key !== "End") {
        if (extend || key.length !== 1) return;
        const now = performance.now();
        const letter = key.toLocaleLowerCase();
        this.search = now - this.lastKey < 700 ? this.search + letter : letter;
        this.lastKey = now;
        // 连续按同一个字母时，在匹配的名称之间循环。
        const repeated = [...this.search].every((part) => part === letter);
        const prefix = repeated ? letter : this.search;
        const first = this.current + (repeated ? 1 : 0);
        for (let step = 0; step < this.entries.length; step++) {
          const candidate = (first + step) % this.entries.length;
          if (
            this.entries[candidate].name.toLocaleLowerCase().startsWith(prefix)
          ) {
            this.move(candidate, false, false);
            break;
          }
        }
        event.preventDefault();
        return;
      }
      index = Math.max(0, Math.min(index, this.rows.length - 1));
      this.move(index, shiftKey, extend);
    }
    event.preventDefault();
  }
}

export class UploadPicker {
  private dialog = document.createElement("dialog");
  private directory: LocalDirectory | null = null;

  constructor() {
    this.dialog.className = "upload-picker";
    this.dialog.setAttribute("aria-labelledby", "upload-picker-title");
    this.dialog.innerHTML = `
      <header class="upload-picker-heading">
        <h2 id="upload-picker-title">选择上传项目</h2>
        <p class="upload-picker-target">上传到 <span></span></p>
      </header>
      <div class="upload-picker-browser">
        <nav class="upload-picker-places" aria-label="本机位置">
          <p>本机位置</p>
          <button type="button" class="upload-picker-place" data-computer>${icon(Monitor).outerHTML}此电脑</button>
          <button type="button" class="upload-picker-place" data-home>${icon(House).outerHTML}主目录</button>
          <p>驱动器</p>
          <div class="upload-picker-drive-buttons" role="group" aria-label="可用盘符"></div>
        </nav>
        <div class="upload-picker-content">
          <div class="upload-picker-location">
            <button type="button" class="icon-button" data-parent aria-label="上一级" title="上一级">${icon(ArrowUp).outerHTML}</button>
            <div class="upload-picker-path">
              <input aria-label="本地目录" placeholder="输入本地目录" spellcheck="false" autocomplete="off">
              <button type="button" data-go>前往</button>
            </div>
          </div>
          <div class="upload-picker-columns" aria-hidden="true"><span>名称</span><span>类型</span></div>
          <div class="upload-picker-entries">
            <div class="upload-picker-list" role="listbox" tabindex="0" aria-label="本地文件和文件夹" aria-describedby="upload-picker-help" aria-multiselectable="true"></div>
            <div class="upload-picker-drive-overview" aria-label="此电脑中的驱动器" role="group" hidden></div>
            <p class="upload-picker-empty" hidden></p>
          </div>
        </div>
      </div>
      <footer class="upload-picker-footer">
        <div class="upload-picker-feedback">
          <p class="upload-picker-status" role="status"></p>
          <p id="upload-picker-help">Ctrl 多选 · Shift 连选 · 双击打开文件夹</p>
        </div>
        <div class="dialog-actions">
          <button type="button" class="secondary" data-cancel>取消</button>
          <button type="button" class="primary" data-accept>${icon(Upload).outerHTML}上传</button>
        </div>
      </footer>`;
    this.dialog.addEventListener(
      "keydown",
      (event) => {
        if (event.key !== "Escape" || event.isComposing) return;
        event.preventDefault();
        event.stopPropagation();
        this.close();
      },
      { capture: true },
    );
    document.body.append(this.dialog);
  }

  close() {
    if (this.dialog.open) this.dialog.close();
  }

  hasFocus() {
    return this.dialog.contains(document.activeElement);
  }

  choose(target: string): Promise<string[]> {
    const dialog = this.dialog;
    const get = <T extends HTMLElement = HTMLElement>(selector: string) =>
      dialog.querySelector<T>(selector)!;
    const input = get<HTMLInputElement>("input");
    const list = get(".upload-picker-list");
    const overview = get(".upload-picker-drive-overview");
    const empty = get(".upload-picker-empty");
    const driveButtons = get(".upload-picker-drive-buttons");
    const status = get(".upload-picker-status");
    const help = get("#upload-picker-help");
    const button = (action: string) =>
      get<HTMLButtonElement>(`[data-${action}]`);
    const accept = button("accept");
    const parent = button("parent");
    const computer = button("computer");
    const go = button("go");
    const cancel = button("cancel");
    let atComputer = false;
    let pending = false;
    let active = true;
    let navigation = 0;
    let driveRequest = 0;
    let paths: string[] = [];
    let driveMessage = "正在读取驱动器…";
    let shownDirectory: LocalDirectory | null = null;
    let message: string | undefined;
    let error = false;
    const localDrive = () => {
      const letter = this.directory?.path.match(
        /^(?:\\\\\?\\)?([a-z]):[\\/]/i,
      )?.[1];
      return letter ? `${letter.toUpperCase()}:\\` : undefined;
    };
    const driveItems = (root: HTMLElement) => [
      ...root.querySelectorAll("button"),
    ];
    const update = () => {
      const selection = items.selection();
      const count = selection.length;
      const total = shownDirectory?.entries.length ?? 0;
      accept.disabled = pending || atComputer || !count;
      parent.disabled = atComputer || !shownDirectory;
      go.disabled = !input.value;
      list.hidden = atComputer;
      overview.hidden = !atComputer;
      empty.hidden = atComputer ? paths.length > 0 : total > 0;
      empty.textContent = atComputer
        ? driveMessage
        : pending
          ? "正在读取本地目录…"
          : shownDirectory
            ? "此目录没有可上传的项目"
            : "请从左侧选择一个位置";
      if (pending) list.setAttribute("aria-busy", "true");
      else list.removeAttribute("aria-busy");
      computer.setAttribute("aria-current", String(atComputer));
      const currentDrive =
        atComputer || !shownDirectory ? undefined : localDrive();
      for (const drive of driveItems(driveButtons))
        drive.setAttribute(
          "aria-current",
          String(drive.value === currentDrive),
        );
      const folders = selection.filter((entry) => entry.directory).length;
      status.textContent =
        message ??
        (atComputer
          ? "选择要打开的驱动器"
          : count
            ? `已选 ${count} 项${folders && folders < count ? ` · ${folders} 个文件夹，${count - folders} 个文件` : ""}`
            : `${total} 个项目`);
      status.title = status.textContent;
      status.toggleAttribute("data-error", error);
      help.textContent = atComputer
        ? "点击盘符以浏览其中的文件和文件夹"
        : "Ctrl 多选 · Shift 连选 · 双击打开文件夹";
    };
    const navigate = async (path: string | null) => {
      const request = ++navigation;
      const from = document.activeElement;
      pending = true;
      error = false;
      message = path ? `正在打开 ${path}…` : "正在打开主目录…";
      update();
      const current = () => active && dialog.open && navigation === request;
      try {
        const directory = await invoke<LocalDirectory>(
          "sftp_local_upload_directory",
          { path },
        );
        if (!current()) return;
        this.directory = shownDirectory = directory;
        atComputer = false;
        input.value = directory.path;
        input.placeholder = "输入本地目录";
        items.replace(directory.entries);
        pending = false;
        message = undefined;
        update();
        if (
          (document.activeElement === from && from !== cancel) ||
          (from &&
            overview.contains(from) &&
            document.activeElement === document.body)
        )
          list.focus({ preventScroll: true });
      } catch (reason) {
        if (!current()) return;
        pending = false;
        error = true;
        message = path ? `${path}：${String(reason)}` : String(reason);
        update();
      }
    };
    const items = new LocalItems(
      list,
      () => {
        message = undefined;
        error = false;
        update();
      },
      ({ path }) => void navigate(path),
      () => pending,
    );
    const showDriveMessage = (text: string) => {
      const hint = document.createElement("p");
      hint.className = "upload-picker-drive-message";
      hint.textContent = text;
      driveButtons.replaceChildren(hint);
    };
    const loadDrives = async () => {
      const request = ++driveRequest;
      driveButtons.setAttribute("aria-busy", "true");
      if (!paths.length) showDriveMessage((driveMessage = "正在读取…"));
      try {
        const next = await invoke<string[]>("sftp_local_upload_drives");
        if (!active || !dialog.open || request !== driveRequest) return;
        if (next.length && next.join() === paths.join()) return;
        paths = next;
        const render = (root: HTMLElement, main: boolean) => {
          const focused = driveItems(root).find(
            (drive) => drive === document.activeElement,
          )?.value;
          const buttons = paths.map((path) => {
            const drive = document.createElement("button");
            drive.type = "button";
            drive.className = main
              ? "upload-picker-row"
              : "upload-picker-place";
            drive.value = path;
            drive.title = `打开 ${path}`;
            drive.append(
              ...rowLabel(
                icon(HardDrive),
                path.slice(0, 2),
                main ? "驱动器" : undefined,
              ),
            );
            drive.onclick = () => void navigate(path);
            return drive;
          });
          root.replaceChildren(...buttons);
          if (focused)
            (
              buttons.find((drive) => drive.value === focused) ?? computer
            ).focus();
        };
        render(driveButtons, false);
        render(overview, true);
        if (!paths.length) showDriveMessage((driveMessage = "没有可用驱动器"));
        update();
      } catch {
        if (
          active &&
          dialog.open &&
          request === driveRequest &&
          !paths.length
        ) {
          showDriveMessage("无法读取盘符");
          driveMessage = "无法读取驱动器，可点「此电脑」重试";
          update();
        }
      } finally {
        if (active && request === driveRequest)
          driveButtons.removeAttribute("aria-busy");
      }
    };
    const showComputer = () => {
      ++navigation;
      atComputer = true;
      pending = false;
      message = undefined;
      error = false;
      input.value = "";
      input.placeholder = "此电脑";
      shownDirectory = null;
      items.replace([]);
      update();
      const drives = driveItems(overview);
      (
        drives.find((drive) => drive.value === localDrive()) ??
        drives[0] ??
        computer
      ).focus();
      void loadDrives();
    };
    const goToPath = () => {
      if (input.value) void navigate(input.value);
    };
    let chosen: string[] = [];
    const destination = get(".upload-picker-target span");
    destination.textContent = destination.title = target;
    input.value = this.directory?.path ?? "";
    input.placeholder = "输入本地目录";
    items.replace([]);
    driveButtons.replaceChildren();
    overview.replaceChildren();
    computer.onclick = showComputer;
    button("home").onclick = () => void navigate(null);
    parent.onclick = () => {
      const path = shownDirectory?.parent;
      if (path) void navigate(path);
      else showComputer();
    };
    go.onclick = goToPath;
    input.oninput = () => {
      go.disabled = !input.value;
    };
    input.onkeydown = (event) => {
      if (event.key === "Enter" && !event.isComposing) {
        event.preventDefault();
        goToPath();
      }
    };
    cancel.onclick = () => this.close();
    accept.onclick = () => {
      chosen = items.selection().map((entry) => entry.path);
      this.close();
    };
    return new Promise((resolve) => {
      dialog.addEventListener(
        "close",
        () => {
          active = false;
          resolve(chosen);
        },
        { once: true },
      );
      update();
      dialog.showModal();
      list.focus({ preventScroll: true });
      void loadDrives();
      void navigate(this.directory?.path ?? null);
    });
  }
}
