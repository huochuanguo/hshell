#!/usr/bin/env python3
"""Launch the perf-probe build in real WebKit and enforce interaction budgets.
Build with VITE_PERF_PROBE=1 npm run build, then cargo build -p hshell
--release --features tauri/custom-protocol,perf-probe. Production has no probe.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
parser = argparse.ArgumentParser()
parser.add_argument('binary')
parser.add_argument('--output', required=True)
args = parser.parse_args()
with tempfile.TemporaryDirectory(prefix='hshell-native-perf-') as directory:
    result_path = Path(directory) / 'result.json'
    env = os.environ.copy()
    env.update(HSHELL_PERF_RESULT=str(result_path), XDG_CONFIG_HOME=str(Path(directory)/'config'))
    subprocess.run([str(Path(args.binary).resolve())], env=env, timeout=45, check=True)
    result = json.loads(result_path.read_text())
    Path(args.output).write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))
    assert result['switchP95'] < 60, 'Tab switching exceeds 60 ms P95'
    assert result['inputP95'] < 60, 'Local PTY input echo exceeds 60 ms P95'
    assert result['outputMs'] < 5000, '1 MB PTY/IPC output exceeds 5 seconds'
