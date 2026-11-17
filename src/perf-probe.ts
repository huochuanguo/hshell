// Compiled out of production. Runs the real TerminalView in native WebKit.
import { invoke } from "@tauri-apps/api/core";
import { TerminalView, loadTerminalFonts } from "./terminal";
import type { Profile } from "./types";
import { runCloseProbe, type CloseProbeHooks } from "./close-probe";

const pause = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
const frame = () => new Promise<number>((r) => requestAnimationFrame(r));
let phase = "startup";
let measuredView: TerminalView | undefined;
export async function runProbe(hooks: CloseProbeHooks) {
  try {
    const scenario = await invoke<string>("perf_scenario");
    if (scenario.startsWith("close-")) {
      await runCloseProbe(scenario, hooks);
      return;
    }
    await measure();
  } catch (error) {
    await invoke("perf_result", {
      result: JSON.stringify({
        error: String(error),
        phase,
        viewportY: measuredView?.term.buffer.active.viewportY,
        baseY: measuredView?.term.buffer.active.baseY,
      }),
    });
  }
}
async function measure() {
  await loadTerminalFonts();
  const sshMode = await invoke<boolean>("perf_mode");
  const pane = document.createElement("div");
  pane.style.cssText =
    "position:fixed;inset:50px 0 30px 220px;background:var(--canvas);z-index:20";
  document.body.append(pane);
  const profile: Profile = {
    id: "perf-probe",
    name: "probe",
    host: "localhost",
    username: "",
    port: null,
    identityFile: "",
    group: "",
    favorite: false,
  };
  const views = [0, 1].map(
    () =>
      new TerminalView(
        profile,
        () => {},
        () => {},
        async () => false,
        false,
      ),
  );
  for (const view of views) view.mount(pane);
  views[0].setVisible(true);
  await views[0].start();
  views[1].setVisible(false);
  await views[1].start();
  await pause(1500);
  const frames: number[] = [];
  const switches: number[] = [];
  let running = true;
  let last = await frame();
  const monitor = async () => {
    while (running) {
      const now = await frame();
      frames.push(now - last);
      last = now;
    }
  };
  void monitor();
  for (let i = 0; i < 30; i++) {
    const start = performance.now();
    views[0].setVisible(i % 2 === 0);
    views[1].setVisible(i % 2 !== 0);
    await frame();
    await frame();
    switches.push(performance.now() - start);
    await pause(30);
  }
  const view = views[1];
  measuredView = view;
  const waitFor = (marker: string) =>
    new Promise<void>((resolve, reject) => {
      // An OSC acknowledgement follows the actual echoed bytes. Unlike scanning
      // recent rows, this remains reliable when new output evicts those rows.
      const parsed = sshMode
        ? view.term.parser.registerOscHandler(777, (data) => {
            if (data === marker) {
              clearTimeout(timeout);
              sub.dispose();
              parsed?.dispose();
              resolve();
            }
            return true;
          })
        : undefined;
      const timeout = setTimeout(() => {
        sub.dispose();
        parsed?.dispose();
        reject(new Error("echo timeout"));
      }, 15000);
      const sub = view.term.onWriteParsed(() => {
        if (sshMode) return;
        const buffer = view.term.buffer.active;
        for (
          let i = buffer.length - 1;
          i >= Math.max(0, buffer.length - 80);
          i--
        ) {
          if (buffer.getLine(i)?.translateToString().includes(marker)) {
            clearTimeout(timeout);
            sub.dispose();
            resolve();
            break;
          }
        }
      });
    });
  const inputTimes: number[] = [];
  for (let i = 0; i < 40; i++) {
    const marker = `probe-echo-${i}`;
    const received = waitFor(marker);
    const start = performance.now();
    view.term.paste(marker + "\n");
    await received;
    inputTimes.push(performance.now() - start);
  }
  const output = "0123456789 abcdefghijklmnopqrstuvwxyz 中文输出\r\n".repeat(
    20000,
  );
  const begin = performance.now();
  const received = waitFor("probe-output-complete");
  view.term.paste(output.replace(/\r/g, "") + "probe-output-complete\n");
  await received;
  const outputMs = performance.now() - begin;
  const interaction: Record<string, number> = {};
  if (sshMode) {
    const percentile = (xs: number[]) =>
      [...xs].sort((a, b) => a - b)[Math.floor((xs.length - 1) * 0.95)];
    let ready = waitFor("probe-preload-done");
    view.term.paste("probe-preload\n");
    await ready;
    await frame();
    await frame();
    const characterEcho = async (offset: number) => {
      const times: number[] = [];
      for (let i = 0; i < 24; i++) {
        // Unique Unicode characters exclude matches from earlier output. No Enter.
        const char = String.fromCharCode(0xe100 + offset + i);
        const received = waitFor(char);
        const start = performance.now();
        view.term.paste(char);
        await received;
        times.push(performance.now() - start);
        await pause(8);
      }
      return percentile(times);
    };
    const wheelScroll = async () => {
      const times: number[] = [];
      const screen = view.element.querySelector(".xterm-screen")!;
      view.term.scrollToLine(1500);
      await frame();
      await frame();
      for (let i = 0; i < 48; i++) {
        // New logs can evict the whole scrollback while this phase runs. Keep
        // wheel samples away from boundaries where a no-op is intentional.
        const buffer = view.term.buffer.active;
        if (buffer.viewportY < 300 || buffer.viewportY > buffer.baseY - 300) {
          view.term.scrollToLine(Math.floor(buffer.baseY / 2));
          await frame();
        }
        const before = view.term.buffer.active.viewportY;
        const start = performance.now();
        await new Promise<void>((resolve, reject) => {
          let scrolled = false;
          const scroll = view.term.onScroll(() => {
            scrolled = true;
          });
          const timeout = setTimeout(() => {
            scroll.dispose();
            render.dispose();
            reject(
              new Error(
                `wheel did not render: before=${before}, now=${view.term.buffer.active.viewportY}`,
              ),
            );
          }, 2000);
          const render = view.term.onRender(() => {
            if (scrolled) {
              clearTimeout(timeout);
              scroll.dispose();
              render.dispose();
              resolve();
            }
          });
          const wheel = new WheelEvent("wheel", {
            deltaY: i % 2 ? 120 : -120,
            deltaMode: 0,
            bubbles: true,
            cancelable: true,
          });
          screen.dispatchEvent(wheel);
          if (!wheel.defaultPrevented) {
            clearTimeout(timeout);
            scroll.dispose();
            render.dispose();
            reject(new Error("wheel was not consumed by xterm"));
          }
        });
        await frame();
        times.push(performance.now() - start);
      }
      return percentile(times);
    };
    phase = "idle character echo";
    interaction.keyInputP95 = await characterEcho(0);
    phase = "idle wheel scrolling";
    interaction.scrollP95 = await wheelScroll();
    view.term.scrollToBottom();
    view.term.paste("\nprobe-load-on\n");
    await pause(300);
    phase = "wheel scrolling during output";
    interaction.loadedScrollP95 = await wheelScroll();
    view.term.scrollToBottom();
    phase = "character echo during output";
    interaction.loadedInputP95 = await characterEcho(32);
    ready = waitFor("probe-load-stopped");
    view.term.paste("\nprobe-load-off\n");
    await ready;
  }
  running = false;
  const percentile = (xs: number[], p: number) =>
    [...xs].sort((a, b) => a - b)[Math.floor((xs.length - 1) * p)];
  await invoke("perf_result", {
    result: JSON.stringify({
      ...interaction,
      inputP95: percentile(inputTimes, 0.95),
      switchP95: percentile(switches, 0.95),
      frameP95: percentile(frames, 0.95),
      maxFrame: Math.max(...frames),
      outputMs,
      outputBytes: new TextEncoder().encode(output).length,
      fontsLoaded:
        document.fonts.check('400 15px "HShell Mono"') &&
        document.fonts.check('600 15px "HShell Mono"'),
      fontFamily: view.term.options.fontFamily,
      fontSize: view.term.options.fontSize,
      lineHeight: view.term.options.lineHeight,
    }),
  });
}
