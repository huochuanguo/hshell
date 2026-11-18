#!/usr/bin/env python3
"""构建前校验标签与两个平台的安装包版本一致。"""
import argparse
import json
import os
from pathlib import Path
import re


def metadata(tag, root):
    if not re.fullmatch(r'v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', tag):
        raise ValueError('Release tag must be vMAJOR.MINOR.PATCH without leading zeros')
    version = tag[1:]
    for path in ('crates/native-linux/Cargo.toml', 'src-tauri/Cargo.toml'):
        manifest = (root / path).read_text(encoding='utf-8')
        match = re.search(r'^version = "([0-9.]+)"$', manifest, re.M)
        if not match or match[1] != version:
            raise ValueError(f'Release tag does not match {path}')
    for path in ('src-tauri/tauri.conf.json', 'package.json', 'package-lock.json'):
        config = json.loads((root / path).read_text(encoding='utf-8'))
        if config.get('version') != version:
            raise ValueError(f'Release tag does not match {path}')
        if path == 'package-lock.json' and config.get('packages', {}).get('', {}).get('version') != version:
            raise ValueError(f'Release tag does not match the root package in {path}')
    notes = f'docs/releases/{version}.md'
    if not (root / notes).is_file() or not (root / notes).read_text(encoding='utf-8').strip():
        raise ValueError(f'Missing release notes: {notes}')
    return {
        'version': version,
        'package': f'hshell_{version}_amd64.deb',
        'windows_package': f'HShell_{version}_x64-setup.exe',
        'notes': notes,
    }


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
