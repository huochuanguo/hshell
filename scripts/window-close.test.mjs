import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { mockIPC, mockWindows, clearMocks } from '@tauri-apps/api/mocks';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { createCloseHandler } from '../src/window-close.ts';

const deferred = () => {
  let resolve;
  const promise = new Promise(r => { resolve = r; });
  return { promise, resolve };
};
function fixture(overrides = {}) {
  const calls = { prevented: 0, confirmed: 0, disposed: 0, destroyed: 0, errors: [] };
  const options = {
    sessions: () => [],
    confirm: async () => { calls.confirmed++; return true; },
    destroy: async () => { calls.destroyed++; },
    reportError: message => calls.errors.push(message),
    ...overrides,
  };
  const handler = createCloseHandler(options);
  return { calls, options, close: () => handler({ preventDefault() { calls.prevented++; } }) };
}
const session = (dispose, extra = {}) => ({ status: 'running', preview: false, dispose, ...extra });

test('只有后台文件传输时也确认退出并按顺序保存断点', async () => {
  const order = [];
  const { close } = fixture({
    hasBackgroundWork: async () => true,
    confirm: async () => { order.push('确认'); return true; },
    cleanup: async () => { order.push('保存断点'); },
    destroy: async () => { order.push('销毁窗口'); },
  });
  await close();
  assert.deepEqual(order, ['确认', '保存断点', '销毁窗口']);
});

test('取消退出时后台传输继续运行', async () => {
  let stopped = false;
  const { close, calls } = fixture({ hasBackgroundWork: async () => true, confirm: async () => false, cleanup: async () => { stopped = true; } });
  await close();
  assert.equal(stopped, false);
  assert.equal(calls.destroyed, 0);
});

// 使用已安装的 Tauri 包和真实权限配置，明确要求窗口具备销毁权限。
test('Tauri close-request listener reaches permitted native destruction', async () => {
  globalThis.window = { crypto: globalThis.crypto };
  mockWindows('main');
  try {
    const capability = JSON.parse(await readFile(new URL('../src-tauri/capabilities/default.json', import.meta.url)));
    assert.deepEqual(capability.windows, ['main']);
    const permitted = action => capability.permissions.includes(`core:window:allow-${action}`);
    let listener;
    let destroyed = 0;
    mockIPC(async (command, args) => {
      if (command === 'plugin:event|listen') { listener = args.handler; return listener; }
      if (command === 'plugin:window|destroy') {
        if (!permitted('destroy')) throw new Error('window.destroy not allowed');
        destroyed++;
      }
      assert.notEqual(command, 'plugin:window|close', 'must not recursively request close');
    });
    const { calls, options } = fixture();
    const appWindow = getCurrentWindow();
    await appWindow.onCloseRequested(createCloseHandler({ ...options, destroy: () => appWindow.destroy() }));
    await window.__TAURI_INTERNALS__.callbacks.get(listener)({ event: 'tauri://close-requested', payload: null, id: listener });
    assert.deepEqual(calls.errors, [], 'native capability must permit destruction');
    assert.equal(destroyed, 1, 'window must be destroyed exactly once');
  } finally { clearMocks(); }
});

test('running sessions are confirmed and fully disposed before destruction', async () => {
  const cleanup = deferred();
  const f = fixture({ sessions: () => [session(() => { f.calls.disposed++; return cleanup.promise; })] });
  const closing = f.close();
  await Promise.resolve();
  assert.equal(f.calls.confirmed, 1);
  assert.equal(f.calls.disposed, 1);
  assert.equal(f.calls.destroyed, 0);
  cleanup.resolve();
  await closing;
  assert.equal(f.calls.destroyed, 1);
});

test('cancel keeps sessions alive and permits the next close request', async () => {
  let accepted = false;
  const f = fixture({
    sessions: () => [session(async () => { f.calls.disposed++; })],
    confirm: async () => { f.calls.confirmed++; return accepted; },
  });
  await f.close();
  assert.equal(f.calls.disposed, 0);
  assert.equal(f.calls.destroyed, 0);
  accepted = true;
  await f.close();
  assert.equal(f.calls.confirmed, 2);
  assert.equal(f.calls.disposed, 1);
  assert.equal(f.calls.destroyed, 1);
});

test('repeated requests cannot bypass pending confirmation or cleanup', async () => {
  const confirmation = deferred();
  const cleanup = deferred();
  const f = fixture({
    sessions: () => [session(() => { f.calls.disposed++; return cleanup.promise; })],
    confirm: () => { f.calls.confirmed++; return confirmation.promise; },
  });
  const closing = f.close();
  await f.close();
  assert.equal(f.calls.confirmed, 1);
  assert.equal(f.calls.disposed, 0);
  assert.equal(f.calls.destroyed, 0);
  confirmation.resolve(true);
  await Promise.resolve();
  await f.close();
  assert.equal(f.calls.disposed, 1);
  assert.equal(f.calls.destroyed, 0);
  cleanup.resolve();
  await closing;
  await f.close();
  assert.equal(f.calls.destroyed, 1);
  assert.equal(f.calls.prevented, 4);
});

test('a native destruction failure is visible and can be retried', async () => {
  let rejected = true;
  const f = fixture({ destroy: async () => { if (rejected) throw new Error('native failure'); f.calls.destroyed++; } });
  await f.close();
  assert.match(f.calls.errors[0], /native failure/);
  rejected = false;
  await f.close();
  assert.equal(f.calls.destroyed, 1);
});

test('a disposal failure still reaches native destruction and its cleanup fallback', async () => {
  const f = fixture({ sessions: () => [session(async () => { throw new Error('terminal failure'); })] });
  await f.close();
  assert.equal(f.calls.destroyed, 1);
});

test('exited sessions and browser previews need no disconnect confirmation', async () => {
  const f = fixture({ sessions: () => [
    session(async () => {}, { status: 'exited' }),
    session(async () => {}, { preview: true }),
  ] });
  await f.close();
  assert.equal(f.calls.confirmed, 0);
  assert.equal(f.calls.destroyed, 1);
});
