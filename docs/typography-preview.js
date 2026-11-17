import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

// Design sample only. This module is not imported by the desktop application.
const presets = [
  {
    id: "jetbrains",
    label: "A · JetBrains Mono",
    ui: "QZ Plex Sans",
    mono: "QZ JetBrains Mono",
    description:
      "推荐：IBM Plex Sans + 思源黑体界面，JetBrains Mono 终端。清楚区分数字、标点与英文命令。",
  },
  {
    id: "plex",
    label: "B · IBM Plex Mono",
    ui: "QZ Plex Sans",
    mono: "QZ Plex Mono",
    description:
      "IBM Plex Sans + 思源黑体界面，IBM Plex Mono 终端。字形克制，工具感更强。",
  },
  {
    id: "ubuntu",
    label: "C · Ubuntu Sans Mono",
    ui: "QZ Ubuntu Sans",
    mono: "QZ Ubuntu Mono",
    description:
      "Ubuntu Sans + 思源黑体界面，Ubuntu Sans Mono 终端。更贴近 Ubuntu 桌面的字体风格。",
  },
];
const app = document.querySelector("#app");
const controls = document.querySelector(".review-controls");
const header = document.querySelector(".review-header");
header.querySelector("strong").textContent = "栖栈 SSH · 字体与文字颜色";
header.querySelector("aside").innerHTML =
  "真实字体与 xterm 渲染样张<br>未建立 SSH 连接";
controls.innerHTML =
  presets
    .map(
      (p) =>
        `<button class="palette-button" data-font="${p.id}" aria-pressed="false">${p.label}</button>`,
    )
    .join("") +
  '<span class="preview-note" id="font-info" role="status">加载字体…</span>';
controls.setAttribute("aria-label", "终端字体方案");
const pane = document.querySelector("#terminal-panes");
pane.replaceChildren();
const element = document.createElement("div");
element.className = "font-terminal";
pane.append(element);
const theme = {
  background: "#17191D",
  foreground: "#D3D9E2",
  cursor: "#9ABCEB",
  selectionBackground: "#33445C",
  selectionForeground: "#E7EDF5",
  black: "#1E2126",
  brightBlack: "#A2AAB7",
  red: "#E69A9A",
  brightRed: "#F2AEAA",
  green: "#9CCAA0",
  brightGreen: "#B7D6AC",
  yellow: "#DDC28D",
  brightYellow: "#EED6A1",
  blue: "#9ABCEB",
  brightBlue: "#B5D0F1",
  magenta: "#C6A7D9",
  brightMagenta: "#DBBDE7",
  cyan: "#91C6CB",
  brightCyan: "#AFE0DB",
  white: "#D3D9E2",
  brightWhite: "#E7EDF5",
};
const term = new Terminal({
  fontSize: 15,
  lineHeight: 1.36,
  fontWeight: "400",
  fontWeightBold: "600",
  fontFamily: '"QZ JetBrains Mono", "Noto Sans Mono CJK SC", monospace',
  letterSpacing: 0,
  cursorStyle: "bar",
  cursorBlink: false,
  disableStdin: true,
  scrollback: 300,
  allowTransparency: false,
  theme,
});
const fit = new FitAddon();
term.loadAddon(fit);
term.open(element);
let resizeFrame = 0;
new ResizeObserver(() => {
  cancelAnimationFrame(resizeFrame);
  resizeFrame = requestAnimationFrame(() => fit.fit());
}).observe(element);
const esc = "\x1b[";
const color = (code, text) => `${esc}${code}m${text}${esc}0m`;
const prompt =
  color("32", "deploy@prod-api-01") +
  ":" +
  color("34", "~/services/api") +
  "$ ";
const pad = (text, width) =>
  text +
  " ".repeat(
    Math.max(
      0,
      width -
        [...text].reduce((n, c) => n + (/[^\x00-\x7f]/.test(c) ? 2 : 1), 0),
    ),
  );
const lines = [
  color("90", "Ubuntu 24.04.3 LTS · 字体、中文与日志颜色样张"),
  "",
  prompt + 'printf "font-check\\n"',
  "字形辨识  0 O o   1 l I |   ` ' \"   { } [ ] ( )   / \\ _ -",
  "符号辨识  !=  ==  =>  ->  <=  >=  ::  &&  ||   fi fl ffi",
  "",
  prompt + "ls -lh config/",
  pad("名称", 25) + pad("权限", 14) + pad("大小", 8) + "修改时间",
  pad("app.toml", 25) + pad("-rw-r--r--", 14) + pad("1.6K", 8) + "Nov 16 09:42",
  pad("数据库配置.yaml", 25) +
    pad("-rw-------", 14) +
    pad("2.3K", 8) +
    "Nov 16 09:40",
  pad("日志归档/", 25) +
    pad("drwxr-xr-x", 14) +
    pad("4.0K", 8) +
    "Nov 16 08:16",
  "",
  prompt + "tail -n 4 logs/server.log",
  color("90", "09:42:18") +
    " " +
    color("36", "INFO ") +
    " 服务已启动，监听 " +
    color("34", "0.0.0.0:8080"),
  color("90", "09:42:19") +
    " " +
    color("32", "INFO ") +
    " GET /health " +
    color("32", "200") +
    " " +
    color("90", "3.8ms"),
  color("90", "09:42:22") +
    " " +
    color("33", "WARN ") +
    " 缓存响应较慢，已回退至数据库",
  color("90", "09:42:24") +
    " " +
    color("31", "ERROR") +
    " 上游连接超时 " +
    color("90", "retry=1/3"),
  "",
  "文字层次  " +
    color("1", "重点 / 600") +
    "  正文 / 400  " +
    color("90", "时间戳与辅助信息"),
  "终端颜色  " +
    ["31", "32", "33", "34", "35", "36", "37"]
      .map((c, i) =>
        color(c, ["红色", "绿色", "黄色", "蓝色", "紫色", "青色", "灰白"][i]),
      )
      .join("  "),
  "",
  prompt,
];
let generation = 0;
async function applyFont(id) {
  const preset = presets.find((p) => p.id === id) || presets[0];
  const current = ++generation;
  document.querySelector("#font-info").textContent = "加载字体…";
  const loadedFonts = await Promise.all([
    document.fonts.load(`400 15px "${preset.mono}"`, "O0Il1"),
    document.fonts.load(`600 15px "${preset.mono}"`, "O0Il1"),
    document.fonts.load(`400 13px "${preset.ui}"`, "SSH"),
    document.fonts.load(`500 13px "${preset.ui}"`, "SSH"),
  ]);
  if (current !== generation) return;
  if (
    loadedFonts.some(
      (faces) =>
        faces.length === 0 || faces.some((face) => face.status !== "loaded"),
    )
  )
    throw new Error("实际字体文件未加载");
  if (!document.fonts.check(`400 15px "${preset.mono}"`, "O0Il1"))
    throw new Error("字体加载失败");
  app.style.setProperty(
    "--font-ui",
    `"${preset.ui}", "Noto Sans CJK SC", "Microsoft YaHei", sans-serif`,
  );
  app.style.setProperty(
    "--mono",
    `"${preset.mono}", "Noto Sans Mono CJK SC", monospace`,
  );
  term.options.fontFamily = `"${preset.mono}", "Noto Sans Mono CJK SC", monospace`;
  await new Promise(requestAnimationFrame);
  fit.fit();
  term.reset();
  await new Promise((resolve) => term.write(lines.join("\r\n"), resolve));
  document
    .querySelectorAll("[data-font]")
    .forEach((button) =>
      button.setAttribute(
        "aria-pressed",
        String(button.dataset.font === preset.id),
      ),
    );
  document.querySelector("#palette-description").textContent =
    preset.description;
  document.querySelector("#font-info").textContent =
    "界面 13 px · 终端 15 px / 400 · 行高 1.36";
  app.dataset.fontReady = "true";
  app.dataset.terminalFont = preset.mono;
  document.documentElement.dataset.font = preset.id;
  history.replaceState(null, "", `?font=${preset.id}#graphite`);
}
controls.querySelectorAll("[data-font]").forEach(
  (button) =>
    (button.onclick = () => {
      void applyFont(button.dataset.font).catch((error) => {
        document.querySelector("#font-info").textContent = String(error);
      });
    }),
);
void applyFont(
  new URLSearchParams(location.search).get("font") || "jetbrains",
).catch((error) => {
  document.querySelector("#font-info").textContent = String(error);
});
