#!/usr/bin/env python3
"""Measure an existing app PID and descendants; no terminal contents are read."""
import argparse
import json
import os
from pathlib import Path
import time


def snapshot(root_pid):
    processes = {}
    for path in Path('/proc').iterdir():
        if not path.name.isdigit():
            continue
        try:
            data = (path / 'stat').read_text().rsplit(')', 1)[1].split()
            processes[int(path.name)] = {
                'parent': int(data[1]), 'ticks': int(data[11]) + int(data[12]),
                'rss': int(data[21]) * os.sysconf('SC_PAGE_SIZE'),
            }
        except (OSError, ValueError, IndexError):
            continue
    selected = {root_pid} if root_pid in processes else set()
    while True:
        children = {pid for pid, row in processes.items() if row['parent'] in selected}
        if children <= selected:
            break
        selected |= children
    pss = 0
    pss_readable = True
    for pid in selected:
        try:
            lines = Path(f'/proc/{pid}/smaps_rollup').read_text().splitlines()
            pss += int(next(line.split()[1] for line in lines if line.startswith('Pss:'))) * 1024
        except (OSError, StopIteration):
            pss_readable = False
    return {
        'processes': {pid: processes[pid]['ticks'] for pid in selected},
        'rss_mib': round(sum(processes[pid]['rss'] for pid in selected) / 1024**2, 2),
        'pss_mib': round(pss / 1024**2, 2) if pss_readable else None,
    }


def measure(pid, seconds):
    initial = snapshot(pid)
    if not initial['processes']:
        raise SystemExit('PID does not exist')
    start = time.monotonic()
    time.sleep(seconds)
    final = snapshot(pid)
    elapsed = time.monotonic() - start
    if not final['processes']:
        raise SystemExit('Application exited during measurement')
    ticks = sum(max(0, value - initial['processes'].get(process, 0)) for process, value in final['processes'].items())
    return {
        'pid': pid, 'elapsed_seconds': round(elapsed, 2), 'process_count': len(final['processes']),
        'cpu_percent_one_core': round(ticks / os.sysconf('SC_CLK_TCK') / elapsed * 100, 2),
        'rss_mib': final['rss_mib'], 'pss_mib': final['pss_mib'],
        'notes': 'RSS double-counts shared pages; PSS may be unavailable for sandboxed processes. Exited child CPU is not included.',
    }


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('pid', type=int)
    parser.add_argument('--seconds', type=int, default=30)
    args = parser.parse_args()
    if not 1 <= args.seconds <= 60:
        parser.error('--seconds must be in 1..60')
    print(json.dumps(measure(args.pid, args.seconds), ensure_ascii=False, indent=2))
