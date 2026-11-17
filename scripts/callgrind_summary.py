#!/usr/bin/env python3
"""Summarize self instruction counts, excluding calls even when their count is 0.

Zeroing counters while a stack is active produces calls=0 with nonzero inclusive
cost. That cost belongs to callees and must not be attributed to the caller.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re


def summarize(path):
    names = {'ob': {}, 'fn': {}}
    objects, functions = Counter(), Counter()
    current_object, current_function = 'unknown', 'unknown'
    call_cost = False
    summary = None
    reported_total = None
    for line in path.read_text().splitlines():
        if line.startswith('events:'):
            assert line == 'events: Ir', 'Only instruction-only profiles supported'
        elif line.startswith('positions:'):
            assert line == 'positions: line'
        elif line.startswith('summary:'):
            summary = int(line.split()[1])
        elif line.startswith('totals:'):
            reported_total = int(line.split()[1])
        elif match := re.match(r'^(c?ob|c?fn)=\((\d+)\)(?: (.*))?$', line):
            kind, index, name = match.groups()
            family = kind.removeprefix('c')
            if name is not None:
                names[family][index] = name
            name = names[family][index]
            if kind == 'ob':
                current_object = name
            elif kind == 'fn':
                current_function = name
        elif line.startswith('calls='):
            call_cost = True
        elif re.match(r'^(?:[+\-]?\d+|\*)\s+\d+', line):
            if call_cost:
                call_cost = False
            else:
                cost = int(line.split()[1])
                objects[current_object] += cost
                functions[(current_object, current_function)] += cost
    total = sum(objects.values())
    assert total == (reported_total if reported_total is not None else summary), (total, reported_total, summary)
    return dict(totalInstructions=total, reportedSummary=summary,
                objects=[dict(object=key, instructions=value, percent=round(value/total*100, 2)) for key, value in objects.most_common()],
                topFunctions=[dict(object=key[0], function=key[1], instructions=value, percent=round(value/total*100, 2)) for key, value in functions.most_common(15)])


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('profile', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = summarize(args.profile)
    result.update(profileSha256=hashlib.sha256(args.profile.read_bytes()).hexdigest(),
                  notes='Self instruction counts during two-session loopback output under Callgrind. Not wall-time CPU shares or uninstrumented performance. No frequent text extraction during scoped sample.')
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result['objects'][:8]))
