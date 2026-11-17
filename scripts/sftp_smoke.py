#!/usr/bin/env python3
"""Real OpenSSH SFTP validation in a disposable loopback-only container."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import time
from native_memory import memory


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary')
    parser.add_argument('--output',required=True)
    parser.add_argument('--gui')
    parser.add_argument('--gui-only',action='store_true')
    parser.add_argument('--authentication-only',action='store_true')
    parser.add_argument('--appearance-only',action='store_true')
    args=parser.parse_args()
    if args.appearance_only:
        assert args.gui,'Appearance verification requires --gui'
        args.gui_only=True
    binary=Path(args.binary).resolve()
    with tempfile.TemporaryDirectory(prefix='hshell-sftp-') as directory:
        root=Path(directory);root.chmod(0o755);(root/'data/large').mkdir(parents=True)
        for i in range(10000):(root/'data/large'/f'file-{i:05d}').touch()
        appearance=root/'data/appearance';appearance.mkdir()
        for name in ['configs','logs']:(appearance/name).mkdir()
        for name in ['README.txt','app.toml','deploy.sh','backup.tar.gz','logo.png']:(appearance/name).write_text('fixture\n')
        (appearance/'current-config').symlink_to('app.toml')
        for name in ['host','user']:subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(root/name)],check=True)
        (root/'sshd_config').write_text('''Port 22
ListenAddress 0.0.0.0
HostKey /fixture/host
AuthorizedKeysFile /fixture/user.pub
PermitRootLogin yes
PubkeyAuthentication yes
PasswordAuthentication yes
KbdInteractiveAuthentication no
UsePAM no
StrictModes no
PrintMotd no
Subsystem sftp internal-sftp
''')
        container=subprocess.check_output(['docker','run','-d','--rm','--init','--tmpfs','/limited:rw,size=1m,mode=1777','-p','127.0.0.1::22','-v',str(root)+':/fixture','hshell-test:programs-ubuntu22','sh','-c','mkdir -p /run/sshd\nexec /usr/sbin/sshd -D -e -f /fixture/sshd_config'],text=True).strip()
        paused=False
        try:
            subprocess.run(['docker','exec','-i',container,'chpasswd'],input='root:fixture-only-test-123\n',text=True,check=True)
            port=int(subprocess.check_output(['docker','port',container,'22/tcp'],text=True).strip().rsplit(':',1)[1]);(root/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(root/'host.pub').read_text())
            config=root/'ssh-config';config.write_text(f'''Host fixture
  HostName 127.0.0.1
  Port {port}
  User root
  IdentityFile {root/'user'}
  IdentitiesOnly yes
  IdentityAgent none
  UserKnownHostsFile {root/'known_hosts'}
  GlobalKnownHostsFile /dev/null
  LogLevel ERROR
Host fixture-password
  HostName 127.0.0.1
  Port {port}
  User root
  PubkeyAuthentication no
  UserKnownHostsFile {root/'known_hosts_password'}
  GlobalKnownHostsFile /dev/null
  LogLevel ERROR
''')
            bindir=root/'bin';bindir.mkdir();wrapper=bindir/'ssh';wrapper.write_text('#!/bin/sh\nexec /usr/bin/ssh -F '+shlex.quote(str(config))+' "$@"\n');wrapper.chmod(0o700)
            env=os.environ.copy();env.update(PATH=str(bindir)+os.pathsep+env['PATH'],HSHELL_SFTP_ROOT=str(root),XDG_CONFIG_HOME=str(root/'config'),GTK_IM_MODULE='simple',NO_AT_BRIDGE='1')
            results=[]
            gui_hash=None
            def run(mode):
                process=subprocess.run([str(binary),mode],env=env,text=True,capture_output=True,timeout=90)
                assert process.returncode==0,(mode,process.stdout,process.stderr)
                value=json.loads(process.stdout.strip());results.append(value);print(json.dumps(value,ensure_ascii=False),flush=True)
            if not args.gui_only and not args.authentication_only:
                run('suite')
                for name in ['upload','download']:
                    run('pause-'+name);run('resume-'+name)
                    # Fresh task after the completed file: the old final file is
                    # removed only inside this disposable fixture.
                    (root/'data'/name).unlink() if name=='upload' else (root/(name+'.local')).unlink()
                    run('pause-'+name)
                    record=json.loads(next((root/('state-'+name)).glob('*.json')).read_text());partial=Path(record['partial'])
                    if name=='upload':partial=root/'data'/partial.name
                    if name=='upload':subprocess.run(['docker','exec',container,'chmod','a+rw','/fixture/data/'+partial.name],check=True)
                    with partial.open('r+b') as f:f.write(b'tampered-prefix')
                    run('reject-'+name)
                    # Check changed source identity as a separate rejection.
                    source=root/(name+'.local') if name=='upload' else root/'data'/name
                    with source.open('ab') as f:f.write(b'changed-source')
                    run('reject-'+name)
                process=subprocess.Popen([str(binary),'network'],env=env,text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
                try:
                    deadline=time.monotonic()+15
                    while not (root/'network-progress').exists():
                        assert process.poll() is None
                        assert time.monotonic()<deadline
                        time.sleep(.005)
                    subprocess.run(['docker','pause',container],check=True,stdout=subprocess.DEVNULL);paused=True
                    stdout,stderr=process.communicate(timeout=45);assert process.returncode==0,(stdout,stderr);results.append(json.loads(stdout));print(stdout,flush=True)
                finally:
                    if paused:subprocess.run(['docker','unpause',container],check=True,stdout=subprocess.DEVNULL);paused=False
                    if process.poll() is None:process.kill();process.wait()
                run('resume-network')
            if not args.gui_only:
                run('authentication')
            if args.gui:
                gui=Path(args.gui).resolve();result=root/'gui.json';snapshot=Path(args.output).resolve().with_suffix('.png')
                gui_hash=hashlib.sha256(gui.read_bytes()).hexdigest()
                env.update(HSHELL_NATIVE_PROBE='1',HSHELL_NATIVE_SFTP='1',HSHELL_NATIVE_RESULT=str(result),HSHELL_NATIVE_SNAPSHOT=str(snapshot))
                if args.appearance_only:env['HSHELL_NATIVE_SFTP_APPEARANCE']='1'
                gui_process=subprocess.Popen([str(gui)],env=env,text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
                samples={};deadline=time.monotonic()+120
                try:
                    while gui_process.poll() is None:
                        assert time.monotonic()<deadline,'GUI timed out'
                        try:
                            phase=json.loads(result.read_text())['phase']
                            sample=memory(gui_process.pid)
                            samples.setdefault(phase,[]).append({'pssMiB':sample['pssMiB'],'rssMiB':sample['rssMiB']})
                        except (OSError,ValueError,RuntimeError):pass
                        time.sleep(.1)
                    stdout,stderr=gui_process.communicate()
                    assert gui_process.returncode==0,(stdout,stderr)
                    assert 'WARNING' not in stderr and 'CRITICAL' not in stderr,stderr
                    measured=json.loads(result.read_text());assert measured['phase']=='complete',measured
                    measured['memorySamples']=samples
                    results.append(measured)
                finally:
                    if gui_process.poll() is None:gui_process.kill();gui_process.wait()
                if not args.appearance_only:
                    env['HSHELL_NATIVE_SFTP_RESTORE']='1'
                    restored=subprocess.run([str(gui)],env=env,text=True,capture_output=True,timeout=45)
                    assert restored.returncode==0,(restored.stdout,restored.stderr)
                    assert 'WARNING' not in restored.stderr and 'CRITICAL' not in restored.stderr,restored.stderr
                    measured=json.loads(result.read_text());assert measured['phase']=='complete' and measured['restoredAfterExit'],measured
                    results.append(measured)
                    def digest(path):
                        value=hashlib.sha256()
                        with path.open('rb') as f:
                            while block:=f.read(1024*1024):value.update(block)
                        return value.hexdigest()
                    assert digest(root/'gui-upload.local')==digest(root/'data/gui-upload')==digest(root/'gui-download.local')==digest(root/'data/gui-pause')==digest(root/'data/gui-exit')
            remaining=[]
            for proc in Path('/proc').iterdir():
                try:
                    argv=(proc/'cmdline').read_bytes().split(b'\0')
                    if str(config).encode() in argv and Path(os.fsdecode(argv[0])).name=='ssh':remaining.append(proc.name)
                except (OSError,ValueError):pass
            assert not remaining,remaining
            output={'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'guiBinarySha256':gui_hash,'results':results,'sshRemaining':0,'osRelease':Path('/etc/os-release').read_text()}
            Path(args.output).write_text(json.dumps(output,indent=2,ensure_ascii=False)+'\n')
        finally:
            if paused:subprocess.run(['docker','unpause',container],stdout=subprocess.DEVNULL)
            subprocess.run(['docker','stop','-t','2',container],stdout=subprocess.DEVNULL,check=False)


if __name__=='__main__':main()
