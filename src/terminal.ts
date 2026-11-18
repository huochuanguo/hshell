import { Channel, invoke } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import type { Profile, TerminalEvent } from "./types";
import "@xterm/xterm/css/xterm.css";
import { requestAuthentication } from "./auth";
import { terminalAppearance } from "./appearance";
import { copyText, pasteText } from "./clipboard";
import {
  DEFAULT_TERMINAL_FONT_SIZE,
  getTerminalFontSize,
  onTerminalFontSizeChange,
  setTerminalFontSize,
  terminalFontShortcut,
} from "./terminal-font";
export { loadTerminalFonts } from "./appearance";

const INPUT_LIMIT = 1024 * 1024;

export class TerminalView {
  readonly id = crypto.randomUUID();
  readonly element = document.createElement("div");
  readonly term: Terminal;
  readonly profile: Profile;
  status: "starting" | "running" | "exited" = "starting";
  exitCode: number | null = null;
  private authAbort = new AbortController();
  private fit = new FitAddon();
  private search = new SearchAddon();
  private observer: ResizeObserver;
  private visible = false;
  private disposed = false;
  private frame = 0;
  private queue: string[] = [];
  private queuedBytes = 0;
  private writing = false;
  private sentSize = "";
  private previewInput = "";
  private stopFontSizeListener: () => void;

  constructor(
    profile: Profile,
    private changed: () => void,
    private notify: (message: string) => void,
    private confirmPaste: (lines: number) => Promise<boolean>,
    readonly preview = false,
  ) {
    this.profile = { ...profile };
    this.element.className = "terminal-pane";
    this.element.hidden = true;
    this.term = new Terminal({
      ...terminalAppearance(),
      cursorBlink: false,
      cursorStyle: "bar",
      scrollback: 3000,
      allowProposedApi: false,
      allowTransparency: false,
    });
    this.term.loadAddon(this.fit);
    this.term.loadAddon(this.search);
    this.observer = new ResizeObserver(() => this.scheduleFit());
    this.stopFontSizeListener = onTerminalFontSizeChange((size) => {
      this.term.options.fontSize = size;
      this.scheduleFit();
    });
    this.term.onData((data) => this.enqueue(data));
    this.element.addEventListener(
      "paste",
      (event) => {
        const text = event.clipboardData?.getData("text/plain");
        if (!text) return;
        if (text.length > INPUT_LIMIT || /[\r\n]/.test(text)) {
          event.preventDefault();
          event.stopImmediatePropagation();
          if (new TextEncoder().encode(text).length > INPUT_LIMIT) {
            this.notify("粘贴内容超过 1 MiB，请通过文件传输处理。");
            return;
          }
          void this.confirmPaste(text.split(/\r\n|\r|\n/).length).then((ok) => {
            if (ok && !this.disposed && this.status === "running")
              this.term.paste(text);
          });
        }
      },
      true,
    );
    this.term.attachCustomKeyEventHandler((event) => {
      const fontAction = terminalFontShortcut(event);
      if (fontAction) {
        event.preventDefault();
        if (event.type === "keydown")
          setTerminalFontSize(
            fontAction === "reset"
              ? DEFAULT_TERMINAL_FONT_SIZE
              : getTerminalFontSize() + (fontAction === "increase" ? 1 : -1),
          );
        return false;
      }
      if (event.altKey && !event.ctrlKey && /^[1-9]$/.test(event.key))
        return false;
      if (
        event.ctrlKey &&
        event.shiftKey &&
        !event.altKey &&
        ["C", "V", "F", "T", "W", "K", "B"].includes(event.key.toUpperCase())
      ) {
        event.preventDefault();
        if (event.type === "keydown" && event.key.toUpperCase() === "C") {
          const selection = this.term.getSelection();
          if (selection)
            void copyText(selection).catch(() =>
              this.notify("复制失败，请检查剪贴板权限。"),
            );
        }
        if (event.type === "keydown" && event.key.toUpperCase() === "V") {
          event.preventDefault();
          void pasteText()
            .then((text) => this.paste(text))
            .catch(() => this.notify("读取剪贴板失败。"));
        }
        return false;
      }
      return true;
    });
  }

  mount(parent: HTMLElement) {
    parent.append(this.element);
    this.term.open(this.element);
    this.observer.observe(this.element);
  }

  private async paste(text: string) {
    if (!text || this.disposed || this.status !== "running") return;
    if (new TextEncoder().encode(text).length > INPUT_LIMIT) {
      this.notify("粘贴内容超过 1 MiB，请通过文件传输处理。");
      return;
    }
    if (
      /[\r\n]/.test(text) &&
      !(await this.confirmPaste(text.split(/\r\n|\r|\n/).length))
    )
      return;
    if (!this.disposed && this.status === "running") this.term.paste(text);
  }

  async start() {
    this.fitNow();
    if (this.preview) {
      this.status = "running";
      this.term.writeln("\x1b[34mHShell · 终端界面预览\x1b[0m");
      this.term.writeln(
        "\x1b[90m此终端仅在浏览器本地运行，未建立 SSH 连接。\x1b[0m",
      );
      this.term.writeln("");
      this.term.writeln(
        "可以输入 help 查看演示命令，体验输入、选择与终端搜索。",
      );
      this.term.writeln("实际连接请使用桌面客户端。");
      this.term.writeln("");
      this.previewPrompt();
      this.changed();
      this.term.focus();
      return;
    }
    const onEvent = new Channel<ArrayBuffer | TerminalEvent>();
    onEvent.onmessage = (event) => {
      if (this.disposed) return;
      if (event instanceof ArrayBuffer) {
        const bytes = new Uint8Array(event);
        // 解析完成后才确认消费，隐藏标签也必须持续处理输出。
        this.term.write(bytes, () => {
          if (!this.disposed)
            void invoke("ack_output", { id: this.id }).catch(() => {});
        });
      } else if (event.type === "auth") {
        void requestAuthentication(
          {
            name: this.profile.name,
            prompt: event.prompt,
            confirm: event.confirm,
          },
          this.authAbort.signal,
        ).then((answer) => {
          if (this.disposed || this.status === "exited") return;
          if (answer === null) {
            void invoke("close_session", { id: this.id });
            return;
          }
          return invoke("auth_respond", {
            id: this.id,
            requestId: event.request_id,
            answer,
          }).catch((error) => this.notify(String(error)));
        });
      } else {
        this.authAbort.abort();
        this.status = "exited";
        this.exitCode = event.code;
        this.queue = [];
        this.queuedBytes = 0;
        this.term.options.disableStdin = true;
        if (event.error) this.notify(event.error);
        this.changed();
      }
    };
    try {
      await invoke("start_session", {
        id: this.id,
        profile: this.profile,
        cols: this.term.cols,
        rows: this.term.rows,
        onEvent,
      });
      if (this.disposed) {
        await invoke("close_session", { id: this.id });
        return;
      }
      if (this.status === "starting") this.status = "running";
      this.sentSize = "";
      this.scheduleFit();
      this.changed();
      this.term.focus();
    } catch (error) {
      if (this.disposed) return;
      this.status = "exited";
      this.term.options.disableStdin = true;
      this.notify(String(error));
      this.changed();
    }
  }

  setVisible(visible: boolean) {
    if (this.visible === visible) return;
    this.visible = visible;
    this.element.hidden = !visible;
    if (!visible) {
      return;
    }
    this.scheduleFit();
    this.term.focus();
  }

  private scheduleFit() {
    if (this.frame || this.disposed || !this.visible) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.fitNow();
    });
  }

  private fitNow() {
    if (this.disposed || this.element.clientWidth < 20) return;
    this.fit.fit();
    const size = `${this.term.cols}:${this.term.rows}`;
    if (this.status === "running" && size !== this.sentSize) {
      this.sentSize = size;
      this.changed();
      if (this.preview) return;
      void invoke("resize_session", {
        id: this.id,
        cols: this.term.cols,
        rows: this.term.rows,
      }).catch(() => {});
    }
  }

  private enqueue(data: string) {
    if (this.status !== "running" || this.disposed) return;
    if (this.preview) {
      this.previewWrite(data);
      return;
    }
    const length = new TextEncoder().encode(data).length;
    if (this.queuedBytes + length > INPUT_LIMIT) {
      this.notify("待发送输入超过 1 MiB，已拒绝本次输入。");
      return;
    }
    this.queue.push(data);
    this.queuedBytes += length;
    void this.flush();
  }

  private previewPrompt() {
    this.term.write("\x1b[32mpreview\x1b[0m \x1b[34m~\x1b[0m $ ");
  }

  private previewWrite(data: string) {
    if (data.startsWith("\x1b")) return;
    for (const char of data) {
      if (char === "\r" || char === "\n") {
        this.term.write("\r\n");
        const line = this.previewInput.trim();
        this.previewInput = "";
        if (line === "clear") this.term.clear();
        else if (line === "help") {
          this.term.writeln("  help          查看演示命令");
          this.term.writeln("  echo <文本>   在本地回显文字");
          this.term.writeln("  clear         清空终端");
          this.term.writeln("  exit          结束演示");
          this.term.writeln(
            "\x1b[90m仅支持以上本地演示命令，不会执行系统命令。\x1b[0m",
          );
        } else if (line === "echo" || line.startsWith("echo "))
          this.term.writeln(line.slice(5).replace(/[\x00-\x1f\x7f]/g, ""));
        else if (line === "exit") {
          this.term.writeln("演示已结束。");
          this.status = "exited";
          this.term.options.disableStdin = true;
          this.changed();
          return;
        } else if (line)
          this.term.writeln("这是本地预览。输入 help 查看演示命令。");
        this.previewPrompt();
      } else if (char === "\x7f") {
        if (this.previewInput) {
          this.previewInput = Array.from(this.previewInput)
            .slice(0, -1)
            .join("");
          this.term.write("\b \b");
        }
      } else if (char === "\x03") {
        this.previewInput = "";
        this.term.write("^C\r\n");
        this.previewPrompt();
      } else if (char >= " " && this.previewInput.length < 4096) {
        this.previewInput += char;
        this.term.write(char);
      }
    }
  }

  private async flush() {
    if (this.writing) return;
    this.writing = true;
    try {
      while (this.queue.length && !this.disposed && this.status === "running") {
        const data = this.queue.shift()!;
        for (let offset = 0; offset < data.length; ) {
          let end = Math.min(offset + 8192, data.length);
          const last = data.charCodeAt(end - 1);
          if (end < data.length && last >= 0xd800 && last <= 0xdbff) end--;
          await invoke("write_session", {
            id: this.id,
            data: data.slice(offset, end),
          });
          offset = end;
          if (this.disposed || this.status !== "running") break;
        }
        this.queuedBytes = Math.max(
          0,
          this.queuedBytes - new TextEncoder().encode(data).length,
        );
      }
    } catch (error) {
      this.queue = [];
      this.queuedBytes = 0;
      if (!this.disposed) this.notify(`发送失败：${error}`);
    } finally {
      this.writing = false;
    }
  }

  find(value: string, previous = false): boolean {
    if (!value) {
      this.search.clearDecorations();
      return true;
    }
    return previous
      ? this.search.findPrevious(value)
      : this.search.findNext(value);
  }

  async dispose() {
    this.disposed = true;
    this.authAbort.abort();
    this.queue = [];
    this.observer.disconnect();
    this.stopFontSizeListener();
    cancelAnimationFrame(this.frame);
    this.term.dispose();
    this.element.remove();
    if (!this.preview) await invoke("close_session", { id: this.id });
  }
}
