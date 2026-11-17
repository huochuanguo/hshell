#!/usr/bin/env python3
"""Measure startup milestones with isolated 0/1,000-host fixtures and mapped UI.

Requires a native-probe release build; does not clear global filesystem caches.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import time

from x11_window import X11Windows

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('binary')
parser.add_argument('--output', required=True)
parser.add_argument('--runs', type=int, default=3)
args = parser.parse_args()
assert 1 <= args.runs <= 10
binary = Path(args.binary).resolve()
rows = []
for count in [0, 1000]:
    for run in range(args.runs):
        with tempfile.TemporaryDirectory(prefix='hshell-startup-') as directory:
            root = Path(directory)
            config = root / 'io.qizhan.ssh/profiles.json'
            config.parent.mkdir()
            config.write_text(json.dumps([dict(id=f'bench-{i}', name=f'bench-node-{i}', host=f'host-{i}',
                username='fixture', port=None, identityFile='', group=f'Group {i % 10}', favorite=i % 2 == 0)
                for i in range(count)]))
            result = root / 'result.json'
            env = os.environ.copy()
            env.update(XDG_CONFIG_HOME=str(root), GDK_BACKEND='x11', HSHELL_NATIVE_PROBE='1',
                       HSHELL_NATIVE_STARTUP='1', HSHELL_NATIVE_RESULT=str(result))
            windows = X11Windows()
            started = time.monotonic()
            process = subprocess.Popen([str(binary)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            try:
                window = windows.wait(process, 15)
                mapped = (time.monotonic()-started)*1000
                _, stderr = process.communicate(timeout=30)
                assert process.returncode == 0 and not stderr, stderr.decode()
                data = json.loads(result.read_text())
                assert data['phase'] == 'complete', data
                data.update(hosts=count, run=run+1, launchToMappedMs=mapped, window=window)
                rows.append(data)
                print(json.dumps(data), flush=True)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.communicate()
                windows.close()
summary = {}
for count in [0, 1000]:
    group = [row for row in rows if row['hosts'] == count]
    keys = group[0]['milestonesMs'].keys()
    summary[str(count)] = {key: round(statistics.median(row['milestonesMs'][key] for row in group), 2) for key in keys}
output = dict(binarySha256=hashlib.sha256(binary.read_bytes()).hexdigest(), runs=rows, medianMilestonesMs=summary,
              notes='Repeated startup on one desktop; not a cold-cache or physical presentation measurement. Own child-PID mapped window verified.')
Path(args.output).write_text(json.dumps(output, indent=2) + '\n')
