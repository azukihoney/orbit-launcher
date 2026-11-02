"""PID-scoped review regressions. Uses temporary samples; never moves the pointer.

Runs each case in an isolated process and writes results after every case.
No entry execution, real recycling, screenshots, or production paths.
"""
import argparse
import ctypes as C
from ctypes import wintypes as W
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import time
import traceback

spec = importlib.util.spec_from_file_location('native', Path(__file__).with_name('native_smoke.py'))
n = importlib.util.module_from_spec(spec)
spec.loader.exec_module(n)
u = n.u
k = n.k
u.SendMessageTimeoutW.argtypes = [W.HWND, W.UINT, W.WPARAM, W.LPARAM, W.UINT, W.UINT, C.POINTER(C.c_size_t)]
u.IsWindow.argtypes = [W.HWND]
u.EnumChildWindows.argtypes = [W.HWND, n.callback, W.LPARAM]


def send(hwnd, msg, w=0, l=0):
    result = C.c_size_t()
    assert u.SendMessageTimeoutW(hwnd, msg, w, l, 2, 1500, C.byref(result)), f'message {msg:x} failed: {C.get_last_error()}'
    return result.value


def post(hwnd, msg, w=0, l=0):
    assert u.PostMessageW(hwnd, msg, w, l)


class Case:
    def __init__(self, exe, directory, auto_hide=False):
        self.path = Path(directory) / 'config.json'
        cfg = n.sample()
        cfg['settings'].update(language='en', auto_hide=auto_hide)
        cfg['pages'][0]['items'] = []
        cfg['common'] = []
        self.path.write_text(json.dumps(cfg), encoding='utf-8')
        self.p = subprocess.Popen([str(exe), '--config', str(self.path), '--no-gesture'])
        self.main = n.wait(lambda: next(iter(n.windows(self.p.pid, 'OrbitRust-')), None))
        time.sleep(.25)

    def command(self, id, hwnd=None):
        hwnd = hwnd or self.main
        klass = C.create_unicode_buffer(100)
        u.GetClassNameW(hwnd, klass, 100)
        button = u.GetDlgItem(hwnd, id)
        # Windows 11 may expose a single OK button as IDCANCEL (2).
        if klass.value == '#32770' and id == 1 and not button:
            button = u.GetDlgItem(hwnd, 2)
        if klass.value == '#32770' and button:
            post(button, 0xf5)  # BM_CLICK, with real control notification
        else:
            post(hwnd, 0x111, id)
        time.sleep(.15)

    def dialog(self):
        return n.wait(lambda: next(iter(n.windows(self.p.pid, 'OrbitRustDialog')), None))

    def box(self, timeout=5):
        return n.wait(lambda: next((h for h in n.windows(self.p.pid, '#32770') if u.IsWindowVisible(h)), None), timeout)

    def alive(self):
        assert self.p.poll() is None, 'test process exited unexpectedly'

    def dismiss(self):
        for h in n.windows(self.p.pid, '#32770'):
            self.command(1, h)
            n.wait(lambda: not u.IsWindow(h))

    def menu(self):
        post(self.main, 0x8003, 0, 0x205)
        # A disabled owner may make TrackPopupMenuEx return without a visible menu.
        # The tray callback still passes through menu(), including its old busy reset.
        time.sleep(.3)
        post(self.main, 0x1f)  # WM_CANCELMODE ends this process's menu only
        time.sleep(.2)

    def finish(self):
        if self.p.poll() is None:
            self.p.terminate()
            self.p.wait(timeout=5)


def diagnostic_begin(c):
    c.command(109)
    box = c.box()
    c.command(6, box)
    n.wait(lambda: not n.windows(c.p.pid, '#32770'))


def close_during_diagnostic_completion(c):
    diagnostic_begin(c)
    c.command(100)
    d = c.dialog()
    box = c.box(24)  # genuine production 20-second completion path
    c.command(2, d)
    if u.IsWindow(box):
        c.command(1, box)
    n.wait(lambda: not n.windows(c.p.pid, 'OrbitRustDialog'))
    time.sleep(.3)
    post(c.main, 0x10)
    time.sleep(.35)
    c.alive()
    assert not u.IsWindowVisible(c.main), 'main cannot hide after dialog close'
    post(c.main, 0x8004)
    c.command(100)
    d = c.dialog()
    c.command(2, d)
    n.wait(lambda: not n.windows(c.p.pid, 'OrbitRustDialog'))
    c.command(107)
    assert c.p.wait(timeout=5) == 0


def close_while_app_borrowed(c):
    c.command(109)
    box = c.box()
    post(c.main, 0x10)
    time.sleep(.3)
    c.alive()
    assert u.IsWindow(c.main), 'WM_CLOSE destroyed borrowed main'
    c.command(7, box)
    c.command(100)
    c.dialog()


def quit_guard_across_menu(c):
    c.command(100)
    d = c.dialog()
    for menu in (False, True):
        if menu:
            c.menu()
        c.command(107)
        c.alive()
        assert u.IsWindow(d), 'quit destroyed active dialog'
        c.dismiss()


def add_target_fixed(c):
    c.command(101)
    d = c.dialog()
    c.command(102)
    n.text(u.GetDlgItem(d, 100), 'Inert sample')
    n.text(u.GetDlgItem(d, 101), r'C:\Example\never-run.cmd')
    c.command(1, d)
    n.wait(lambda: not n.windows(c.p.pid, 'OrbitRustDialog'))
    cfg = json.loads(c.path.read_text(encoding='utf-8'))
    assert len(cfg['pages'][0]['items']) == 1 and not cfg['common'], 'registration destination changed'


def diagnostic_rejected_while_dialog(c):
    c.command(100)
    d = c.dialog()
    c.command(109)
    assert not n.windows(c.p.pid, '#32770'), 'diagnostic confirmation allowed during editing'
    assert u.IsWindow(d) and not u.IsWindowEnabled(c.main)


def browse_quit_guard(c, browse_id=301, settings=False):
    c.command(100 if settings else 101)
    d = c.dialog()
    c.command(browse_id, d)
    box = c.box()
    c.menu()
    c.command(107)
    c.alive()
    assert u.IsWindow(d) and u.IsWindow(box), 'quit destroyed active selection or parent'
    # A rejection message may be owned by the selection; dismiss only that one.
    for h in n.windows(c.p.pid, '#32770'):
        if h != box:
            c.command(1, h)
    c.command(2, box)
    n.wait(lambda: not u.IsWindow(box))
    assert u.IsWindow(d)
    c.command(2, d)
    n.wait(lambda: not n.windows(c.p.pid, 'OrbitRustDialog'))


def browse_parent_close_deferred(c, browse_id=301, settings=False):
    c.command(100 if settings else 101)
    d = c.dialog()
    c.command(browse_id, d)
    box = c.box()
    post(d, 0x10)
    time.sleep(.25)
    c.alive()
    assert u.IsWindow(d) and u.IsWindow(box), 'parent destroyed inside selection loop'
    c.command(2, box)
    n.wait(lambda: not n.windows(c.p.pid, 'OrbitRustDialog'))
    assert u.IsWindowEnabled(c.main)


def config_commands_blocked(c):
    c.command(100)
    d = c.dialog()
    before = c.path.read_bytes()
    c.command(108)
    assert c.path.read_bytes() == before, 'gesture changed during settings'
    for id in (101, 102, 103, 104, 105, 200, 201, 202, 203):
        c.command(id)
        assert c.path.read_bytes() == before and u.IsWindow(d), f'command {id} mutated editing state'


def unicode_expansion(c):
    c.command(101)
    d = c.dialog()
    n.text(u.GetDlgItem(d, 100), 'Unicode sample')
    n.text(u.GetDlgItem(d, 101), '\u0130%WINDIR%')
    c.command(1, d)
    time.sleep(.5)
    c.alive()


def dpi_during_message(c):
    c.command(109)
    box = c.box()
    before = W.RECT()
    u.GetWindowRect(c.main, C.byref(before))
    for dpi in (120, 144):
        # Feature-only helper makes a real RECT inside the target process.
        post(c.main, 0x8011, dpi | (dpi << 16))
    time.sleep(.25)
    c.command(7, box)
    time.sleep(.4)
    after = W.RECT()
    u.GetWindowRect(c.main, C.byref(after))
    assert after.right - after.left > (before.right - before.left) * 1.4, 'latest deferred DPI was lost'


def recycle_done_during_message(c, result_button=6):
    c.command(5)  # Only allowed by runner with --test-feature, never on default exe.
    box = c.box()
    c.command(result_button, box)
    n.wait(lambda: not u.IsWindow(box))
    c.command(109)
    box = c.box()
    time.sleep(1)  # substitute finishes while the App borrow is held by confirm()
    c.command(7, box)
    n.wait(lambda: not u.IsWindow(box))
    time.sleep(.3)
    c.dismiss()  # failure result has one error message, after busy is cleared
    c.command(107)
    assert c.p.wait(timeout=3) == 0, 'completion notification lost; quit remains blocked'


def recycle_menu_guard(c):
    c.command(5)
    box = c.box()
    c.menu()
    c.command(5)
    assert len(n.windows(c.p.pid, '#32770')) == 1, 'recycle operation started twice'
    c.command(107)
    c.alive()
    assert u.IsWindow(box), 'quit destroyed recycle confirmation'


def recycle_confirmation_auto_hide(c):
    c.command(5)
    box = c.box()
    c.menu()
    pos = W.POINT()
    u.GetCursorPos(C.byref(pos))
    u.SetWindowPos(c.main, None, pos.x + 600, pos.y + 600, 0, 0, 0x15)
    time.sleep(1.2)
    assert u.IsWindowVisible(c.main), 'auto-hide hid the disabled confirmation owner'
    assert u.IsWindow(box), 'recycle confirmation unexpectedly closed'


def selection_config_isolation(c, browse_id=301, settings=False):
    c.command(100 if settings else 101)
    d = c.dialog()
    c.command(browse_id, d)
    box = c.box()
    before = c.path.read_bytes()
    for id in (108, 109, 101, 102, 103, 104, 105, 200, 201, 202, 203):
        c.command(id)
        assert c.path.read_bytes() == before, f'command {id} changed config inside selection'
        assert u.IsWindow(d) and u.IsWindow(box)
    assert len(n.windows(c.p.pid, '#32770')) == 1


def dpi_during_menu(c):
    before = W.RECT()
    u.GetWindowRect(c.main, C.byref(before))
    post(c.main, 0x8003, 0, 0x205)
    time.sleep(.25)
    for dpi in (120, 144):
        post(c.main, 0x8011, dpi | (dpi << 16))
    time.sleep(.2)
    post(c.main, 0x1f)
    time.sleep(.35)
    after = W.RECT()
    u.GetWindowRect(c.main, C.byref(after))
    assert after.right - after.left > (before.right - before.left) * 1.4, 'menu lost DPI notification'


def dpi_during_move_scope(c):
    before = W.RECT()
    u.GetWindowRect(c.main, C.byref(before))
    post(c.main, 0x8012)
    box = c.box()
    for dpi in (120, 144):
        post(c.main, 0x8011, dpi | (dpi << 16))
    time.sleep(.2)
    c.command(1, box)
    n.wait(lambda: not u.IsWindow(box))
    time.sleep(.3)
    after = W.RECT()
    u.GetWindowRect(c.main, C.byref(after))
    assert after.right - after.left > (before.right - before.left) * 1.4, 'move scope lost DPI notification'


def recycle_done_during_move_scope(c):
    c.command(5)
    box = c.box()
    c.command(6, box)
    n.wait(lambda: not u.IsWindow(box))
    post(c.main, 0x8012)
    box = c.box()
    time.sleep(1)
    c.command(1, box)
    n.wait(lambda: not u.IsWindow(box))
    time.sleep(.25)
    c.command(107)
    assert c.p.wait(timeout=3) == 0


def tooltip_settings(c):
    tip = n.wait(lambda: next(iter(n.windows(c.p.pid, 'tooltips_class32')), None))
    u.ShowWindow.argtypes = [W.HWND, C.c_int]
    u.ShowWindow(tip, 4)
    n.wait(lambda: u.IsWindowVisible(tip))
    c.command(100)
    c.dialog()


CASES = {
    'dialog_close_diagnostic_completion': close_during_diagnostic_completion,
    'borrowed_main_close': close_while_app_borrowed,
    'quit_guard_menu': quit_guard_across_menu,
    'add_target_fixed': add_target_fixed,
    'diagnostic_guard': diagnostic_rejected_while_dialog,
    'file_selection_quit': browse_quit_guard,
    'font_selection_quit': lambda c: browse_quit_guard(c, 310, True),
    'folder_selection_quit': lambda c: browse_quit_guard(c, 302),
    'file_selection_parent_close': browse_parent_close_deferred,
    'font_selection_parent_close': lambda c: browse_parent_close_deferred(c, 310, True),
    'folder_selection_parent_close': lambda c: browse_parent_close_deferred(c, 302),
    'config_command_guard': config_commands_blocked,
    'unicode_expansion': unicode_expansion,
    'dpi_during_message': dpi_during_message,
    'recycle_done_during_message': recycle_done_during_message,
    'recycle_menu_guard': recycle_menu_guard,
    'recycle_confirmation_auto_hide': recycle_confirmation_auto_hide,
    'file_selection_config': selection_config_isolation,
    'folder_selection_config': lambda c: selection_config_isolation(c, 302),
    'font_selection_config': lambda c: selection_config_isolation(c, 310, True),
    'dpi_during_menu': dpi_during_menu,
    'dpi_during_move_scope': dpi_during_move_scope,
    'recycle_done_during_move_scope': recycle_done_during_move_scope,
    'recycle_cancel_during_message': lambda c: recycle_done_during_message(c, 2),
    'recycle_failure_during_message': lambda c: recycle_done_during_message(c, 7),
    'tooltip_settings': tooltip_settings,
}
FEATURE_CASES = {'dpi_during_message', 'recycle_done_during_message', 'recycle_menu_guard', 'recycle_confirmation_auto_hide',
                 'dpi_during_menu', 'dpi_during_move_scope', 'recycle_done_during_move_scope',
                 'recycle_cancel_during_message', 'recycle_failure_during_message'}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', action='append', choices=CASES)
    parser.add_argument('--test-feature', action='store_true')
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    result = {'exe': str(args.exe.resolve()), 'cases': {}, 'complete': False}
    selected = args.case or [name for name in CASES if name != 'tooltip_settings' and (args.test_feature or name not in FEATURE_CASES)]
    assert args.test_feature or not (set(selected) & FEATURE_CASES), 'Substitute cases require explicit --test-feature'
    if set(selected) & FEATURE_CASES:
        # Refuse real recycling even if the caller accidentally supplies a default exe.
        data = args.exe.read_bytes()
        assert b'Orbit review recycle substitute' in data or 'Orbit review recycle substitute'.encode('utf-16le') in data, 'Not a substitute-only build'
    for name in selected:
        started = time.monotonic()
        with tempfile.TemporaryDirectory(prefix='Orbit-review-') as directory:
            c = None
            try:
                c = Case(args.exe.resolve(), directory, name == 'recycle_confirmation_auto_hide')
                CASES[name](c)
                row = {'passed': True}
            except Exception as error:
                row = {'passed': False, 'error': str(error), 'traceback': traceback.format_exc()}
            finally:
                if c:
                    c.finish()
            row['seconds'] = round(time.monotonic() - started, 2)
            result['cases'][name] = row
            args.output.write_text(json.dumps(result, indent=2), encoding='utf-8')
            print(name, json.dumps(row), flush=True)
    result['complete'] = True
    args.output.write_text(json.dumps(result, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
