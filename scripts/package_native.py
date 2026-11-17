#!/usr/bin/env python3
"""Package the production Linux frontend. Build on Ubuntu 22.04 for 22/24."""
import argparse
import hashlib
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary')
    parser.add_argument('--output', help='Defaults to target/release/bundle/deb/hshell_VERSION_amd64.deb')
    args = parser.parse_args()
    source = Path(__file__).resolve().parent.parent
    binary = Path(args.binary).resolve()
    version = re.search(r'^version = "([0-9.]+)"$', (source / 'crates/native-linux/Cargo.toml').read_text(), re.M)[1]
    output = Path(args.output or source / f'target/release/bundle/deb/hshell_{version}_amd64.deb').resolve()
    payload = binary.read_bytes()
    assert b'HSHELL_NATIVE_PROBE' not in payload, 'Development probe must never ship'
    assert b'libwebkit' not in payload.lower(), 'Native package unexpectedly links WebKit'
    headers = subprocess.check_output(['readelf', '-h', str(binary)], text=True)
    assert 'Advanced Micro Devices X86-64' in headers, 'Only amd64 package currently supported'
    with tempfile.TemporaryDirectory(prefix='hshell-native-package-') as directory:
        root = Path(directory)
        package = root / 'debian/hshell'
        for folder in ['DEBIAN', 'usr/bin', 'usr/lib/hshell/fonts', 'usr/share/applications',
                       'usr/share/icons/hicolor/128x128/apps', 'usr/share/doc/hshell']:
            (package / folder).mkdir(parents=True)
        executable = package / 'usr/bin/hshell'
        shutil.copyfile(binary, executable)
        executable.chmod(0o755)
        for font in (source / 'native-assets/fonts').glob('*.ttf'):
            shutil.copyfile(font, package / 'usr/lib/hshell/fonts' / font.name)
        for license_file in (source / 'LICENSES').glob('*.txt'):
            shutil.copyfile(license_file, package / 'usr/share/doc/hshell' / license_file.name)
        shutil.copyfile(source / 'native-assets/fonts/README.md', package / 'usr/share/doc/hshell/fonts.md')
        for size in [16, 24, 32, 48, 64, 128, 256, 512]:
            icon_name = '128x128@2x.png' if size == 256 else f'{size}x{size}.png'
            destination = package / f'usr/share/icons/hicolor/{size}x{size}/apps'
            destination.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source / 'src-tauri/icons' / icon_name, destination / 'hshell.png')
        scalable = package / 'usr/share/icons/hicolor/scalable/apps'
        scalable.mkdir(parents=True)
        shutil.copyfile(source / 'src/assets/hshell.svg', scalable / 'hshell.svg')
        (package / 'usr/share/applications/hshell.desktop').write_text('''[Desktop Entry]
Type=Application
Name=HShell
Name[zh_CN]=HShell
Comment=Native OpenSSH terminal workspace
Exec=hshell
Icon=hshell
Terminal=false
Categories=Network;RemoteAccess;Development;
StartupWMClass=Hshell
''')
        # Let the baseline distro compute actual direct shared-library dependencies.
        (root / 'debian/control').write_text('Source: hshell\n\nPackage: hshell\nArchitecture: amd64\n')
        (package / 'DEBIAN/control').write_text(f'Package: hshell\nVersion: {version}\nArchitecture: amd64\n')
        dependencies = subprocess.check_output(['dpkg-shlibdeps', '-O', '-e' + str(executable.relative_to(root))], cwd=root, text=True)
        depends = next(line.split('=', 1)[1] for line in dependencies.splitlines() if line.startswith('shlibs:Depends='))
        assert 'libvte-2.91-0' in depends and 'libwebkit' not in depends, depends
        size = sum(file.stat().st_size for file in package.rglob('*') if file.is_file())
        (package / 'DEBIAN/control').write_text(f'''Package: hshell
Version: {version}
Section: net
Priority: optional
Architecture: amd64
Maintainer: HShell Maintainers <maintainers@hshell.invalid>
Installed-Size: {(size + 1023) // 1024}
Depends: openssh-client, {depends}
Recommends: gnome-keyring, fonts-noto-cjk
Conflicts: qizhan-ssh
Replaces: qizhan-ssh
Description: Native OpenSSH terminal workspace
 A local GTK and VTE client with saved connections, independent terminal
 tabs, system credential storage, and direct PTY rendering.
''')
        output.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(['dpkg-deb', '--root-owner-group', '-Zxz', '--build', str(package), str(output)], check=True)
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    output.with_suffix(output.suffix + '.sha256').write_text(f'{digest}  {output.name}\n')
    print(f'{output}\nSHA256: {digest}')


if __name__ == '__main__':
    main()
