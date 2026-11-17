// Included only by the development perf-probe build, never by a release package.
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { TerminalView } from "./terminal";
import type { Profile } from "./types";

export interface CloseProbeHooks {
  connect(profile: Profile): Promise<void>;
  sessions(): TerminalView[];
}
const pause = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
async function until(check: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!check()) {
    if (performance.now() > deadline) throw new Error(message);
    await pause(20);
  }
}
function click(selector: string) {
  const button = document.querySelector<HTMLButtonElement>(selector);
  if (!button) throw new Error(`missing button: ${selector}`);
  button.click();
}
export async function runCloseProbe(scenario: string, hooks: CloseProbeHooks) {
  const appWindow = getCurrentWindow();
  const mark = (phase: string) =>
    invoke<number>("perf_checkpoint", {
      result: JSON.stringify({ scenario, phase }),
    });
  const expectedSessions = scenario === "close-empty" ? 0 : 2;
  for (let i = 0; i < expectedSessions; i++) {
    await hooks.connect({
      id: "perf-probe",
      name: "close fixture",
      host: "localhost",
      username: "",
      port: null,
      identityFile: "",
      group: "",
      favorite: false,
    });
  }
  await until(
    () =>
      hooks.sessions().every((view) => {
        const buffer = view.term.buffer.active;
        return Array.from({ length: buffer.length }, (_, i) =>
          buffer.getLine(i)?.translateToString(),
        ).some((line) => line?.includes("fixture-ready"));
      }),
    "SSH fixture never became ready",
  );
  if ((await mark("ready")) !== expectedSessions)
    throw new Error("wrong native session count");
  if (scenario === "close-editor") {
    document.querySelector<HTMLDialogElement>("#profile-dialog")!.showModal();
  }
  if (scenario === "close-info") {
    document.querySelector<HTMLDialogElement>("#info-dialog")!.showModal();
  }
  const confirmed = () =>
    document.querySelector<HTMLDialogElement>("#confirm-dialog")!.open;
  await appWindow.close();
  if (expectedSessions) {
    await until(confirmed, "no exit confirmation");
    if (scenario === "close-repeated") {
      for (let i = 0; i < 5; i++) await appWindow.close();
      await pause(150);
      if (!confirmed() || (await mark("repeated")) !== expectedSessions)
        throw new Error("repeated close bypassed confirmation");
    }
    if (scenario === "close-cancel-retry" || scenario === "close-repeated") {
      click("#confirm-cancel");
      await pause(150);
      if (
        confirmed() ||
        (await mark("cancelled")) !== expectedSessions ||
        hooks.sessions().some((view) => view.status !== "running")
      )
        throw new Error("cancel disconnected sessions");
      await appWindow.close();
      await until(confirmed, "cannot close again after cancelling");
    }
    await mark("accepted");
    click("#confirm-ok");
  }
  // Success is determined by native WindowEvent::Destroyed + process exit.
  // A timer exits only to return a diagnostic failure if the window stays alive.
  setTimeout(() => {
    void invoke("perf_result", {
      result: JSON.stringify({
        scenario,
        error: "window remained alive after close",
      }),
    });
  }, 3000);
}
