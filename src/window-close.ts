interface CloseEvent {
  preventDefault(): void;
}

interface Session {
  status: string;
  preview: boolean;
  dispose(): Promise<void>;
}

interface CloseOptions {
  sessions(): Iterable<Session>;
  confirm(): Promise<boolean>;
  destroy(): Promise<void>;
  reportError(message: string): void;
}

export function createCloseHandler(options: CloseOptions) {
  let busy = false;
  return async (event: CloseEvent) => {
    // Tauri's listener otherwise calls destroy implicitly. Own this sequence
    // explicitly so repeated requests cannot bypass confirmation or cleanup.
    event.preventDefault();
    if (busy) return;
    busy = true;
    let destroyed = false;
    try {
      const sessions = [...options.sessions()];
      if (
        sessions.some((view) => view.status !== "exited" && !view.preview) &&
        !(await options.confirm())
      )
        return;
      // Native WindowEvent::Destroyed also closes every managed SSH process.
      // An individual terminal's disposal error must not trap the whole app.
      await Promise.allSettled(sessions.map((view) => view.dispose()));
      await options.destroy();
      destroyed = true;
    } catch (error) {
      options.reportError(`退出失败：${String(error)}。请重试关闭窗口。`);
    } finally {
      if (!destroyed) busy = false;
    }
  };
}
