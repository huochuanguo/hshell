"""使用回环 Paramiko 服务器验证 Windows OpenSSH SFTP，不访问用户 SSH 配置。"""
import argparse
import errno
import hashlib
import json
import os
from pathlib import Path
import posixpath
import socket
import subprocess
import tempfile
import threading

import paramiko


class Handle(paramiko.SFTPHandle):
    def stat(self):
        return paramiko.SFTPAttributes.from_stat(os.fstat(self.readfile.fileno()))

    def chattr(self, attr):
        try:
            if attr.st_size is not None:
                self.writefile.truncate(attr.st_size)
            return paramiko.SFTP_OK
        except OSError as error:
            print(f'夹具截断失败：{error}', flush=True)
            return paramiko.SFTPServer.convert_errno(error.errno)

    def read(self, offset, length):
        result = super().read(offset, length)
        if isinstance(result, int) and result != paramiko.SFTP_EOF:
            print(f'夹具读取失败：offset={offset}, status={result}', flush=True)
        return result


class Files(paramiko.SFTPServerInterface):
    def __init__(self, server, *args, root, **kwargs):
        super().__init__(server, *args, **kwargs)
        self.root = Path(root).resolve()

    def canonicalize(self, path):
        return posixpath.normpath(path if path.startswith('/') else '/data/' + path)

    def local(self, path):
        remote = self.canonicalize(path)
        local = self.root.joinpath(*remote.split('/')).resolve()
        if not local.is_relative_to(self.root):
            raise OSError(errno.EACCES, '路径超出夹具目录')
        return local

    def stat(self, path):
        try:
            return paramiko.SFTPAttributes.from_stat(self.local(path).stat())
        except OSError as error:
            return paramiko.SFTPServer.convert_errno(error.errno)

    lstat = stat

    def list_folder(self, path):
        try:
            result = []
            for entry in self.local(path).iterdir():
                attr = paramiko.SFTPAttributes.from_stat(entry.stat())
                attr.filename = entry.name
                result.append(attr)
            return result
        except OSError as error:
            return paramiko.SFTPServer.convert_errno(error.errno)

    def open(self, path, flags, attr):
        try:
            descriptor = os.open(self.local(path), flags | getattr(os, 'O_BINARY', 0), 0o600)
            mode = 'r+b' if flags & os.O_RDWR else 'wb' if flags & os.O_WRONLY else 'rb'
            file = os.fdopen(descriptor, mode, buffering=0)
            handle = Handle(flags)
            handle.readfile = file
            handle.writefile = file
            return handle
        except OSError as error:
            return paramiko.SFTPServer.convert_errno(error.errno)

    def operation(self, action, *paths):
        try:
            action(*(self.local(path) for path in paths))
            return paramiko.SFTP_OK
        except OSError as error:
            print(f'夹具 {action.__name__} 失败：{error}', flush=True)
            return paramiko.SFTPServer.convert_errno(error.errno)

    def mkdir(self, path, attr):
        return self.operation(os.mkdir, path)

    def rmdir(self, path):
        return self.operation(os.rmdir, path)

    def remove(self, path):
        return self.operation(os.remove, path)

    def rename(self, old, new):
        if self.local(new).exists():
            return paramiko.SFTP_FAILURE
        return self.operation(os.rename, old, new)

    def posix_rename(self, old, new):
        return self.operation(os.replace, old, new)


class Authentication(paramiko.ServerInterface):
    def get_allowed_auths(self, username):
        return 'password'

    def check_auth_password(self, username, password):
        return paramiko.AUTH_SUCCESSFUL if (username, password) == ('fixture', 'fixture-only') else paramiko.AUTH_FAILED

    def check_channel_request(self, kind, channel_id):
        return paramiko.OPEN_SUCCEEDED if kind == 'session' else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', default='target/windows-sftp.json')
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='hshell-windows-sftp-') as directory:
        root = Path(directory)
        (root / 'remote/data').mkdir(parents=True)
        key = paramiko.RSAKey.generate(2048)
        listener = socket.socket()
        listener.bind(('127.0.0.1', 0))
        listener.listen(8)
        listener.settimeout(0.2)
        port = listener.getsockname()[1]
        transports = []
        stop = threading.Event()

        def serve(client):
            transport = paramiko.Transport(client)
            transports.append(transport)
            try:
                transport.add_server_key(key)
                transport.set_subsystem_handler('sftp', paramiko.SFTPServer, Files, root=root / 'remote')
                transport.start_server(server=Authentication())
                while transport.is_active() and not stop.wait(0.1):
                    pass
            except (EOFError, OSError, paramiko.SSHException):
                pass
            finally:
                transport.close()

        def accept():
            while not stop.is_set():
                try:
                    client, _ = listener.accept()
                    threading.Thread(target=serve, args=(client,), daemon=True).start()
                except socket.timeout:
                    continue
                except OSError:
                    break

        threading.Thread(target=accept, daemon=True).start()
        known_hosts = root / 'known_hosts'
        known_hosts.write_text(f'[127.0.0.1]:{port} ssh-rsa {key.get_base64()}\n', encoding='utf-8')
        empty = root / 'empty'
        empty.touch()
        config = root / 'config'
        config.write_text(f'''Host fixture
  HostName 127.0.0.1
  Port {port}
  User fixture
  PubkeyAuthentication no
  IdentityAgent none
  UserKnownHostsFile "{known_hosts.as_posix()}"
  GlobalKnownHostsFile "{empty.as_posix()}"
  LogLevel ERROR
''', encoding='utf-8')
        try:
            subprocess.run(['cargo', 'build', '-p', 'hshell-sftp', '--example', 'windows_verify', '--features', 'windows-fixture', '--locked'], check=True)
            binary = Path('target/debug/examples/windows_verify' + ('.exe' if os.name == 'nt' else '')).resolve()
            phases = ['pause-upload', 'resume-upload-pause-download', 'resume-download']
            for phase in phases:
                subprocess.run([str(binary), str(config), str(root), phase], check=True, timeout=120)
            original = root / '中文 上传文件.bin'
            downloaded = root / '中文 下载文件.bin'
            checksum = hashlib.sha256(original.read_bytes()).hexdigest()
            assert hashlib.sha256(downloaded.read_bytes()).hexdigest() == checksum
            result = {'platform': os.name, 'bytes': original.stat().st_size, 'sha256': checksum, 'phases': phases,
                      'passed': ['saved-password', 'unicode-paths', 'upload-restart-resume', 'download-restart-resume', 'prefix-verification', 'sha256-roundtrip', 'list-mkdir-rename-remove', 'refuse-existing-rename'],
                      'limitations': ['回环 Paramiko 夹具，未替代生产服务器、跳板机或长时验收']}
            output = Path(args.output)
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
            print(json.dumps(result, ensure_ascii=False))
        finally:
            stop.set()
            listener.close()
            for transport in transports:
                transport.close()


if __name__ == '__main__':
    main()
