#!/usr/bin/env python3
"""Native window-close regression with real OpenSSH and disposable loopback keys.
Requires VITE_PERF_PROBE=1 frontend + perf-probe Rust build. Never use to publish.
"""
import argparse
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

with tempfile.TemporaryDirectory(prefix='hshell-close-') as directory:
    root = Path(directory)
    host_key = paramiko.RSAKey.generate(2048)
    user_key = paramiko.RSAKey.generate(2048)
    key = root / 'key'
    user_key.write_private_key_file(str(key))
    key.chmod(0o600)
    listener = socket.socket()
    listener.bind(('127.0.0.1', 0))
    listener.listen(8)
    connections = []
    lock = threading.Lock()

    def serve(client):
        transport = paramiko.Transport(client)
        try:
            transport.add_server_key(host_key)
            server = Fixture(user_key)
            transport.start_server(server=server)
            channel = transport.accept(10)
            if channel is None or not server.shell.wait(10):
                return
            with lock:
                connections.append(transport)
            channel.sendall(b'fixture-ready\r\n')
            while channel.recv(8192):
                pass
        except (EOFError, OSError, paramiko.SSHException):
            pass
        finally:
            transport.close()

    def accept():
        try:
            while True:
                client, _ = listener.accept()
                threading.Thread(target=serve, args=(client,), daemon=True).start()
        except OSError:
            pass
    threading.Thread(target=accept, daemon=True).start()
    port = listener.getsockname()[1]
    known_hosts = root / 'known_hosts'
    known_hosts.write_text(f'[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}\n')
    config = root / 'ssh-config'
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

    def live_ssh():
        found = []
        for entry in Path('/proc').iterdir():
            if not entry.name.isdigit():
                continue
            try:
                argv = (entry / 'cmdline').read_bytes().split(b'\0')
                if str(config).encode() in argv and Path(os.fsdecode(argv[0])).name == 'ssh':
                    found.append(int(entry.name))
            except (OSError, ValueError):
                pass
        return found

    results = []
    try:
        for scenario in ['close-empty', 'close-running', 'close-cancel-retry', 'close-repeated', 'close-editor', 'close-info']:
            result_path = root / f'{scenario}.json'
            env = os.environ.copy()
            env.update(HSHELL_PERF_SCENARIO=scenario, HSHELL_PERF_SSH_CONFIG=str(config),
                       HSHELL_PERF_RESULT=str(result_path), XDG_CONFIG_HOME=str(root / scenario))
            before = len(connections)
            started = time.monotonic()
            process = subprocess.Popen([str(Path(args.binary).resolve())], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                out, err = process.communicate(timeout=20)
            except subprocess.TimeoutExpired:
                process.kill()
                out, err = process.communicate()
                raise AssertionError(f'{scenario}: native app failed to exit; stderr={err.decode()}')
            assert process.returncode == 0, (scenario, process.returncode, err.decode())
            result = json.loads(result_path.read_text())
            assert 'error' not in result, result
            assert result_path.with_suffix('.destroyed').exists(), f'{scenario}: no native destruction event'
            expected = 0 if scenario == 'close-empty' else 2
            assert len(connections) - before == expected, f'{scenario}: SSH sessions did not authenticate'
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline and (live_ssh() or any(t.is_active() for t in connections)):
                time.sleep(.03)
            assert not live_ssh(), f'{scenario}: SSH child processes survived app exit'
            assert not any(t.is_active() for t in connections), f'{scenario}: server connections survived app exit'
            result.update(exitCode=process.returncode, destroyed=True, sshConnections=expected,
                          sshRemaining=0, elapsedMs=round((time.monotonic() - started) * 1000))
            results.append(result)
            print(json.dumps(result), flush=True)
        Path(args.output).write_text(json.dumps(results, indent=2) + '\n')
    finally:
        listener.close()
