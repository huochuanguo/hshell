import assert from "node:assert/strict";
import { test } from "node:test";

let instance = 0;
const loadPreference = () =>
  import(`../src/terminal-font.ts?test=${instance++}`);

function storageFixture(t, stored = null, unavailable = false) {
  const descriptor = Object.getOwnPropertyDescriptor(
    globalThis,
    "localStorage",
  );
  const values = new Map(
    stored === null ? [] : [["hshell.terminalFontSize", stored]],
  );
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem(key) {
        if (unavailable) throw new Error("存储不可用");
        return values.get(key) ?? null;
      },
      setItem(key, value) {
        if (unavailable) throw new Error("存储不可用");
        values.set(key, value);
      },
    },
  });
  t.after(() => {
    if (descriptor)
      Object.defineProperty(globalThis, "localStorage", descriptor);
    else delete globalThis.localStorage;
  });
  return values;
}

test("终端字号立即通知现有会话，并在下次加载时恢复", async (t) => {
  const values = storageFixture(t, "18");
  const preference = await loadPreference();
  assert.equal(preference.getTerminalFontSize(), 18);
  const sizes = [];
  const stop = preference.onTerminalFontSizeChange((size) => sizes.push(size));
  preference.setTerminalFontSize(20);
  preference.setTerminalFontSize(20);
  assert.deepEqual(sizes, [20]);
  assert.equal(values.get("hshell.terminalFontSize"), "20");
  assert.equal((await loadPreference()).getTerminalFontSize(), 20);
  stop();
  preference.setTerminalFontSize(preference.DEFAULT_TERMINAL_FONT_SIZE);
  assert.deepEqual(sizes, [20]);
  assert.equal((await loadPreference()).getTerminalFontSize(), 15);
});

test("损坏、空白、非整数及越界字号回到默认值", async (t) => {
  const values = storageFixture(t);
  for (const stored of [
    null,
    "",
    " ",
    "abc",
    "NaN",
    "Infinity",
    "15.5",
    "9",
    "33",
  ]) {
    if (stored === null) values.delete("hshell.terminalFontSize");
    else values.set("hshell.terminalFontSize", stored);
    assert.equal(
      (await loadPreference()).getTerminalFontSize(),
      15,
      String(stored),
    );
  }
});

test("字号限制在可用范围内，拒绝非有限数值", async (t) => {
  storageFixture(t);
  const preference = await loadPreference();
  preference.setTerminalFontSize(100);
  assert.equal(preference.getTerminalFontSize(), 32);
  for (const size of [NaN, Infinity, -Infinity])
    preference.setTerminalFontSize(size);
  assert.equal(preference.getTerminalFontSize(), 32);
  preference.setTerminalFontSize(-10);
  assert.equal(preference.getTerminalFontSize(), 10);
});

test("存储被禁用时仍可调整当前字号并通知会话", async (t) => {
  storageFixture(t, null, true);
  const preference = await loadPreference();
  assert.equal(preference.getTerminalFontSize(), 15);
  const sizes = [];
  preference.onTerminalFontSizeChange((size) => sizes.push(size));
  assert.doesNotThrow(() => preference.setTerminalFontSize(19));
  assert.equal(preference.getTerminalFontSize(), 19);
  assert.deepEqual(sizes, [19]);
});

test("字号快捷键支持主键盘和小键盘，保留远端控制键及输入法组合", async () => {
  const { terminalFontShortcut } = await loadPreference();
  const key = (value, modifiers = {}) =>
    terminalFontShortcut({
      key: value,
      ctrlKey: true,
      shiftKey: false,
      altKey: false,
      metaKey: false,
      isComposing: false,
      ...modifiers,
    });
  assert.equal(key("="), "increase");
  assert.equal(key("+", { shiftKey: true }), "increase");
  assert.equal(key("+"), "increase");
  assert.equal(key("-"), "decrease");
  assert.equal(key("0"), "reset");
  for (const value of ["b", "k", "_", ")", "c", "v"])
    assert.equal(key(value), null);
  for (const modifiers of [
    { ctrlKey: false },
    { altKey: true },
    { metaKey: true },
    { isComposing: true },
  ])
    assert.equal(key("=", modifiers), null);
  assert.equal(key("-", { shiftKey: true }), null);
  assert.equal(key("0", { shiftKey: true }), null);
});
