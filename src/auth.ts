export interface AuthPrompt {
  name: string;
  prompt: string;
  confirm: boolean;
}
// OpenSSH prompts may arrive from several tabs; present them in order.
let tail: Promise<unknown> = Promise.resolve();
export function requestAuthentication(
  request: AuthPrompt,
  signal?: AbortSignal,
): Promise<string | null> {
  const result = tail.then(
    () =>
      new Promise<string | null>((resolve) => {
        if (signal?.aborted) {
          resolve(null);
          return;
        }
        const dialog = document.createElement("dialog");
        dialog.className = "auth-dialog";
        const heading = document.createElement("h2");
        heading.textContent = request.confirm
          ? "确认 SSH 主机指纹"
          : "SSH 身份验证";
        const name = document.createElement("p");
        name.className = "dialog-description";
        name.textContent = request.name;
        const prompt = document.createElement("pre");
        prompt.className = "auth-prompt";
        prompt.textContent = request.prompt;
        const form = document.createElement("form");
        const input = document.createElement("input");
        input.type = "password";
        input.autocomplete = "off";
        input.maxLength = 4096;
        input.setAttribute("aria-label", "密码或密钥口令");
        input.placeholder = "请输入密码或密钥口令";
        input.hidden = request.confirm;
        const note = document.createElement("p");
        note.className = "dialog-description";
        note.textContent = request.confirm
          ? "请先通过可信渠道核对指纹，再决定是否信任。"
          : "请按提示输入密码或密钥口令；跳板认证需手动输入。本次输入不会覆盖已保存密码。";
        const actions = document.createElement("div");
        actions.className = "dialog-actions";
        const cancel = document.createElement("button");
        cancel.type = "button";
        cancel.className = "secondary";
        cancel.textContent = "取消连接";
        const ok = document.createElement("button");
        ok.type = "submit";
        ok.className = "primary";
        ok.textContent = request.confirm ? "信任并继续" : "继续";
        const abort = () => finish(null);
        const finish = (value: string | null) => {
          signal?.removeEventListener("abort", abort);
          input.value = "";
          dialog.close();
          dialog.remove();
          resolve(value);
        };
        cancel.onclick = () => finish(null);
        dialog.oncancel = (event) => {
          event.preventDefault();
          finish(null);
        };
        form.onsubmit = (event) => {
          event.preventDefault();
          finish(request.confirm ? "yes" : input.value);
        };
        actions.append(cancel, ok);
        form.append(input, note, actions);
        dialog.append(heading, name, prompt, form);
        document.body.append(dialog);
        signal?.addEventListener("abort", abort, { once: true });
        dialog.showModal();
        if (request.confirm) cancel.focus();
        else input.focus();
      }),
  );
  tail = result.catch(() => {});
  return result;
}
