import assert from 'node:assert/strict';
import { test } from 'node:test';
import { remoteJoin, remoteParent, localBasename, downloadName } from '../src/paths.ts';

test('远程 Unix 路径保留正斜杠、冒号、中文及文件名中的反斜杠', () => {
  assert.equal(remoteJoin('/home/用户', 'a:b\\c.txt'), '/home/用户/a:b\\c.txt');
  assert.equal(remoteJoin('/', '中文 文件'), '/中文 文件');
  assert.equal(remoteParent('/home/用户/'), '/home');
  assert.equal(remoteParent('/home'), '/');
  assert.equal(remoteParent('/'), '/');
  for (const name of ['', '.', '..', '../file', 'a/b', 'a\0b']) assert.throws(() => remoteJoin('/home', name));
  assert.throws(() => remoteJoin('C:\\Users', 'file'));
});

test('本地 Windows 路径和下载默认名称独立处理', () => {
  assert.equal(localBasename('C:\\Users\\用户\\私有 文件.txt'), '私有 文件.txt');
  assert.equal(localBasename('/tmp/文件.txt'), '文件.txt');
  assert.equal(downloadName('a:b\\c?.txt'), 'a_b_c_.txt');
  assert.equal(downloadName('CON.txt'), '_CON.txt');
  assert.equal(downloadName('name. '), 'name');
  assert.equal(downloadName('...'), '_download');
});
