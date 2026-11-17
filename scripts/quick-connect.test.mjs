import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseQuickConnection } from '../src/connection.ts';

test('quick connection preserves SSH alias defaults without saving a password', () => {
  const profile = parseQuickConnection(' staging ', '');
  assert.equal(profile.host, 'staging');
  assert.equal(profile.username, '');
  assert.equal(profile.port, null);
  assert.equal('password' in profile, false);
});

test('quick connection separates user, IPv6 address and explicit port', () => {
  const profile = parseQuickConnection('deploy@[2001:db8::1]', '2222');
  assert.equal(profile.host, '2001:db8::1');
  assert.equal(profile.username, 'deploy');
  assert.equal(profile.port, 2222);
});

test('quick connection rejects option injection and malformed destinations', () => {
  for (const destination of ['', '-oProxyCommand=anything', 'user@@host', '@host', '-user@host', 'user name@host', 'host;command', 'host\ncommand', 'host:2222']) {
    assert.throws(() => parseQuickConnection(destination, ''), destination);
  }
});

test('quick connection rejects ports outside the SSH port range', () => {
  for (const port of ['0', '-1', '65536', '22.5', 'NaN']) {
    assert.throws(() => parseQuickConnection('host', port), port);
  }
});
