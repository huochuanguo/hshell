#!/usr/bin/env python3
"""Validate a release tag against the native app version before building."""
import argparse
import os
from pathlib import Path
import re


def metadata(tag, root):
    if not re.fullmatch(r'v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', tag):
        raise ValueError('Release tag must be vMAJOR.MINOR.PATCH without leading zeros')
    manifest = (root / 'crates/native-linux/Cargo.toml').read_text()
    match = re.search(r'^version = "([0-9.]+)"$', manifest, re.M)
    if not match or tag != 'v' + match[1]:
        raise ValueError('Release tag does not match crates/native-linux/Cargo.toml')
    version = match[1]
    notes = f'docs/releases/{version}.md'
    if not (root / notes).is_file() or not (root / notes).read_text().strip():
        raise ValueError(f'Missing release notes: {notes}')
    return {'version': version, 'package': f'hshell_{version}_amd64.deb', 'notes': notes}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('tag')
    args = parser.parse_args()
    try:
        values = metadata(args.tag, Path(__file__).resolve().parent.parent)
    except ValueError as error:
        parser.exit(1, f'{error}\n')
    output = ''.join(f'{key}={value}\n' for key, value in values.items())
    print(output, end='')
    if os.environ.get('GITHUB_OUTPUT'):
        with open(os.environ['GITHUB_OUTPUT'], 'a') as stream:
            stream.write(output)


if __name__ == '__main__':
    main()
