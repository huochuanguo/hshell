#!/usr/bin/env python3
"""Validate a native soak against the requested duration (default: two hours).

Resource growth compares two stable 30-minute windows after warmup. RSS and
PSS are both reported; changes to system-library sharing can alter PSS.
An intentionally shortened run keeps its resource evidence separate from a
completed follow-up lifecycle run; it cannot supply full-duration latency data.
"""
import argparse
import hashlib
import json
from pathlib import Path
import statistics


def check_lifecycle(data, manifest, minimum_seconds):
    result = data['result']
    assert not data['diagnosticOnly'], 'Instrumented profiles cannot validate a soak'
    assert result['phase'] == 'complete' and result['seconds'] >= minimum_seconds
    assert result['tabs'] == 12 and result['cycles'] >= 64
    assert data['binarySha256'] == manifest['binarySha256'], 'Different test binary'
    assert data['sshRemaining'] == 0 and result['sessions'] == 0
    assert result['hiddenOutputComplete'] and result['reclaimCalls'] == 3
    assert result['partialLoad'], 'Missing input during partial cleanup'
    assert result['inputP95'] < 200 and result['partialInputP95'] < 200
    assert result['loopTickP95'] < 80
    assert data['fixture']['connections'] == result['tabs'] + result['cycles']
    return result


def check_resources(samples, seconds, minimum_seconds, max_growth_mib):
    assert seconds >= minimum_seconds, 'Requested duration not reached'
    assert samples[-1]['seconds'] >= seconds - 61, 'Full runtime not observed'
    assert samples[0]['seconds'] <= 60, 'Missing initial observation'
    assert all(0 < b['seconds'] - a['seconds'] <= 61
               for a, b in zip(samples, samples[1:])), 'Sampling gap or reversed time'
    assert all(row['cpuPercentOneCore'] is None or row['cpuPercentOneCore'] >= 0
               for row in samples), 'Process-tree CPU counter reset during workload'
    first = [row for row in samples if 600 <= row['seconds'] <= 2400]
    last = [row for row in samples if row['seconds'] >= seconds - 1800]
    assert len(first) >= 29 and len(last) >= 29, 'Not enough stable-window samples'
    growth = {}
    for key in ['pssMiB', 'rssMiB']:
        start = statistics.median(row[key] for row in first)
        end = statistics.median(row[key] for row in last)
        growth[key] = dict(firstMedian=start, finalMedian=end, delta=round(end-start, 2))
        assert end-start <= max_growth_mib, (key, growth[key])
    for key in ['fds', 'threads']:
        assert max(row[key] for row in last) <= max(row[key] for row in first), key
    stable = [row for row in samples if row['seconds'] >= 600]
    return dict(
        observedSampleSeconds=samples[-1]['seconds'], samples=len(samples),
        resourceGrowth=growth,
        pssStableRangeMiB=[min(row['pssMiB'] for row in stable),
                           max(row['pssMiB'] for row in stable)],
        rssStableRangeMiB=[min(row['rssMiB'] for row in stable),
                           max(row['rssMiB'] for row in stable)],
        fdsStableRange=[min(row['fds'] for row in stable), max(row['fds'] for row in stable)],
        threadsStableRange=[min(row['threads'] for row in stable), max(row['threads'] for row in stable)],
        cpuMedianOneCore=statistics.median(row['cpuPercentOneCore'] for row in stable
                                          if row['cpuPercentOneCore'] is not None))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--max-growth-mib', type=float, default=8)
    parser.add_argument('--min-seconds', type=int, default=7200)
    parser.add_argument('--shortened-close', type=Path,
                        help='Intentional close record; positional report is streaming JSONL')
    parser.add_argument('--lifecycle-report', type=Path,
                        help='Separate completed lifecycle regression of the same binary')
    args = parser.parse_args()
    assert args.min_seconds >= 7200, 'Soak acceptance requires at least two hours'
    assert args.max_growth_mib >= 0
    assert bool(args.shortened_close) == bool(args.lifecycle_report), 'Supply both follow-up records'
    manifest = json.loads(args.manifest.read_text())
    for name, expected in manifest['sourceSha256'].items():
        assert hashlib.sha256(Path(name).read_bytes()).hexdigest() == expected, name
    if args.shortened_close:
        close = json.loads(args.shortened_close.read_text())
        assert all(close[key] for key in ['windowCloseProtocol', 'realConfirmationAccepted', 'applicationExited'])
        assert close['sshRemaining'] == 0 and close['originalProbeCompleted'] is False
        data = json.loads(args.lifecycle_report.read_text())
        result = check_lifecycle(data, manifest, 20)
        samples = [json.loads(line) for line in args.report.read_text().splitlines()]
        excluded = []
        if samples[-1]['cpuPercentOneCore'] is not None and samples[-1]['cpuPercentOneCore'] < 0:
            excluded.append(samples.pop())
            assert excluded[0]['fds'] < samples[-1]['fds'], 'Invalid shutdown sample'
            assert excluded[0]['threads'] < samples[-1]['threads'], 'Invalid shutdown sample'
        seconds = samples[-1]['seconds']
        summary = dict(
            acceptance='shortened-resource-run-with-separate-lifecycle',
            originalProbeCompleted=False, fullDurationLatencyAvailable=False,
            lifecycleReport=str(args.lifecycle_report), lifecycleResult=result,
            closeReport=str(args.shortened_close),
            excludedShutdownSamples=excluded,
            excludedSampleReason='SSH children already exited; process-tree CPU counter reset. Raw JSONL preserved.')
    else:
        data = json.loads(args.report.read_text())
        result = check_lifecycle(data, manifest, args.min_seconds)
        samples = data['streaming']
        seconds = result['seconds']
        summary = dict(acceptance='completed-soak', result=result, fixture=data['fixture'])
    summary.update(check_resources(samples, seconds, args.min_seconds, args.max_growth_mib))
    summary.update(
        binarySha256=manifest['binarySha256'], environment=manifest['environment'],
        seconds=seconds, minimumSeconds=args.min_seconds, sourceHashesVerified=True,
        sshRemaining=0,
        notes='Fixed loopback workload with programmatic input and scrolling; not all production servers, physical input, Wayland, Windows or GPU memory.')
    args.output.write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary))


if __name__ == '__main__':
    main()
