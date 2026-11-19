#!/usr/bin/env python3
"""Benchmark the production GTK host filter with 1,000 isolated fixture profiles.

Measures synchronous filtering work, excluding GTK's search debounce and painting.
Uses the legacy config namespace and old records without passwordSaved.
Only runs with a development native-probe binary; never touches user data.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('binary')
parser.add_argument('--output', required=True)
parser.add_argument('--updates', action='store_true', help='Exercise persisted favorite/edit/add/remove updates')
parser.add_argument('--interleave', action='store_true', help='Save and filter while initial batches are still building')
parser.add_argument('--max-replaced-rows', type=int, help='Regression budget for rebuilding existing GTK rows')
args = parser.parse_args()
binary = Path(args.binary).resolve()
with tempfile.TemporaryDirectory(prefix='hshell-host-filter-') as directory:
    root = Path(directory)
    config = root / 'io.hshell.ssh/profiles.json'
    config.parent.mkdir()
    profiles = [dict(id=f'bench-{i}', name=f'bench-node-{i}', host=f'host-{i}',
                     username='fixture', port=None, identityFile='', group=f'Group {i % 10}',
                     favorite=i % 2 == 0) for i in range(1000)]
    config.write_text(json.dumps(profiles))
    original = config.read_bytes()
    result = root / 'result.json'
    env = os.environ.copy()
    env.update(XDG_CONFIG_HOME=str(root), HSHELL_NATIVE_PROBE='1',
               HSHELL_NATIVE_HOST_BENCH='1', HSHELL_NATIVE_RESULT=str(result))
    if args.updates:
        env['HSHELL_NATIVE_HOST_UPDATES'] = '1'
    if args.interleave:
        assert args.updates, '--interleave requires --updates'
        env['HSHELL_NATIVE_HOST_INTERLEAVE'] = '1'
    process = subprocess.run([str(binary)], env=env, capture_output=True, text=True, timeout=60)
    assert process.returncode == 0, process.stderr
    assert not process.stderr, process.stderr
    data = json.loads(result.read_text())
    assert data['phase'] == 'complete', data
    if args.updates:
        expected = json.loads(original)
        for profile in expected:
            profile['passwordSaved'] = False
        assert json.loads(config.read_text()) == expected, 'Host update benchmark did not restore fixture profiles'
    else:
        assert config.read_bytes() == original, 'Filtering modified saved profiles'
    samples = [s['elapsedMs' if args.updates else 'milliseconds'] for s in data['updates' if args.updates else 'samples']]
    data.update(binarySha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                medianMs=round(statistics.median(samples), 2), maxMs=round(max(samples), 2))
    Path(args.output).write_text(json.dumps(data, indent=2) + '\n')
    print(json.dumps(data))
    if args.max_replaced_rows is not None:
        assert args.updates, '--max-replaced-rows requires --updates'
        assert all(row['replacedRows'] <= args.max_replaced_rows for row in data['updates']), 'Existing host rows were unnecessarily rebuilt'
