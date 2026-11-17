"""Inspect only X11 window metadata for a test-owned process."""
import ctypes as c
import ctypes.util
import time


Window = c.c_ulong
Atom = c.c_ulong
Display = c.c_void_p


class XError(c.Structure):
    _fields_ = [
        ('type', c.c_int), ('display', Display), ('resourceid', c.c_ulong),
        ('serial', c.c_ulong), ('error_code', c.c_ubyte),
        ('request_code', c.c_ubyte), ('minor_code', c.c_ubyte),
    ]


class Attributes(c.Structure):
    _fields_ = [
        ('x', c.c_int), ('y', c.c_int), ('width', c.c_int), ('height', c.c_int),
        ('border_width', c.c_int), ('depth', c.c_int), ('visual', c.c_void_p),
        ('root', Window), ('window_class', c.c_int), ('bit_gravity', c.c_int),
        ('win_gravity', c.c_int), ('backing_store', c.c_int),
        ('backing_planes', c.c_ulong), ('backing_pixel', c.c_ulong),
        ('save_under', c.c_int), ('colormap', c.c_ulong),
        ('map_installed', c.c_int), ('map_state', c.c_int),
        ('all_event_masks', c.c_long), ('your_event_mask', c.c_long),
        ('do_not_propagate_mask', c.c_long), ('override_redirect', c.c_int),
        ('screen', c.c_void_p),
    ]


class X11Windows:
    def __init__(self):
        self.x = c.CDLL(ctypes.util.find_library('X11'))
        x = self.x
        x.XOpenDisplay.argtypes = [c.c_char_p]
        x.XOpenDisplay.restype = Display
        x.XDefaultRootWindow.argtypes = [Display]
        x.XDefaultRootWindow.restype = Window
        x.XInternAtom.argtypes = [Display, c.c_char_p, c.c_int]
        x.XInternAtom.restype = Atom
        x.XQueryTree.argtypes = [Display, Window, c.POINTER(Window), c.POINTER(Window), c.POINTER(c.POINTER(Window)), c.POINTER(c.c_uint)]
        x.XGetWindowProperty.argtypes = [Display, Window, Atom, c.c_long, c.c_long, c.c_int, Atom, c.POINTER(Atom), c.POINTER(c.c_int), c.POINTER(c.c_ulong), c.POINTER(c.c_ulong), c.POINTER(c.POINTER(c.c_ubyte))]
        x.XGetWindowAttributes.argtypes = [Display, Window, c.POINTER(Attributes)]
        x.XFree.argtypes = [c.c_void_p]
        x.XCloseDisplay.argtypes = [Display]
        x.XSetErrorHandler.argtypes = [c.c_void_p]
        x.XSetErrorHandler.restype = c.c_void_p
        self.display = x.XOpenDisplay(None)
        assert self.display, 'No X11 display'
        self.errors = []

        @c.CFUNCTYPE(c.c_int, Display, c.POINTER(XError))
        def on_error(display, event):
            # An unrelated desktop window may disappear during tree traversal.
            if event.contents.error_code != 3:  # BadWindow
                self.errors.append((event.contents.error_code, event.contents.request_code))
            return 0

        self.error_handler = on_error
        self.previous_handler = x.XSetErrorHandler(on_error)
        self.root = x.XDefaultRootWindow(self.display)
        self.pid_atom = self.atom(b'_NET_WM_PID')
        self.type_atom = self.atom(b'_NET_WM_WINDOW_TYPE')
        self.normal_atom = self.atom(b'_NET_WM_WINDOW_TYPE_NORMAL')

    def atom(self, name):
        return self.x.XInternAtom(self.display, name, 0)

    def check_errors(self):
        if self.errors:
            raise RuntimeError(f'Unexpected X11 error: {self.errors}')

    def property32(self, window, atom, expected_type):
        actual, fmt, count, remaining = Atom(), c.c_int(), c.c_ulong(), c.c_ulong()
        data = c.POINTER(c.c_ubyte)()
        result = self.x.XGetWindowProperty(self.display, window, atom, 0, 32, 0, expected_type,
                                          c.byref(actual), c.byref(fmt), c.byref(count), c.byref(remaining), c.byref(data))
        try:
            self.check_errors()
            if result == 0 and count.value and fmt.value == 32 and actual.value == expected_type:
                return list(c.cast(data, c.POINTER(c.c_ulong))[:count.value])
            return []
        finally:
            if data:
                self.x.XFree(data)

    def find(self, pid, window=None):
        window = self.root if window is None else window
        # Skip GTK's hidden group-leader window and require an actual mapped UI.
        if (self.property32(window, self.pid_atom, 6) == [pid]
                and self.normal_atom in self.property32(window, self.type_atom, 4)):
            attrs = Attributes()
            available = self.x.XGetWindowAttributes(self.display, window, c.byref(attrs))
            self.check_errors()
            if available and attrs.map_state == 2 and attrs.width > 0 and attrs.height > 0:
                return {'id': window, 'width': attrs.width, 'height': attrs.height}
        root_return, parent = Window(), Window()
        children = c.POINTER(Window)()
        count = c.c_uint()
        available = self.x.XQueryTree(self.display, window, c.byref(root_return), c.byref(parent), c.byref(children), c.byref(count))
        self.check_errors()
        if not available:
            return None
        try:
            for i in range(count.value):
                found = self.find(pid, children[i])
                if found:
                    return found
            return None
        finally:
            if children:
                self.x.XFree(children)

    def wait(self, process, timeout=10):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline and process.poll() is None:
            window = self.find(process.pid)
            if window:
                return window
            time.sleep(.05)
        raise AssertionError('Native mapped main window not found for test process')

    def close(self):
        if self.display:
            self.x.XCloseDisplay(self.display)
            self.display = None
            self.x.XSetErrorHandler(self.previous_handler)
