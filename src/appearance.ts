import type { ITheme } from "@xterm/xterm";

let fontsReady: Promise<void> | undefined;

export function loadTerminalFonts(): Promise<void> {
  // Measure the grid only after the bundled faces are ready. A missing font
  // still leaves the system monospace fallback available for SSH connections.
  return (fontsReady ??= Promise.all([
    document.fonts.load('400 15px "HShell Mono"'),
    document.fonts.load('600 15px "HShell Mono"'),
  ])
    .then(() => {})
    .catch(() => {}));
}

export function terminalAppearance() {
  const style = getComputedStyle(document.documentElement);
  const color = (name: string) => style.getPropertyValue(`--${name}`).trim();
  const theme: ITheme = {
    background: color("canvas"),
    foreground: color("terminal-foreground"),
    cursor: color("accent"),
    selectionBackground: color("terminal-selection"),
    selectionForeground: color("text-bright"),
    black: color("ansi-black"),
    brightBlack: color("ansi-bright-black"),
    red: color("ansi-red"),
    brightRed: color("ansi-bright-red"),
    green: color("ansi-green"),
    brightGreen: color("ansi-bright-green"),
    yellow: color("ansi-yellow"),
    brightYellow: color("ansi-bright-yellow"),
    blue: color("ansi-blue"),
    brightBlue: color("ansi-bright-blue"),
    magenta: color("ansi-magenta"),
    brightMagenta: color("ansi-bright-magenta"),
    cyan: color("ansi-cyan"),
    brightCyan: color("ansi-bright-cyan"),
    white: color("ansi-white"),
    brightWhite: color("ansi-bright-white"),
  };
  return {
    fontFamily: color("mono"),
    fontSize: 15,
    fontWeight: "400" as const,
    fontWeightBold: "600" as const,
    lineHeight: 1.36,
    theme,
  };
}
