import { invoke } from "@tauri-apps/api/core";

interface Snippet {
  id: string;
  name: string;
  group: string;
  command: string;
}

export function bindSnippetLibrary(
  button: HTMLButtonElement,
  notify: (message: string) => void,
  confirm: (title: string, message: string, action: string) => Promise<boolean>,
  insert: (command: string) => boolean,
) {
  const dialog = document.createElement("dialog");
  dialog.className = "snippet-dialog";
  dialog.setAttribute("aria-labelledby", "snippet-title");
  dialog.innerHTML = `
    <div class="dialog-heading"><h2 id="snippet-title">命令片段</h2><button type="button" class="secondary" id="snippet-close">关闭</button></div>
    <form id="snippet-library">
      <label>搜索片段<input id="snippet-search" type="search" autocomplete="off" placeholder="名称、分组或命令"></label>
      <select id="snippet-list" size="6" aria-label="命令片段"></select>
      <p id="snippet-preview" class="snippet-preview" aria-live="polite"></p>
      <div class="dialog-actions"><button type="button" class="secondary" id="snippet-new">新建</button><button type="button" class="secondary" id="snippet-edit">编辑</button><button type="button" class="secondary" id="snippet-delete">删除</button><span></span><button type="submit" class="primary" id="snippet-insert">插入到终端</button></div>
    </form>
    <form id="snippet-editor" hidden>
      <label>名称<input id="snippet-name" maxlength="80" required autocomplete="off"></label>
      <label>分组（可留空）<input id="snippet-group" maxlength="40" autocomplete="off"></label>
      <label>命令<textarea id="snippet-command" rows="3" maxlength="4096" required spellcheck="false" aria-describedby="snippet-hint"></textarea><small id="snippet-hint">仅保存单行命令。插入后由你按 Enter 执行。</small></label>
      <div class="dialog-actions"><span></span><button type="button" class="secondary" id="snippet-back">返回</button><button type="submit" class="primary" id="snippet-save">保存片段</button></div>
    </form>
    <p class="form-error" role="alert"></p>`;
  document.body.append(dialog);
  const el = <T extends HTMLElement>(id: string) =>
    dialog.querySelector<T>(id)!;
  const library = el<HTMLFormElement>("#snippet-library");
  const editor = el<HTMLFormElement>("#snippet-editor");
  const search = el<HTMLInputElement>("#snippet-search");
  const list = el<HTMLSelectElement>("#snippet-list");
  const name = el<HTMLInputElement>("#snippet-name");
  const group = el<HTMLInputElement>("#snippet-group");
  const command = el<HTMLTextAreaElement>("#snippet-command");
  const error = el(".form-error");
  const create = el<HTMLButtonElement>("#snippet-new");
  const edit = el<HTMLButtonElement>("#snippet-edit");
  const remove = el<HTMLButtonElement>("#snippet-delete");
  const add = el<HTMLButtonElement>("#snippet-insert");
  const save = el<HTMLButtonElement>("#snippet-save");
  const back = el<HTMLButtonElement>("#snippet-back");
  const close = el<HTMLButtonElement>("#snippet-close");
  let snippets: Snippet[] = [];
  let editing: string | undefined;
  let busy = false;
  let loaded = false;

  const current = () => snippets.find(({ id }) => id === list.value);
  function sync() {
    const selected = current();
    el("#snippet-preview").textContent =
      selected?.command ??
      (loaded ? (snippets.length ? "没有匹配的命令片段" : "暂无命令片段") : "");
    create.disabled = busy || !loaded;
    edit.disabled = remove.disabled = add.disabled = busy || !selected;
    search.disabled = list.disabled = busy || !loaded;
    save.disabled = back.disabled = close.disabled = busy;
  }
  function render(selection = list.value) {
    const query = search.value.trim().toLowerCase();
    list.replaceChildren();
    const results = snippets
      .filter((item) =>
        [item.name, item.group, item.command].some((part) =>
          part.toLowerCase().includes(query),
        ),
      )
      .sort(
        (a, b) =>
          a.group.localeCompare(b.group, "zh-CN") ||
          a.name.localeCompare(b.name, "zh-CN"),
      );
    let groupNode: HTMLOptGroupElement | undefined;
    let previous: string | undefined;
    for (const item of results) {
      if (!groupNode || item.group !== previous) {
        groupNode = document.createElement("optgroup");
        groupNode.label = item.group || "未分组";
        list.append(groupNode);
        previous = item.group;
      }
      groupNode.append(new Option(item.name, item.id));
    }
    list.value = results.some(({ id }) => id === selection)
      ? selection
      : (results[0]?.id ?? "");
    sync();
  }
  function showEditor(item?: Snippet) {
    editing = item?.id;
    name.value = item?.name ?? "";
    group.value = item?.group ?? "";
    command.value = item?.command ?? "";
    error.textContent = "";
    library.hidden = true;
    editor.hidden = false;
    name.focus();
  }
  function showLibrary(id?: string) {
    editor.hidden = true;
    library.hidden = false;
    error.textContent = "";
    render(id);
    search.focus();
  }
  async function persist(next: Snippet[], id?: string) {
    busy = true;
    sync();
    try {
      await invoke("store_snippets", { snippets: next });
      snippets = next;
      search.value = "";
      showLibrary(id);
    } catch (failure) {
      error.textContent = String(failure);
    } finally {
      busy = false;
      sync();
      if (editor.hidden) search.focus();
    }
  }
  button.onclick = async () => {
    if (dialog.open) return;
    loaded = false;
    busy = true;
    snippets = [];
    search.value = "";
    showLibrary();
    dialog.showModal();
    try {
      snippets = await invoke<Snippet[]>("get_snippets");
      loaded = true;
    } catch (failure) {
      error.textContent = String(failure);
    } finally {
      busy = false;
      render();
      (loaded ? search : close).focus();
    }
  };
  close.onclick = () => dialog.close();
  dialog.oncancel = (event) => {
    if (busy) event.preventDefault();
  };
  search.oninput = () => render();
  list.onchange = sync;
  create.onclick = () => showEditor();
  edit.onclick = () => showEditor(current());
  back.onclick = () => showLibrary();
  remove.onclick = async () => {
    const selected = current();
    if (selected && (await confirm("删除命令片段？", selected.name, "删除")))
      await persist(snippets.filter(({ id }) => id !== selected.id));
  };
  library.onsubmit = (event) => {
    event.preventDefault();
    const selected = current();
    if (!selected || busy || event.submitter !== add) return;
    if (!insert(selected.command)) {
      error.textContent = "请先打开一个运行中的 SSH 会话。";
      return;
    }
    dialog.close();
    notify("命令已插入终端，请确认后按 Enter。");
  };
  editor.onsubmit = (event) => {
    event.preventDefault();
    if (busy) return;
    if (
      !command.value.trim() ||
      /[\x00-\x1f\x7f-\x9f]/.test(command.value) ||
      new TextEncoder().encode(command.value).length > 4096
    ) {
      error.textContent = "命令须为单行文本，不能含控制字符，最多 4096 字节。";
      return;
    }
    const item = {
      id: editing ?? crypto.randomUUID(),
      name: name.value.trim(),
      group: group.value.trim(),
      command: command.value,
    };
    void persist(
      editing
        ? snippets.map((entry) => (entry.id === editing ? item : entry))
        : [...snippets, item],
      item.id,
    );
  };
}
