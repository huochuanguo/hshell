#!/usr/bin/env python3
"""Select an engine on an existing *isolated* IBus context, never the desktop bus."""
import os
from pathlib import Path
import time
import gi
gi.require_version('IBus','1.0')
from gi.repository import IBus, GLib


def main():
    address=os.environ['IBUS_ADDRESS']
    runtime=Path(os.environ['XDG_RUNTIME_DIR'])
    assert str(runtime).startswith('/tmp/hshell-native-keyring-') and address=='unix:path='+str(runtime/'ibus')
    IBus.init()
    bus=IBus.Bus()
    assert bus.is_connected(), 'Isolated IBus is not connected'
    context=bus.create_input_context('hshell-bootstrap')
    context.set_capabilities(IBus.Capabilite.PREEDIT_TEXT | IBus.Capabilite.FOCUS)
    context.focus_in()
    context.set_engine('libpinyin')
    deadline=time.monotonic()+8
    main_context=GLib.MainContext.default()
    while time.monotonic()<deadline:
        while main_context.pending():main_context.iteration(False)
        engine=context.get_engine()
        if engine and engine.get_name()=='libpinyin':
            assert bus.set_global_engine('libpinyin')
            return
        time.sleep(.02)
    raise AssertionError('Isolated context did not select libpinyin')


if __name__=='__main__':main()
