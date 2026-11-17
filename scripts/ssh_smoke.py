#!/usr/bin/env python3
"""Loopback-only SSH integration fixture; requires Python paramiko.

Creates disposable keys and never reads/writes the user's SSH configuration.
The fixture accepts one known test password/key and does not execute commands.
"""
import base64
import hashlib
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading

import paramiko


class Fixture(paramiko.ServerInterface):
    def __init__(self, public_key):
        self.public_key = public_key
        self.shell = threading.Event()
        self.size = (80, 24)

    def get_allowed_auths(self, username):
        return "publickey,password"

    def check_auth_password(self, username, password):
        return paramiko.AUTH_SUCCESSFUL if (username, password) == ("fixture", "fixture-only") else paramiko.AUTH_FAILED

    def check_auth_publickey(self, username, key):
        return paramiko.AUTH_SUCCESSFUL if username == "fixture" and key == self.public_key else paramiko.AUTH_FAILED

    def check_channel_request(self, kind, channel_id):
        return paramiko.OPEN_SUCCEEDED if kind == "session" else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_channel_pty_request(self, channel, term, width, height, *args):
        self.size = (width, height)
        return True

    def check_channel_window_change_request(self, channel, width, height, *args):
        self.size = (width, height)
        return True

    def check_channel_shell_request(self, channel):
        self.shell.set()
        return True


def serve(client, host_key, public_key):
    transport = paramiko.Transport(client)
    try:
        transport.add_server_key(host_key)
        server = Fixture(public_key)
        transport.start_server(server=server)
        channel = transport.accept(10)
        if channel is None or not server.shell.wait(10):
            return
        channel.settimeout(10)
        channel.sendall("fixture-ready 中文终端\r\n".encode())
        line = b""
        while True:
            data = channel.recv(8192)
            if not data:
                break
            line += data
            if b"\n" in line:
                channel.sendall(b"fixture-received:" + line + b"\r\n")
                channel.send_exit_status(0)
                channel.shutdown_write()
                channel.close()
                transport.join(timeout=2)  # Let OpenSSH receive exit-status and close cleanly.
                break
    except (EOFError, OSError, paramiko.SSHException):
        pass
    finally:
        transport.close()


def main():
    with tempfile.TemporaryDirectory(prefix="hshell-test-") as directory:
        host_key = paramiko.RSAKey.generate(2048)
        user_key = paramiko.RSAKey.generate(2048)
        wrong_key = paramiko.RSAKey.generate(2048)
        key_path = str(Path(directory) / "id_rsa")
        user_key.write_private_key_file(key_path)
        os.chmod(key_path, 0o600)
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen(8)

        def accept():
            try:
                while True:
                    client, _ = listener.accept()
                    threading.Thread(target=serve, args=(client, host_key, user_key), daemon=True).start()
            except OSError:
                return

        threading.Thread(target=accept, daemon=True).start()
        env = os.environ.copy()
        env.update({
            "HSHELL_TEST_PORT": str(listener.getsockname()[1]),
            "HSHELL_TEST_KEY": key_path,
            "HSHELL_TEST_FINGERPRINT": "SHA256:" + base64.b64encode(hashlib.sha256(host_key.asbytes()).digest()).decode().rstrip("="),
            "HSHELL_TEST_WRONG_KEY": wrong_key.get_base64(),
        })
        subprocess.run(["cargo", "build", "-p", "ssh-core", "--example", "askpass", "--locked"], check=True)
        env["HSHELL_TEST_ASKPASS"] = str(Path("target/debug/examples/askpass").resolve())
        try:
            result = subprocess.run(["cargo", "test", "-p", "ssh-core", "openssh_fixture", "--", "--ignored", "--nocapture"], env=env, timeout=90)
        finally:
            listener.close()
        raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
