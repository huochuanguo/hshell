#!/usr/bin/env python3
"""Compare native Linux process-tree memory using isolated app configurations.

No terminal contents, connection addresses, environment or credentials are read.
Use the same display, binary baseline, workload and co-running apps for A/B runs.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import statistics
import subprocess
import tempfile
import time

from measure_linux import snapshot
from x11_window import X11Windows


def memory(pid):
    tree = snapshot(pid)
    rows = []
    for child in sorted(tree['processes']):
        try:
            fields = {}
            for line in Path(f'/proc/{child}/smaps_rollup').read_text().splitlines():
                key = line.split(':', 1)[0]
                if key in ['Rss', 'Pss', 'Private_Clean', 'Private_Dirty', 'SwapPss']:
                    fields[key] = int(line.split()[1])
            rows.append({
                'name': Path(f'/proc/{child}/comm').read_text().strip(),
                'rssMiB': fields['Rss'] / 1024,
                'pssMiB': fields['Pss'] / 1024,
                'privateMiB': (fields['Private_Clean'] + fields['Private_Dirty']) / 1024,
                'swapPssMiB': fields['SwapPss'] / 1024,
            })
        except (OSError, KeyError) as error:
            raise RuntimeError(f'Cannot measure complete process tree: {child}') from error
    assert rows, 'Application exited during measurement'
    return {
        'pssMiB': sum(r['pssMiB'] for r in rows),
        'rssMiB': sum(r['rssMiB'] for r in rows),
        'privateMiB': sum(r['privateMiB'] for r in rows),
        'swapPssMiB': sum(r['swapPssMiB'] for r in rows),
        'processes': rows,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary')
    parser.add_argument('--output', required=True)
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--warmup', type=float, default=5)
    parser.add_argument('--samples', type=int, default=10)
    parser.add_argument('--max-pss', type=float)
    parser.add_argument('--env', action='append', default=[])
    args = parser.parse_args()
    assert 1 <= args.runs <= 10 and 1 <= args.samples <= 120
    binary = Path(args.binary).resolve()
    result = {
        'binarySha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
        'scenario': 'idle native window, mapped X11 window verified by child PID, isolated configuration',
        'runs': [],
        'notes': 'PSS apportions shared pages; RSS repeats shared pages. GPU device memory is excluded. Keep co-running apps and display constant.',
    }
    for run in range(args.runs):
        with tempfile.TemporaryDirectory(prefix='hshell-memory-') as directory:
            env = os.environ.copy()
            env['XDG_CONFIG_HOME'] = str(Path(directory) / 'config')
            env['GDK_BACKEND'] = 'x11'
            for item in args.env:
                key, value = item.split('=', 1)
                env[key] = value
            assert env['GDK_BACKEND'] == 'x11', 'Memory readiness verification currently requires X11'
            windows = X11Windows()
            process = subprocess.Popen([str(binary)], env=env, stdout=subprocess.DEVNULL,
                                       stderr=subprocess.PIPE, start_new_session=True)
            try:
                window = windows.wait(process)
                time.sleep(args.warmup)
                assert process.poll() is None, process.stderr.read().decode()
                assert windows.find(process.pid), 'Test window disappeared before measurement'
                initial_cpu = snapshot(process.pid)['processes']
                cpu_started = time.monotonic()
                samples = []
                for _ in range(args.samples):
                    samples.append(memory(process.pid))
                    time.sleep(.5)
                assert windows.find(process.pid), 'Test window disappeared during measurement'
                final_cpu = snapshot(process.pid)['processes']
                cpu_seconds = sum(max(0, ticks - initial_cpu.get(pid, 0)) for pid, ticks in final_cpu.items()) / os.sysconf('SC_CLK_TCK')
                row = {
                    'run': run + 1,
                    'window': window,
                    'pssMedianMiB': round(statistics.median(s['pssMiB'] for s in samples), 2),
                    'rssMedianMiB': round(statistics.median(s['rssMiB'] for s in samples), 2),
                    'pssMaxMiB': round(max(s['pssMiB'] for s in samples), 2),
                    'cpuPercentOneCore': round(cpu_seconds / (time.monotonic() - cpu_started) * 100, 2),
                    'samples': samples,
                }
                result['runs'].append(row)
                print(json.dumps({k: v for k, v in row.items() if k != 'samples'}), flush=True)
            finally:
                # Only the process group created by this test is signalled.
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGTERM)
                    try:
                        process.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.wait(timeout=3)
                process.stderr.close()
                windows.close()
    result['pssMedianMiB'] = round(statistics.median(r['pssMedianMiB'] for r in result['runs']), 2)
    result['rssMedianMiB'] = round(statistics.median(r['rssMedianMiB'] for r in result['runs']), 2)
    Path(args.output).write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({k: v for k, v in result.items() if k != 'runs'}))
    if args.max_pss is not None:
        assert result['pssMedianMiB'] <= args.max_pss, f"PSS exceeds {args.max_pss} MiB: {result['pssMedianMiB']} MiB"


if __name__ == '__main__':
    main()
