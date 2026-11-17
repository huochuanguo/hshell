#!/usr/bin/env python3
"""Finish only the test-owned Xvfb window after a user-shortened soak.

The original fixed-duration probe cannot jump into its final stages. Preserve
streaming samples, exercise the real window close and record the intentional
probe abort separately from a subsequent complete lifecycle regression.
"""
import ctypes as c
import ctypes.util
import json
import os
from pathlib import Path
import time

assert Path('/.dockerenv').exists(), 'Only use inside the disposable soak container'
pid=22
assert Path(f'/proc/{pid}/comm').read_text().strip().startswith('hshell-soak-0.2')
env=dict(item.split(b'=',1) for item in Path(f'/proc/{pid}/environ').read_bytes().split(b'\0') if b'=' in item)
for name in ['DISPLAY','XAUTHORITY']:
    if name.encode() in env:os.environ[name]=os.fsdecode(env[name.encode()])
from x11_window import X11Windows, Window, Atom, Display, Attributes
windows=X11Windows();x=windows.x
main=windows.find(pid);assert main,'No mapped test window'
class Data(c.Union):_fields_=[('b',c.c_char*20),('s',c.c_short*10),('l',c.c_long*5)]
class Client(c.Structure):_fields_=[('type',c.c_int),('serial',c.c_ulong),('send_event',c.c_int),('display',Display),('window',Window),('message_type',Atom),('format',c.c_int),('data',Data)]
class Event(c.Union):_fields_=[('client',Client),('pad',c.c_long*24)]
x.XSendEvent.argtypes=[Display,Window,c.c_int,c.c_long,c.POINTER(Event)]
x.XFlush.argtypes=[Display]
x.XTranslateCoordinates.argtypes=[Display,Window,Window,c.c_int,c.c_int,c.POINTER(c.c_int),c.POINTER(c.c_int),c.POINTER(Window)]
message=Event();message.client=Client(type=33,display=windows.display,window=main['id'],message_type=windows.atom(b'WM_PROTOCOLS'),format=32)
message.client.data.l[0]=windows.atom(b'WM_DELETE_WINDOW')
assert x.XSendEvent(windows.display,main['id'],0,0,c.byref(message))
x.XFlush(windows.display)
dialog_type=windows.atom(b'_NET_WM_WINDOW_TYPE_DIALOG')
def dialog(window):
    if windows.property32(window,windows.pid_atom,6)==[pid] and dialog_type in windows.property32(window,windows.type_atom,4):return window
    root,parent=Window(),Window();children=c.POINTER(Window)();count=c.c_uint()
    if not x.XQueryTree(windows.display,window,c.byref(root),c.byref(parent),c.byref(children),c.byref(count)):return None
    try:
        for i in range(count.value):
            found=dialog(children[i])
            if found:return found
    finally:
        if children:x.XFree(children)
    return None
started=time.monotonic();confirmation=None
while time.monotonic()-started<5:
    confirmation=dialog(windows.root)
    if confirmation:break
    time.sleep(.02)
assert confirmation,'No real close confirmation'
attrs=Attributes();assert x.XGetWindowAttributes(windows.display,confirmation,c.byref(attrs))
xroot,yroot,child=c.c_int(),c.c_int(),Window()
assert x.XTranslateCoordinates(windows.display,confirmation,windows.root,attrs.width-55,attrs.height-25,c.byref(xroot),c.byref(yroot),c.byref(child))
xt=c.CDLL(ctypes.util.find_library('Xtst'))
xt.XTestFakeMotionEvent.argtypes=[Display,c.c_int,c.c_int,c.c_int,c.c_ulong]
xt.XTestFakeButtonEvent.argtypes=[Display,c.c_uint,c.c_int,c.c_ulong]
assert xt.XTestFakeMotionEvent(windows.display,-1,xroot.value,yroot.value,0)
assert xt.XTestFakeButtonEvent(windows.display,1,1,0)
assert xt.XTestFakeButtonEvent(windows.display,1,0,0)
x.XFlush(windows.display)
def ssh_children():
    result=[]
    for entry in Path('/proc').iterdir():
        try:
            if (entry/'comm').read_text().strip()=='ssh':result.append(int(entry.name))
        except OSError:pass
    return result
while time.monotonic()-started<15 and (Path(f'/proc/{pid}').exists() or ssh_children()):time.sleep(.05)
remaining=ssh_children()
assert not remaining,remaining
assert not Path(f'/proc/{pid}').exists(),'Development probe did not finish after the intentional close'
windows.close()
output=dict(reason='User shortened duration to two or three hours',windowCloseProtocol=True,realConfirmationAccepted=True,applicationExited=True,sshRemaining=0,closeToExitMs=round((time.monotonic()-started)*1000),originalProbeCompleted=False,notes='Fixed 8-hour probe reports intentional interruption after its terminal was closed. This is not an 8-hour pass and contains no full-duration latency histogram. Subsequent lifecycle regression is separate.')
Path('/workspace/docs/native-soak-0.2.3-shortened-close.json').write_text(json.dumps(output,indent=2)+'\n')
print(json.dumps(output))
