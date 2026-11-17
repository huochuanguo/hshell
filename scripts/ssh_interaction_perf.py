#!/usr/bin/env python3
"""Native interaction benchmark over real loopback OpenSSH, isolated keys/config.
The Paramiko fixture only echoes input and emits fixed test text, never runs shell.
Requires a perf-probe build; the fixture entry is excluded from production.
"""
import argparse
import codecs
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import paramiko
from ssh_smoke import Fixture

parser = argparse.ArgumentParser()
parser.add_argument('binary')
parser.add_argument('--output', required=True)
args = parser.parse_args()

with tempfile.TemporaryDirectory(prefix='hshell-interaction-') as directory:
    root = Path(directory)
    host_key = paramiko.RSAKey.generate(2048)
    user_key = paramiko.RSAKey.generate(2048)
    key = root/'key'; user_key.write_private_key_file(str(key)); key.chmod(0o600)
    listener = socket.socket(); listener.bind(('127.0.0.1',0)); listener.listen(8)
    stop = threading.Event()
    totals = {'connections':0, 'loadBytes':0, 'echoedKeys':0}
    totals_lock = threading.Lock()

    def serve(client):
        client.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        transport = paramiko.Transport(client)
        try:
            transport.add_server_key(host_key)
            server = Fixture(user_key)
            transport.start_server(server=server)
            channel = transport.accept(10)
            if channel is None or not server.shell.wait(10): return
            channel.settimeout(10)
            with totals_lock: totals['connections'] += 1
            write_lock = threading.Lock()
            loading = threading.Event()
            sequence = 0
            def send(data):
                with write_lock: channel.sendall(data)
            def rows(count):
                nonlocal sequence
                batch = []
                for _ in range(count):
                    sequence += 1
                    batch.append(f'\x1b[32m{sequence:08d}\x1b[0m 服务器日志 中文滚动检查 \x1b[36mrequest=fixture status=200\x1b[0m\r\n')
                return ''.join(batch).encode()
            def producer():
                try:
                    while not stop.is_set() and not channel.closed:
                        if not loading.wait(.05): continue
                        payload = rows(40)
                        send(payload)
                        with totals_lock: totals['loadBytes'] += len(payload)
                        time.sleep(.01)  # Approximately 4000 colored CJK rows/sec.
                except (OSError, EOFError): pass
            threading.Thread(target=producer, daemon=True).start()
            send(b'probe-ready\r\n')
            pending = b''
            decoder = codecs.getincrementaldecoder('utf-8')()
            def acknowledge(marker):
                send(b'\x1b]777;' + marker.encode() + b'\x07')
            while not stop.is_set():
                data = channel.recv(8192)
                if not data: break
                # Echo every byte without waiting for Enter, like an interactive PTY.
                send(data.replace(b'\r',b'\r\n'))
                for char in decoder.decode(data):
                    if 0xe100 <= ord(char) <= 0xe1ff:
                        acknowledge(char)
                        with totals_lock: totals['echoedKeys'] += 1
                pending += data.replace(b'\r',b'\n')
                while b'\n' in pending:
                    line, pending = pending.split(b'\n',1)
                    if line == b'probe-preload':
                        send(rows(4000) + b'probe-preload-done\r\n')
                        acknowledge('probe-preload-done')
                    elif line == b'probe-load-on': loading.set()
                    elif line == b'probe-load-off':
                        loading.clear()
                        send(b'probe-load-stopped\r\n')
                        acknowledge('probe-load-stopped')
                    elif line.startswith(b'probe-echo-') or line == b'probe-output-complete':
                        acknowledge(line.decode())
            loading.clear()
        except (OSError, EOFError, paramiko.SSHException): pass
        finally: transport.close()

    def accept():
        try:
            while not stop.is_set():
                client, _ = listener.accept()
                threading.Thread(target=serve, args=(client,), daemon=True).start()
        except OSError: pass
    threading.Thread(target=accept, daemon=True).start()
    port = listener.getsockname()[1]
    known_hosts = root/'known_hosts'
    known_hosts.write_text(f'[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}\n')
    config = root/'config'
    config.write_text(f'''Host fixture-perf
  HostName 127.0.0.1
  Port {port}
  User fixture
  IdentityFile {key}
  IdentitiesOnly yes
  IdentityAgent none
  UserKnownHostsFile {known_hosts}
  GlobalKnownHostsFile /dev/null
  BatchMode yes
  PreferredAuthentications publickey
  LogLevel ERROR
''')
    result_path = root/'result.json'
    env = os.environ.copy()
    env.update(HSHELL_PERF_SSH_CONFIG=str(config),HSHELL_PERF_RESULT=str(result_path), XDG_CONFIG_HOME=str(root/'app-config'))
    try:
        subprocess.run([str(Path(args.binary).resolve())],env=env,timeout=90,check=True)
        result = json.loads(result_path.read_text())
        result['fixture'] = totals.copy()
        result['transport'] = 'native WebKit + Tauri + PTY + real OpenSSH + loopback Paramiko'
        Path(args.output).write_text(json.dumps(result,indent=2,ensure_ascii=False)+'\n')
        print(json.dumps(result,ensure_ascii=False))
        assert 'error' not in result, f"Native interaction probe failed: {result}"
        assert result['fixture']['connections'] == 2, 'Both sessions must authenticate using real OpenSSH'
        assert result['fixture']['echoedKeys'] == 48, 'Every test character must be echoed by SSH'
        assert result['fixture']['loadBytes'] > 100_000, 'Background output must actually run'
        assert result['keyInputP95'] < 80, 'Character echo exceeds 80 ms P95'
        assert result['loadedInputP95'] < 100, 'Echo under output load exceeds 100 ms P95'
        assert result['scrollP95'] < 60, 'Scroll rendering exceeds 60 ms P95'
        assert result['loadedScrollP95'] < 80, 'Scrolling under output load exceeds 80 ms P95'
    finally:
        stop.set(); listener.close()
