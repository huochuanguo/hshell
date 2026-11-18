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
  hasBackgroundWork?(): Promise<boolean>;
  cleanup?(): Promise<void>;
  destroy(): Promise<void>;
  reportError(message: string): void;
}

export function createCloseHandler(options: CloseOptions) {
  let busy = false;
  return async (event: CloseEvent) => {
    // 统一管理确认和清理，重复关闭请求不能绕过正在进行的退出流程。
    event.preventDefault();
    if (busy) return;
    busy = true;
    let destroyed = false;
    try {
      const sessions = [...options.sessions()];
      if (
        (sessions.some((view) => view.status !== "exited" && !view.preview) ||
          (await options.hasBackgroundWork?.())) &&
        !(await options.confirm())
      )
        return;
      // 原生销毁回调也会清理进程；单个终端清理失败不能阻止整个应用退出。
      await Promise.allSettled(sessions.map((view) => view.dispose()));
      await options.cleanup?.();
      await options.destroy();
      destroyed = true;
    } catch (error) {
      options.reportError(`退出失败：${String(error)}。请重试关闭窗口。`);
    } finally {
      if (!destroyed) busy = false;
    }
  };
}
