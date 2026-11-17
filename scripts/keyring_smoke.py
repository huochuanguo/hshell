#!/usr/bin/env python3
"""Check real Secret Service with disposable DBus, keyring and XDG directories."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile

if len(sys.argv) > 1 and sys.argv[1] == '--inside':
    daemon = subprocess.Popen(['gnome-keyring-daemon', '--foreground', '--unlock', '--components=secrets'], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL)
    daemon.stdin.write(b'disposable-keyring-fixture\n')
    daemon.stdin.close()
    try:
        result = subprocess.run(['target/debug/examples/credentials_check'], timeout=30)
    finally:
        daemon.terminate()
        daemon.wait(timeout=10)
    sys.exit(result.returncode)

subprocess.run(['cargo', 'build', '-p', 'ssh-core', '--example', 'credentials_check', '--locked'], check=True)
with tempfile.TemporaryDirectory(prefix='hshell-keyring-fixture-') as directory:
    env = os.environ.copy()
    for key, suffix in [('HOME', 'home'), ('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'run')]:
        path = Path(directory) / suffix
        path.mkdir(mode=0o700)
        env[key] = str(path)
    env.pop('GNOME_KEYRING_CONTROL', None)
    result = subprocess.run(['dbus-run-session', '--', sys.executable, __file__, '--inside'], env=env, timeout=50)
    sys.exit(result.returncode)
