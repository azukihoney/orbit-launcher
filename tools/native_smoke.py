"""Exercise only the Rust sample UI; never launch entries, empty bins or move the pointer."""
import ctypes as C
from ctypes import wintypes as W
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT=Path(__file__).resolve().parents[1]
EXE=ROOT/'target/release/orbit-launcher.exe'
u=C.WinDLL('user32',use_last_error=True)
k=C.WinDLL('kernel32',use_last_error=True)
callback=C.WINFUNCTYPE(W.BOOL,W.HWND,W.LPARAM)
u.EnumWindows.argtypes=[callback,W.LPARAM]
u.GetWindowThreadProcessId.argtypes=[W.HWND,C.POINTER(W.DWORD)]
u.GetClassNameW.argtypes=[W.HWND,W.LPWSTR,C.c_int]
u.GetDlgItem.argtypes=[W.HWND,C.c_int];u.GetDlgItem.restype=W.HWND
u.SendMessageW.argtypes=[W.HWND,W.UINT,W.WPARAM,W.LPARAM];u.SendMessageW.restype=W.LPARAM
u.PostMessageW.argtypes=[W.HWND,W.UINT,W.WPARAM,W.LPARAM]
u.IsWindowEnabled.argtypes=[W.HWND]
u.GetWindow.argtypes=[W.HWND,W.UINT];u.GetWindow.restype=W.HWND
u.GetDpiForWindow.argtypes=[W.HWND];u.GetDpiForWindow.restype=W.UINT
u.SetWindowTextW.argtypes=[W.HWND,W.LPCWSTR]
u.IsWindowVisible.argtypes=[W.HWND]
u.GetWindowLongPtrW.argtypes=[W.HWND,C.c_int];u.GetWindowLongPtrW.restype=C.c_ssize_t
u.CreateWindowExW.argtypes=[W.DWORD,W.LPCWSTR,W.LPCWSTR,W.DWORD,C.c_int,C.c_int,C.c_int,C.c_int,W.HWND,W.HMENU,W.HINSTANCE,C.c_void_p];u.CreateWindowExW.restype=W.HWND
u.DestroyWindow.argtypes=[W.HWND]
u.SetWindowPos.argtypes=[W.HWND,W.HWND,C.c_int,C.c_int,C.c_int,C.c_int,W.UINT]
u.SetForegroundWindow.argtypes=[W.HWND]
u.GetWindowRect.argtypes=[W.HWND,C.POINTER(W.RECT)]
u.GetGuiResources.argtypes=[W.HANDLE,W.DWORD];u.GetGuiResources.restype=W.DWORD
k.OpenProcess.argtypes=[W.DWORD,W.BOOL,W.DWORD];k.OpenProcess.restype=W.HANDLE
k.CloseHandle.argtypes=[W.HANDLE]


def windows(pid,klass):
    result=[]
    @callback
    def visit(hwnd,_):
        owner=W.DWORD();u.GetWindowThreadProcessId(hwnd,C.byref(owner))
        text=C.create_unicode_buffer(256);u.GetClassNameW(hwnd,text,256)
        if owner.value==pid and text.value.startswith(klass): result.append(hwnd)
        return True
    u.EnumWindows(visit,0)
    return result


def wait(check,seconds=5):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        value=check()
        if value:return value
        time.sleep(.03)
    raise AssertionError('Native check timed out')


def text(hwnd,value):
    assert hwnd,'Native input control was not found'
    buffer=C.create_unicode_buffer(value)
    assert u.SendMessageW(hwnd,0x000c,0,C.addressof(buffer)),'WM_SETTEXT failed'
    check=C.create_unicode_buffer(1024)
    u.SendMessageW(hwnd,0x000d,len(check),C.addressof(check))
    assert check.value==value,(check.value,value)
def command(hwnd,value):u.SendMessageW(hwnd,0x111,value,0)
def point(x,y,scale=1):return (round(x*scale)&0xffff)|((round(y*scale)&0xffff)<<16)
def is_above(hwnd,other):
    seen=set()
    above=u.GetWindow(other,3)
    while above and above not in seen:
        if above==hwnd:return True
        seen.add(above);above=u.GetWindow(above,3)
    return False

def topmost(hwnd):return bool(u.GetWindowLongPtrW(hwnd,-20)&8)


def sample():
    win=os.environ.get('WINDIR',r'C:\Windows')
    def entry(i,name,target,kind='launch'):
        return dict(id=str(i),name=name,target=target,kind=kind,args='',cwd='')
    return dict(version=1,layout_version=2,settings=dict(auto_hide=False,turns=1.5),
        pages=[dict(id='main',name='Apps',items=[
            entry(1,'メモ帳',win+r'\System32\notepad.exe'),entry(2,'電卓',win+r'\System32\calc.exe'),
            entry(3,'コマンド',win+r'\System32\cmd.exe'),entry(4,'エクスプローラー',win+r'\explorer.exe')])],
        common=[entry(5,'Work',str(ROOT.parent),'folder')])


def capture(hwnd,path):
    """Render just this owned window with PrintWindow; no desktop capture or cropping."""
    g=C.WinDLL('gdi32');p=C.WinDLL('gdiplus')
    class Startup(C.Structure):
        _fields_=[('version',W.UINT),('callback',C.c_void_p),('background',W.BOOL),('codecs',W.BOOL)]
    class GUID(C.Structure):
        _fields_=[('a',W.DWORD),('b',W.WORD),('c',W.WORD),('d',C.c_ubyte*8)]
    u.GetWindowRect.argtypes=[W.HWND,C.POINTER(W.RECT)]
    u.GetDC.argtypes=[W.HWND];u.GetDC.restype=W.HDC
    u.ReleaseDC.argtypes=[W.HWND,W.HDC]
    u.PrintWindow.argtypes=[W.HWND,W.HDC,W.UINT]
    g.CreateCompatibleDC.argtypes=[W.HDC];g.CreateCompatibleDC.restype=W.HDC
    g.CreateCompatibleBitmap.argtypes=[W.HDC,C.c_int,C.c_int];g.CreateCompatibleBitmap.restype=W.HBITMAP
    g.SelectObject.argtypes=[W.HDC,W.HGDIOBJ];g.SelectObject.restype=W.HGDIOBJ
    g.DeleteObject.argtypes=[W.HGDIOBJ];g.DeleteDC.argtypes=[W.HDC]
    p.GdiplusStartup.argtypes=[C.POINTER(C.c_size_t),C.POINTER(Startup),C.c_void_p]
    p.GdipCreateBitmapFromHBITMAP.argtypes=[W.HBITMAP,W.HPALETTE,C.POINTER(C.c_void_p)]
    p.GdipSaveImageToFile.argtypes=[C.c_void_p,W.LPCWSTR,C.POINTER(GUID),C.c_void_p]
    p.GdipDisposeImage.argtypes=[C.c_void_p];p.GdiplusShutdown.argtypes=[C.c_size_t]
    token=C.c_size_t();assert p.GdiplusStartup(C.byref(token),C.byref(Startup(1,None,0,0)),None)==0
    r=W.RECT();u.GetWindowRect(hwnd,C.byref(r))
    dc=u.GetDC(hwnd);mem=g.CreateCompatibleDC(dc);bitmap=g.CreateCompatibleBitmap(dc,r.right-r.left,r.bottom-r.top)
    old=g.SelectObject(mem,bitmap);assert u.PrintWindow(hwnd,mem,0)
    image=C.c_void_p();assert p.GdipCreateBitmapFromHBITMAP(bitmap,None,C.byref(image))==0
    encoder=GUID(0x557cf406,0x1a04,0x11d3,(C.c_ubyte*8)(0x9a,0x73,0,0,0xf8,0x1e,0xf3,0x2e))
    assert p.GdipSaveImageToFile(image,str(path),C.byref(encoder),None)==0
    p.GdipDisposeImage(image);g.SelectObject(mem,old);g.DeleteObject(bitmap);g.DeleteDC(mem);u.ReleaseDC(hwnd,dc);p.GdiplusShutdown(token)


def run(output=None):
    output=Path(output) if output else ROOT/'_checks';output.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='Orbit Rust ') as directory:
        config=Path(directory)/'config.json'
        config.write_text(json.dumps(sample(),ensure_ascii=False),encoding='utf-8')
        process=subprocess.Popen([str(EXE),'--config',str(config),'--no-gesture'])
        cover=None
        try:
            main=wait(lambda:next(iter(windows(process.pid,'OrbitRust-')),None))
            scale=u.GetDpiForWindow(main)/96
            time.sleep(.4)
            # Reproduce calling from behind another application's ordinary window.
            r=W.RECT();u.GetWindowRect(main,C.byref(r))
            cover=u.CreateWindowExW(0,'STATIC','Orbit foreground regression cover',0x10cf0000,
                r.left,r.top,r.right-r.left,r.bottom-r.top,None,None,None,None)
            assert cover
            for call_message,call_id in [(0x8004,0),(0x0312,1)]:
                u.SendMessageW(main,0x10,0,0)
                u.SetWindowPos(main,W.HWND(-2),0,0,0,0,0x13)
                u.SetForegroundWindow(cover)
                u.PostMessageW(main,call_message,call_id,0)
                wait(lambda:u.IsWindowVisible(main) and topmost(main))
                assert is_above(main,cover),'Reveal must place Orbit above the cover'
                u.SetForegroundWindow(cover);time.sleep(.1)
                assert topmost(main) and is_above(main,cover),'Orbit must stay above ordinary windows after deactivation'
            state=[]
            for h in windows(process.pid,''):
                cls=C.create_unicode_buffer(100);u.GetClassNameW(h,cls,100)
                state.append(dict(class_name=cls.value,visible=bool(u.IsWindowVisible(h)),
                    enabled=bool(u.IsWindowEnabled(h)),owned_by_main=u.GetWindow(h,4)==main))
            (output/'before-settings.json').write_text(json.dumps(state,indent=2),encoding='utf-8')
            command(main,100);dialog=wait(lambda:next(iter(windows(process.pid,'OrbitRustDialog')),None))
            assert not u.IsWindowEnabled(main)
            assert topmost(dialog) and is_above(dialog,main)
            above=u.GetWindow(main,3);seen=set()
            while above and above!=dialog and above not in seen:seen.add(above);above=u.GetWindow(above,3)
            assert above==dialog,'Settings must be above its owner'
            secondary=subprocess.run([str(EXE),'--config',str(config),'--no-gesture'],timeout=5)
            assert secondary.returncode==0
            assert len(windows(process.pid,'OrbitRustDialog'))==1,'Second launch must retain the active dialog'
            u.PostMessageW(main,0x8004,0,0);time.sleep(.1)
            assert not u.IsWindowEnabled(main)
            assert is_above(dialog,main) and is_above(dialog,cover)
            capture(dialog,output/'settings-ja.png')
            command(dialog,2);wait(lambda:u.IsWindowEnabled(main))
            assert topmost(main) and is_above(main,cover)
            command(main,100);dialog=wait(lambda:next(iter(windows(process.pid,'OrbitRustDialog')),None))
            u.SendMessageW(u.GetDlgItem(dialog,100),0x14e,1,0) # language = en
            u.SendMessageW(u.GetDlgItem(dialog,104),0x14e,1,0) # theme = dark
            text(u.GetDlgItem(dialog,102),'13');text(u.GetDlgItem(dialog,103),'11')
            command(dialog,1);wait(lambda:u.IsWindowEnabled(main))
            saved=json.loads(config.read_text(encoding='utf-8'))
            assert saved['settings']['language']=='en' and saved['settings']['theme']=='dark'
            assert saved['settings']['app_font_px']==13 and saved['settings']['folder_font_px']==11,(saved['settings']['app_font_px'],saved['settings']['folder_font_px'])
            assert saved['settings']['gesture_enabled'] is True,'Appearance save must not disable gesture'
            assert config.with_suffix('.json.bak').exists()
            command(main,100);dialog=wait(lambda:next(iter(windows(process.pid,'OrbitRustDialog')),None))
            capture(dialog,output/'settings-dark-en.png')
            combo=u.GetDlgItem(dialog,104);u.SendMessageW(combo,0x14f,1,0);time.sleep(.1);u.SendMessageW(combo,0x14f,0,0)
            command(dialog,2);wait(lambda:u.IsWindowEnabled(main))
            # Native mouse messages exercise a blank-cell move; the real pointer stays untouched.
            u.SendMessageW(main,0x201,1,point(65,59,scale))
            u.SendMessageW(main,0x200,1,point(475,141,scale))
            u.SendMessageW(main,0x202,0,point(475,141,scale))
            time.sleep(.15)
            saved=json.loads(config.read_text(encoding='utf-8'))
            assert next(e for e in saved['pages'][0]['items'] if e['id']=='1')['cell']==[1,5]
            # Add a registration, but do not execute it.
            command(main,101);dialog=wait(lambda:next(iter(windows(process.pid,'OrbitRustDialog')),None))
            text(u.GetDlgItem(dialog,100),'Test command');text(u.GetDlgItem(dialog,101),r'C:\Example\test.cmd')
            text(u.GetDlgItem(dialog,102),'"a b" & literal')
            command(dialog,1);wait(lambda:u.IsWindowEnabled(main))
            saved=json.loads(config.read_text(encoding='utf-8'))
            assert saved['pages'][0]['items'][-1]['args']=='"a b" & literal'
            # Windows HDROP is transferred to the recipient, which frees it with DragFinish.
            class DROPFILES(C.Structure):
                _fields_=[('offset',W.DWORD),('point',W.POINT),('nonclient',W.BOOL),('wide',W.BOOL)]
            paths=[r'C:\Example\inert.cmd',r'C:\Example\テスト.cmd']
            payload=bytes(DROPFILES(C.sizeof(DROPFILES),W.POINT(round(65*scale),round(59*scale)),0,1))+('\0'.join(paths)+'\0\0').encode('utf-16le')
            k.GlobalAlloc.argtypes=[W.UINT,C.c_size_t];k.GlobalAlloc.restype=W.HGLOBAL
            k.GlobalLock.argtypes=[W.HGLOBAL];k.GlobalLock.restype=C.c_void_p
            k.GlobalUnlock.argtypes=[W.HGLOBAL]
            memory=k.GlobalAlloc(0x42,len(payload));C.memmove(k.GlobalLock(memory),payload,len(payload));k.GlobalUnlock(memory)
            assert u.PostMessageW(main,0x233,memory,0);time.sleep(.3)
            saved=json.loads(config.read_text(encoding='utf-8'))
            assert all(any(e['target']==path for e in saved['pages'][0]['items']) for path in paths),{'app_entries':len(saved['pages'][0]['items']),'common_entries':len(saved['common'])}
            command(main,103);dialog=wait(lambda:next(iter(windows(process.pid,'OrbitRustDialog')),None))
            text(u.GetDlgItem(dialog,100),'Second page');command(dialog,1);wait(lambda:u.IsWindowEnabled(main))
            command(main,104);dialog=wait(lambda:next(iter(windows(process.pid,'OrbitRustDialog')),None))
            text(u.GetDlgItem(dialog,100),'Renamed page');command(dialog,1);wait(lambda:u.IsWindowEnabled(main))
            saved=json.loads(config.read_text(encoding='utf-8'));assert saved['pages'][1]['name']=='Renamed page'
            command(main,105);time.sleep(.1);assert len(json.loads(config.read_text(encoding='utf-8'))['pages'])==1
            # Repeated settings opens should release their font, brush and child windows.
            handle=k.OpenProcess(0x400,False,process.pid);baseline=u.GetGuiResources(handle,0)
            for _ in range(12):
                command(main,100);dialog=wait(lambda:next(iter(windows(process.pid,'OrbitRustDialog')),None))
                command(dialog,2);wait(lambda:u.IsWindowEnabled(main))
            after=u.GetGuiResources(handle,0);k.CloseHandle(handle)
            assert after<=baseline+4,(baseline,after)
            command(main,107);assert process.wait(timeout=5)==0
            print(json.dumps(dict(dialog_z_order=True,cancel_and_save=True,language_and_theme=True,
                independent_font_sizes=True,gesture_setting_preserved=True,sparse_drag_saved=True,
                literal_arguments_preserved=True,unicode_file_drop=True,page_management=True,single_instance=True,
                reveal_topmost=True,topmost_after_deactivation=True,settings_above_topmost_owner=True,
                gdi_before=baseline,gdi_after=after,no_entries_executed=True)))
        finally:
            if cover:u.DestroyWindow(cover)
            if process.poll() is None:process.terminate();process.wait(timeout=5)
    # Preview through the same native renderer, using only public sample registrations.
    for label,settings in [('ja',{}),('dark-en',{'language':'en','theme':'dark'})]:
        cfg=sample();cfg['settings'].update(settings)
        path=output/f'preview-{label}.json';path.write_text(json.dumps(cfg,ensure_ascii=False),encoding='utf-8')
        result=subprocess.run([str(EXE),'--config',str(path),'--preview',str(output/f'rust-{label}.png')],timeout=10)
        assert result.returncode==0


if __name__=='__main__':
    import argparse
    parser=argparse.ArgumentParser()
    parser.add_argument('--exe',type=Path)
    parser.add_argument('--output',type=Path)
    args=parser.parse_args()
    if args.exe:EXE=args.exe.resolve()
    run(args.output)
