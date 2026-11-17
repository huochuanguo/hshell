#!/usr/bin/env python3
"""Real OpenSSH / native VTE memory and lifecycle regression, loopback only.

Requires a native-probe build. Never use that development binary to publish.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shlex
import socket
import statistics
import subprocess
import sys
import tempfile
import threading
import time

import paramiko
from native_memory import memory
from measure_linux import snapshot
from ssh_smoke import Fixture


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary')
    parser.add_argument('--output', required=True)
    parser.add_argument('--snapshot')
    parser.add_argument('--environment', default='Ubuntu 24 X11')
    parser.add_argument('--credentials', action='store_true')
    parser.add_argument('--ime', action='store_true', help='Real isolated IBus/libpinyin composition and commit')
    parser.add_argument('--no-reclaim', action='store_true', help='Development-only A/B: disable idle heap reclamation')
    parser.add_argument('--stress', action='store_true', help='Sustained multi-session output and lifecycle regression')
    parser.add_argument('--tabs', type=int, default=12)
    parser.add_argument('--seconds', type=int, default=20)
    parser.add_argument('--cycles', type=int, default=32)
    parser.add_argument('--sample-interval', type=float, default=1, help='Seconds between resource samples (up to 60 for a multi-hour soak)')
    parser.add_argument('--partial-load', action='store_true', help='Keep input/output running while other tabs are reclaimed')
    parser.add_argument('--max-retained-pss', type=float, help='Optional regression budget after closing all but one session')
    parser.add_argument('--callgrind-control', help='Diagnostic-only instrumented run; excludes timing/memory budgets')
    parser.add_argument('--inside', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    assert not (args.stress and args.credentials), 'Stress mode uses isolated key authentication'
    assert not args.ime or not (args.stress or args.credentials)
    assert not args.partial_load or args.stress
    assert args.max_retained_pss is None or args.stress
    assert not args.callgrind_control or args.stress
    assert 2 <= args.tabs <= 12 and 5 <= args.seconds <= 28_800 and 1 <= args.cycles <= 100
    assert 1 <= args.sample_interval <= 60
    assert not args.callgrind_control or args.seconds <= 60
    binary = Path(args.binary).resolve()
    with tempfile.TemporaryDirectory(prefix='hshell-native-vte-') as directory:
        root = Path(directory)
        host_key = paramiko.RSAKey.generate(2048)
        user_key = paramiko.RSAKey.generate(2048)
        key = root / 'key'
        user_key.write_private_key_file(str(key))
        key.chmod(0o600)
        listener = socket.socket()
        listener.bind(('127.0.0.1', 0))
        listener.listen(16)
        totals = {'connections': 0, 'fillBytes': 0, 'passwordAuthentications': 0, 'stressBytes': 0, 'stressStreamsFinished': 0}
        lock = threading.Lock()
        stop = threading.Event()
        transports = []

        class NativeFixture(Fixture):
            def get_allowed_auths(self, username):
                return 'password' if args.credentials else 'publickey'

            def check_auth_publickey(self, username, key):
                return paramiko.AUTH_FAILED if args.credentials else super().check_auth_publickey(username, key)

            def check_auth_password(self, username, password):
                result = super().check_auth_password(username, password)
                if result == paramiko.AUTH_SUCCESSFUL:
                    with lock:
                        totals['passwordAuthentications'] += 1
                return result

        def serve(client):
            client.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            transport = paramiko.Transport(client)
            with lock:
                transports.append(transport)
            try:
                transport.add_server_key(host_key)
                server = NativeFixture(user_key)
                transport.start_server(server=server)
                channel = transport.accept(10)
                if channel is None or not server.shell.wait(10):
                    return
                with lock:
                    totals['connections'] += 1
                output_lock = threading.Lock()

                def send(payload):
                    with output_lock:
                        channel.sendall(payload)

                def stream():
                    try:
                        for batch in range(100):
                            send(''.join(f'stream-{batch:03d}-{i:02d} 中文持续输出 status=200\r\n' for i in range(50)).encode())
                            time.sleep(.01)
                        send(b'stream-finished\r\n')
                    except (EOFError, OSError):
                        pass

                def stress_stream(seconds):
                    deadline = time.monotonic() + seconds
                    batch = 0
                    try:
                        while time.monotonic() < deadline and not stop.is_set():
                            payload = ''.join(f'\x1b[32m{batch:08d}.{i:02d}\x1b[0m 中文持续日志 status=200 request=fixture\r\n' for i in range(50)).encode()
                            send(payload)
                            with lock:
                                totals['stressBytes'] += len(payload)
                            batch += 1
                            time.sleep(.02)
                        send(b'stress-stream-finished\r\n')
                        with lock:
                            totals['stressStreamsFinished'] += 1
                    except (EOFError, OSError):
                        pass

                send(b'fixture-ready\r\n')
                pending = b''
                while not stop.is_set():
                    data = channel.recv(8192)
                    if not data:
                        break
                    send(data.replace(b'\r', b'\r\n'))
                    pending += data.replace(b'\r', b'\n')
                    while b'\n' in pending:
                        line, pending = pending.split(b'\n', 1)
                        if line == b'probe-fill':
                            payload = ''.join(f'\x1b[32m{i:08d}\x1b[0m 中文滚动检查 request=fixture status=200\r\n' for i in range(4000)).encode()
                            send(payload + b'buffer-filled\r\n')
                            with lock:
                                totals['fillBytes'] += len(payload)
                        elif line == b'probe-size':
                            send(f'fixture-size:{server.size[0]}x{server.size[1]}\r\n'.encode())
                        elif line == b'probe-stream':
                            threading.Thread(target=stream, daemon=True).start()
                        elif line.startswith(b'probe-stress:'):
                            threading.Thread(target=stress_stream, args=(int(line.split(b':')[1]),), daemon=True).start()
                        elif line == b'probe-exit':
                            send(b'fixture-exit\r\n')
                            channel.send_exit_status(0)
                            channel.shutdown_write()
                            channel.close()
                            transport.join(timeout=2)
                            return
            except (EOFError, OSError, paramiko.SSHException):
                pass
            finally:
                transport.close()

        def accept():
            try:
                while not stop.is_set():
                    client, _ = listener.accept()
                    threading.Thread(target=serve, args=(client,), daemon=True).start()
            except OSError:
                pass

        threading.Thread(target=accept, daemon=True).start()
        port = listener.getsockname()[1]
        known = root / 'known_hosts'
        known.write_text('' if args.credentials else f'[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}\n')
        config = root / 'ssh-config'
        config.write_text(f'''Host fixture
  HostName 127.0.0.1
  Port {port}
  User fixture
  IdentityFile {key}
  IdentitiesOnly yes
  IdentityAgent none
  UserKnownHostsFile {known}
  GlobalKnownHostsFile /dev/null
  BatchMode {'no' if args.credentials else 'yes'}
  PreferredAuthentications {'password' if args.credentials else 'publickey'}
  LogLevel ERROR
''')
        bin_dir = root / 'bin'
        bin_dir.mkdir()
        wrapper = bin_dir / 'ssh'
        wrapper.write_text('#!/bin/sh\nexec /usr/bin/ssh -F ' + shlex.quote(str(config)) + ' "$@"\n')
        wrapper.chmod(0o700)
        result_path = root / 'result.json'
        env = os.environ.copy()
        env.update(XDG_CONFIG_HOME=str(root / 'app-config'), PATH=str(bin_dir) + os.pathsep + env['PATH'],
                   HSHELL_NATIVE_PROBE='1', HSHELL_NATIVE_RESULT=str(result_path),
                   HSHELL_NATIVE_FINGERPRINT='SHA256:' + base64.b64encode(hashlib.sha256(host_key.asbytes()).digest()).decode().rstrip('='))
        if args.credentials:
            env['HSHELL_NATIVE_CREDENTIALS'] = '1'
        if args.ime:
            env.update(HSHELL_NATIVE_IME='1', GTK_IM_MODULE='ibus')
            env.pop('IBUS_USE_PORTAL', None)
        if args.no_reclaim:
            env['HSHELL_NATIVE_NO_RECLAIM'] = '1'
        if args.stress:
            env.update(HSHELL_NATIVE_STRESS='1', HSHELL_STRESS_TABS=str(args.tabs),
                       HSHELL_STRESS_SECONDS=str(args.seconds), HSHELL_STRESS_CYCLES=str(args.cycles))
        if args.callgrind_control:
            env['HSHELL_NATIVE_PROFILE'] = '1'
        if args.partial_load:
            env['HSHELL_STRESS_PARTIAL_LOAD'] = '1'
        if args.snapshot:
            env['HSHELL_NATIVE_SNAPSHOT'] = str(Path(args.snapshot).resolve())
        stages = {}
        streaming = []
        previous_cpu = None
        ime_selected = False
        profile_started = False
        profile_dumped = False
        next_sample = 0
        next_progress = 0
        long_run = args.stress and args.seconds >= 3600
        progress_path = Path(args.output).with_suffix('.progress.json')
        samples_path = Path(args.output).with_suffix('.samples.jsonl')
        if long_run:
            samples_path.write_text('')
        process = subprocess.Popen([str(binary)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            deadline = time.monotonic() + (args.seconds + args.cycles*2 + 60 if args.stress else 90)
            final = None
            while time.monotonic() < deadline:
                try:
                    result = json.loads(result_path.read_text())
                except (OSError, ValueError):
                    if process.poll() is not None:
                        raise AssertionError(f'Native client exited early: {process.returncode}: {process.stderr.read().decode()}')
                    time.sleep(.02)
                    continue
                phase = result['phase']
                assert phase != 'error', result
                if args.ime and phase == 'ime-ready' and not ime_selected:
                    result_path.with_suffix('.ready').touch()
                    ime_selected = True
                if args.callgrind_control and phase == 'stress-streaming' and not profile_started:
                    subprocess.run([args.callgrind_control, '-z', str(process.pid)], check=True, timeout=15)
                    profile_started = True
                if args.callgrind_control and phase == 'stress-filled' and not profile_dumped:
                    subprocess.run([args.callgrind_control, '-d', str(process.pid)], check=True, timeout=15)
                    profile_dumped = True
                if phase == 'complete':
                    final = result
                    break
                if process.poll() is not None:
                    raise AssertionError(f'Native client exited during {phase}: {process.returncode}: {process.stderr.read().decode()}')
                if phase == 'stress-streaming' and time.monotonic() >= next_sample:
                    sampled_at = time.monotonic()
                    usage = memory(process.pid)
                    ticks = sum(snapshot(process.pid)['processes'].values())
                    cpu = None if previous_cpu is None else round((ticks-previous_cpu[1])/os.sysconf('SC_CLK_TCK')/(sampled_at-previous_cpu[0])*100, 2)
                    previous_cpu = (sampled_at, ticks)
                    streaming.append(dict(seconds=round(sampled_at-streaming_started, 2) if streaming else 0,
                                          pssMiB=round(usage['pssMiB'], 2), rssMiB=round(usage['rssMiB'], 2),
                                          cpuPercentOneCore=cpu, fds=len(list(Path(f'/proc/{process.pid}/fd').iterdir())),
                                          threads=len(list(Path(f'/proc/{process.pid}/task').iterdir()))))
                    if len(streaming) == 1:
                        streaming_started = sampled_at
                    if long_run:
                        with samples_path.open('a') as samples_file:
                            samples_file.write(json.dumps(streaming[-1]) + '\n')
                    if sampled_at >= next_progress:
                        print(json.dumps({'phase': 'stress-progress', **streaming[-1]}), flush=True)
                        if long_run:
                            progress_path.write_text(json.dumps(dict(phase='running', processPid=process.pid,
                                fixturePid=os.getpid(), configuredSeconds=args.seconds, latest=streaming[-1])) + '\n')
                        next_progress = sampled_at + 60
                    next_sample = sampled_at + args.sample_interval
                if phase not in stages:
                    samples = []
                    for _ in range(4):
                        samples.append(memory(process.pid))
                        time.sleep(.25)
                    names = [p['name'] for p in samples[-1]['processes']]
                    assert not any('WebKit' in name for name in names), names
                    stage = {
                        'pssMedianMiB': round(statistics.median(s['pssMiB'] for s in samples), 2),
                        'rssMedianMiB': round(statistics.median(s['rssMiB'] for s in samples), 2),
                        'sessions': result['sessions'], 'sshProcesses': names.count('ssh'), 'samples': samples,
                        'fds': len(list(Path(f'/proc/{process.pid}/fd').iterdir())),
                        'threads': len(list(Path(f'/proc/{process.pid}/task').iterdir())),
                    }
                    assert stage['sshProcesses'] == result['sessions'], stage
                    stages[phase] = stage
                    print(json.dumps({'phase': phase, **{k: v for k, v in stage.items() if k != 'samples'}}), flush=True)
                time.sleep(.02)
            assert final, 'Native probe timed out'
            process.wait(timeout=5)
            assert process.returncode == 0, process.stderr.read().decode()
            assert totals['connections'] == (1 if args.ime else args.tabs+args.cycles if args.stress else 14), totals
            assert totals['passwordAuthentications'] == (14 if args.credentials else 0), totals
            assert totals['fillBytes'] >= (0 if args.ime else args.cycles*250_000 if args.stress else 1_000_000), totals
            if args.ime:
                assert final['commits'] == 3 and final['preeditWasNotSentToSsh'], final
            elif args.stress:
                if 'reclaimCalls' in final and not args.callgrind_control:
                    assert final['reclaimCalls'] == (0 if args.no_reclaim else 3), final
                assert totals['stressStreamsFinished'] == args.tabs + int(args.partial_load), totals
                if args.partial_load:
                    assert final['partialLoad'], final
                    if not args.callgrind_control:
                        assert final['partialInputP95'] < 200, final
                assert totals['stressBytes'] >= args.tabs*args.seconds*60_000, totals
                if not args.callgrind_control:
                    assert final['inputP95'] < 200 and final['loopTickP95'] < 80, final
                    assert stages['stress-one-after-cycles']['fds'] <= stages['stress-one-after-close']['fds'] + 2, stages
                    assert stages['stress-one-after-cycles']['threads'] <= stages['stress-one-after-close']['threads'] + 1, stages
                    assert stages['stress-one-after-cycles']['pssMedianMiB'] <= stages['stress-one-after-close']['pssMedianMiB'] + 5, stages
                else:
                    assert profile_started and profile_dumped, 'Missing scoped Callgrind sample'
            else:
                assert final['inputP95'] < 80, final
                assert final['inputUnderLoadP95'] < 80, final
                assert final['scrollTickP95'] < 50, final
                assert max(s['pssMedianMiB'] for s in stages.values()) < 120, stages
                assert stages['one-after-cycles']['pssMedianMiB'] <= stages['one-after-close']['pssMedianMiB'] + 3, stages
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline and any(t.is_active() for t in transports):
                time.sleep(.02)
            assert not any(t.is_active() for t in transports), 'Connections survived exit'
            for proc in Path('/proc').iterdir():
                try:
                    argv = (proc / 'cmdline').read_bytes().split(b'\0')
                    assert not (str(config).encode() in argv and Path(os.fsdecode(argv[0])).name == 'ssh'), 'SSH child survived exit'
                except (OSError, ValueError):
                    pass
            stderr = process.stderr.read().decode()
            assert 'CRITICAL' not in stderr and 'WARNING' not in stderr, stderr
            output = {'binarySha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'environment': args.environment,
                      'diagnosticOnly': bool(args.callgrind_control),
                      'stages': stages, 'fixture': totals, 'result': final, 'streaming': streaming,
                      'stderr': stderr, 'sshRemaining': 0,
                      'notes': 'Development-only native GTK/VTE build. Same production PTY path and UI. Loopback fixture; input ends at parsed echo, not physical keyboard latency. Scroll tick measures GTK loop scheduling, not display frame time. PSS depends on co-running processes; no GPU device memory measurement.'}
            Path(args.output).write_text(json.dumps(output, indent=2, ensure_ascii=False) + '\n')
            if long_run:
                progress_path.write_text(json.dumps(dict(phase='complete', report=args.output, sshRemaining=0)) + '\n')
            print(json.dumps(final))
            if args.max_retained_pss is not None:
                assert stages['stress-one-after-close']['pssMedianMiB'] <= args.max_retained_pss, f"Retained memory exceeds {args.max_retained_pss} MiB: {stages['stress-one-after-close']['pssMedianMiB']} MiB"
        finally:
            stop.set()
            listener.close()
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            process.stderr.close()


if __name__ == '__main__':
    if ('--credentials' in sys.argv or '--ime' in sys.argv) and '--inside' not in sys.argv:
        with tempfile.TemporaryDirectory(prefix='hshell-native-keyring-') as directory:
            env = os.environ.copy()
            for key, suffix in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'run')]:
                path = Path(directory) / suffix
                path.mkdir(mode=0o700)
                env[key] = str(path)
            env.pop('GNOME_KEYRING_CONTROL', None)
            subprocess.run(['dbus-run-session', '--', sys.executable, __file__, *sys.argv[1:], '--inside'], env=env, check=True, timeout=150)
    elif '--inside' in sys.argv:
        if '--ime' in sys.argv:
            os.environ['IBUS_ADDRESS'] = 'unix:path=' + str(Path(os.environ['XDG_RUNTIME_DIR']) / 'ibus')
            os.environ.pop('IBUS_USE_PORTAL', None)
            daemon = subprocess.Popen(['ibus-daemon', '--panel=disable', '--emoji-extension=disable', '--address=' + os.environ['IBUS_ADDRESS']])
        else:
            daemon = subprocess.Popen(['gnome-keyring-daemon', '--foreground', '--unlock', '--components=secrets'], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL)
            daemon.stdin.write(b'disposable-native-fixture\n')
            daemon.stdin.close()
        try:
            if '--ime' in sys.argv:
                time.sleep(1)
                subprocess.run(['/usr/bin/python3', str(Path(__file__).with_name('isolated_ime.py'))], check=True, timeout=12)
            main()
        finally:
            daemon.terminate()
            daemon.wait(timeout=10)
    else:
        main()
