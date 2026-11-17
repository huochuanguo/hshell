#!/usr/bin/env python3
"""Ask a production GTK window to close through X11's WM_DELETE_WINDOW protocol.
Targets only the test child PID. Isolates app configuration; needs a working DISPLAY.
"""
import argparse
import ctypes as c
import ctypes.util
import os
from pathlib import Path
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser()
parser.add_argument('binary')
args = parser.parse_args()
x = c.CDLL(ctypes.util.find_library('X11'))
Window = c.c_ulong
Atom = c.c_ulong
Display = c.c_void_p
x.XOpenDisplay.argtypes = [c.c_char_p]
x.XOpenDisplay.restype = Display
x.XDefaultRootWindow.argtypes = [Display]
x.XDefaultRootWindow.restype = Window
x.XInternAtom.argtypes = [Display, c.c_char_p, c.c_int]
x.XInternAtom.restype = Atom
x.XQueryTree.argtypes = [Display, Window, c.POINTER(Window), c.POINTER(Window), c.POINTER(c.POINTER(Window)), c.POINTER(c.c_uint)]
x.XGetWindowProperty.argtypes = [Display, Window, Atom, c.c_long, c.c_long, c.c_int, Atom, c.POINTER(Atom), c.POINTER(c.c_int), c.POINTER(c.c_ulong), c.POINTER(c.c_ulong), c.POINTER(c.POINTER(c.c_ubyte))]
x.XFree.argtypes = [c.c_void_p]
x.XFlush.argtypes = [Display]
x.XCloseDisplay.argtypes = [Display]

class Data(c.Union):
    _fields_ = [('b', c.c_char * 20), ('s', c.c_short * 10), ('l', c.c_long * 5)]
class ClientMessage(c.Structure):
    _fields_ = [('type', c.c_int), ('serial', c.c_ulong), ('send_event', c.c_int),
                ('display', Display), ('window', Window), ('message_type', Atom),
                ('format', c.c_int), ('data', Data)]
class Event(c.Union):
    _fields_ = [('client', ClientMessage), ('pad', c.c_long * 24)]
x.XSendEvent.argtypes = [Display, Window, c.c_int, c.c_long, c.POINTER(Event)]
display = x.XOpenDisplay(None)
assert display, 'No X11 display'
root = x.XDefaultRootWindow(display)
pid_atom = x.XInternAtom(display, b'_NET_WM_PID', 0)
type_atom = x.XInternAtom(display, b'_NET_WM_WINDOW_TYPE', 0)
normal_atom = x.XInternAtom(display, b'_NET_WM_WINDOW_TYPE_NORMAL', 0)

def property32(window, atom, expected_type):
    actual = Atom()
    fmt = c.c_int()
    count = c.c_ulong()
    remaining = c.c_ulong()
    data = c.POINTER(c.c_ubyte)()
    result = x.XGetWindowProperty(display, window, atom, 0, 32, 0, expected_type,
                                  c.byref(actual), c.byref(fmt), c.byref(count), c.byref(remaining), c.byref(data))
    values = list(c.cast(data, c.POINTER(c.c_ulong))[:count.value]) if result == 0 and count.value and fmt.value == 32 else []
    if data:
        x.XFree(data)
    return values

def find(window, pid):
    # GTK also creates a hidden InputOnly group-leader window with this PID.
    # Only the normal top-level window receives the user's title-bar close.
    if property32(window, pid_atom, 6) == [pid] and normal_atom in property32(window, type_atom, 4):
        return window
    root_return, parent = Window(), Window()
    children = c.POINTER(Window)()
    count = c.c_uint()
    x.XQueryTree(display, window, c.byref(root_return), c.byref(parent), c.byref(children), c.byref(count))
    found = None
    for i in range(count.value):
        found = find(children[i], pid)
        if found:
            break
    if children:
        x.XFree(children)
    return found

with tempfile.TemporaryDirectory(prefix='hshell-release-close-') as directory:
    env = os.environ.copy()
    env['XDG_CONFIG_HOME'] = directory
    env['GDK_BACKEND'] = 'x11'
    process = subprocess.Popen([str(Path(args.binary).resolve())], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        deadline = time.monotonic() + 10
        window = None
        while time.monotonic() < deadline and process.poll() is None:
            window = find(root, process.pid)
            if window:
                break
            time.sleep(.05)
        assert window, 'Native main window not found'
        time.sleep(2)  # Allow the production GTK window to finish loading.
        event = Event()
        event.client = ClientMessage(type=33, display=display, window=window,
                                     message_type=x.XInternAtom(display, b'WM_PROTOCOLS', 0), format=32)
        event.client.data.l[0] = x.XInternAtom(display, b'WM_DELETE_WINDOW', 0)
        assert x.XSendEvent(display, window, 0, 0, c.byref(event)), 'Close event not sent'
        started = time.monotonic()
        x.XFlush(display)
        try:
            stdout, stderr = process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            raise AssertionError('Window ignored WM_DELETE_WINDOW and remained alive for 5 seconds')
        assert process.returncode == 0, (process.returncode, stderr.decode())
        print(f'PASS: production window closed and process exited with code 0 in {round((time.monotonic()-started)*1000)} ms')
        if stderr:
            print(stderr.decode())
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate()
        x.XCloseDisplay(display)
