#!/usr/bin/env python3
"""Real vim/tmux via disposable loopback sshd, never a user's SSH destination."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import socket
import subprocess
import tempfile
import time


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary')
    parser.add_argument('--output',required=True)
    parser.add_argument('--image',default='hshell-test:programs-ubuntu22')
    parser.add_argument('--local-server',action='store_true',help='Disposable container only: run sshd in this namespace')
    args=parser.parse_args()
    binary=Path(args.binary).resolve()
    with tempfile.TemporaryDirectory(prefix='hshell-programs-') as d:
        root=Path(d);root.chmod(0o755)
        for name in ['host','user']:
            subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(root/name)],check=True)
        (root/'sshd_config').write_text('''Port 22
ListenAddress 0.0.0.0
HostKey /fixture/host
AuthorizedKeysFile /fixture/user.pub
PermitRootLogin yes
PubkeyAuthentication yes
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
StrictModes no
PrintMotd no
ForceCommand /fixture/shell
''')
        (root/'shell').write_text('#!/bin/sh\nexport LANG=C.UTF-8 PS1="fixture-ready> "\nexec /bin/bash --noprofile --norc -i\n')
        (root/'shell').chmod(0o755)
        container=None;process=None;server=None
        if args.local_server:
            assert os.geteuid()==0 and Path('/.dockerenv').is_file(), 'Local sshd fixture is container-only'
            assert not Path('/fixture').exists()
            Path('/fixture').symlink_to(root,target_is_directory=True)
            Path('/run/sshd').mkdir(exist_ok=True)
        try:
            if args.local_server:
                with socket.socket() as listener:
                    listener.bind(('127.0.0.1',0));port=listener.getsockname()[1]
                conf=root/'sshd_config'
                conf.write_text(conf.read_text().replace('Port 22','Port '+str(port)).replace('ListenAddress 0.0.0.0','ListenAddress 127.0.0.1'))
                server=subprocess.Popen(['/usr/sbin/sshd','-D','-e','-f',str(conf)],stderr=subprocess.DEVNULL)
            else:
                container=subprocess.check_output(['docker','run','-d','--rm','--init','-p','127.0.0.1::22','-v',str(root)+':/fixture',args.image,'sh','-c','mkdir -p /run/sshd\nexec /usr/sbin/sshd -D -e -f /fixture/sshd_config'],text=True).strip()
                published=subprocess.check_output(['docker','port',container,'22/tcp'],text=True).strip()
                assert published.startswith('127.0.0.1:')
                port=int(published.rsplit(':',1)[1])
            (root/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(root/'host.pub').read_text())
            config=root/'ssh-config'
            config.write_text(f'''Host fixture
  HostName 127.0.0.1
  Port {port}
  User root
  IdentityFile {root/'user'}
  IdentitiesOnly yes
  IdentityAgent none
  UserKnownHostsFile {root/'known_hosts'}
  GlobalKnownHostsFile /dev/null
  BatchMode yes
  LogLevel ERROR
''')
            bindir=root/'bin';bindir.mkdir()
            wrapper=bindir/'ssh';wrapper.write_text('#!/bin/sh\nexec /usr/bin/ssh -F '+shlex.quote(str(config))+' "$@"\n');wrapper.chmod(0o700)
            result=root/'result.json'
            env=os.environ.copy();env.update(GTK_IM_MODULE='simple',XDG_CONFIG_HOME=str(root/'config'),PATH=str(bindir)+os.pathsep+env['PATH'],HSHELL_NATIVE_PROBE='1',HSHELL_NATIVE_PROGRAMS='1',HSHELL_PROGRAM_ROOT=str(root),HSHELL_NATIVE_RESULT=str(result))
            process=subprocess.Popen([str(binary)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
            deadline=time.monotonic()+90
            final=None
            while time.monotonic()<deadline:
                try:final=json.loads(result.read_text());break
                except (OSError,ValueError):
                    assert process.poll() is None, process.stderr.read().decode()
                    time.sleep(.05)
            assert final and final['phase']=='complete', final
            process.wait(timeout=10)
            assert process.returncode==0
            stderr=process.stderr.read().decode()
            assert 'WARNING' not in stderr and 'CRITICAL' not in stderr, stderr
            assert (root/'vim-check.txt').read_bytes()=='HShell 中文输入与保存 UTF-8\n'.encode()
            assert len((root/'tmux-windows').read_text().splitlines())==2
            rows,columns=map(int,(root/'terminal-size').read_text().split())
            assert rows>=10 and columns>=30
            for entry in Path('/proc').iterdir():
                try:
                    argv=(entry/'cmdline').read_bytes().split(b'\0')
                    assert not (str(config).encode() in argv and Path(os.fsdecode(argv[0])).name=='ssh'), 'SSH child survived'
                except (OSError,ValueError):pass
            command=['dpkg-query','-W','openssh-server','tmux','vim-tiny']
            versions=subprocess.check_output(command if args.local_server else ['docker','exec',container,*command],text=True)
            output=dict(binarySha256=hashlib.sha256(binary.read_bytes()).hexdigest(),result=final,versions=versions,terminalSize=[rows,columns],sshRemaining=0,stderr=stderr)
            Path(args.output).write_text(json.dumps(output,indent=2,ensure_ascii=False)+'\n')
            print(json.dumps(output,ensure_ascii=False))
        finally:
            if process and process.poll() is None:
                process.terminate()
                try:process.wait(timeout=5)
                except subprocess.TimeoutExpired:process.kill();process.wait()
            if server:server.terminate();server.wait(timeout=5)
            if args.local_server:Path('/fixture').unlink()
            if container:subprocess.run(['docker','stop','-t','2',container],check=False,stdout=subprocess.DEVNULL)


if __name__=='__main__':main()
