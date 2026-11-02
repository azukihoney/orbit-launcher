//! Icon-only acquisition. Legacy file_icon remains unchanged.
use super::*;
use windows_sys::core::GUID;
// GDI+ PixelFormat32bppPARGB (not exported as a named constant by windows-sys).
const PIXEL_FORMAT_32BPP_PARGB: i32 = 0x000e200b;

struct Apartment(bool);
impl Apartment {
    fn new() -> Self {
        Self(unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0 })
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                CoUninitialize();
            }
        }
    }
}
#[repr(C)]
struct ImageList {
    table: *const ImageListTable,
}
#[repr(C)]
struct ImageListTable {
    query: unsafe extern "system" fn(*mut ImageList, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut ImageList) -> u32,
    release: unsafe extern "system" fn(*mut ImageList) -> u32,
    preceding_get_icon: [usize; 7],
    get_icon: unsafe extern "system" fn(*mut ImageList, i32, u32, *mut HICON) -> i32,
    preceding_get_size: [usize; 5],
    get_size: unsafe extern "system" fn(*mut ImageList, *mut i32, *mut i32) -> i32,
}
struct List(*mut ImageList);
impl Drop for List {
    fn drop(&mut self) {
        unsafe {
            ((*(*self.0).table).release)(self.0);
        }
    }
}
impl List {
    fn new(which: i32) -> Option<Self> {
        let iid = GUID {
            data1: 0x46eb5926,
            data2: 0x582e,
            data3: 0x4017,
            data4: [0x9f, 0xdf, 0xe8, 0x99, 0x8d, 0xaa, 0x09, 0x50],
        };
        let mut raw = null_mut();
        let hr = unsafe { SHGetImageList(which, &iid, &mut raw) };
        if raw.is_null() {
            return None;
        }
        let list = Self(raw.cast());
        (hr >= 0).then_some(list)
    }
    fn size(&self) -> Option<(i32, i32)> {
        let (mut x, mut y) = (0, 0);
        let hr = unsafe { ((*(*self.0).table).get_size)(self.0, &mut x, &mut y) };
        (hr >= 0 && x > 0 && y > 0).then_some((x, y))
    }
    fn icon(&self, index: i32) -> Option<Icon> {
        let mut raw = null_mut();
        let hr = unsafe { ((*(*self.0).table).get_icon)(self.0, index, 1, &mut raw) };
        let icon = Icon(raw);
        (hr >= 0 && !raw.is_null()).then_some(icon)
    }
}
fn extract(path: &str, index: i32, pixels: u32) -> Option<Icon> {
    if path_decisions::unc(path) {
        return None;
    }
    #[cfg(feature = "review-test")]
    if !crate::icon_only_probe::io("extract", path) {
        return None;
    }
    let mut raw = null_mut();
    let hr =
        unsafe { SHDefExtractIconW(wide(path).as_ptr(), index, 0, &mut raw, null_mut(), pixels) };
    #[cfg(feature = "review-test")]
    crate::icon_only_probe::event(
        "extract_result",
        serde_json::json!({"index":index,"pixels":pixels,"hr":hr,"valid":!raw.is_null()}),
    );
    extracted(hr, Icon(raw))
}
fn extracted(hr: i32, icon: Icon) -> Option<Icon> {
    (hr == 0 && !icon.0.is_null()).then_some(icon)
}
fn direct(path: &str, pixels: u32) -> Option<Icon> {
    let extension = std::path::Path::new(path)
        .extension()?
        .to_str()?
        .to_ascii_lowercase();
    matches!(extension.as_str(), "exe" | "dll" | "ico")
        .then(|| extract(path, 0, pixels))
        .flatten()
}
fn shell(path: &str, attributes: u32, generic: bool, pixels: u32) -> Option<Icon> {
    // Generic calls use local placeholder names, never the supplied UNC registration.
    if path_decisions::unc(path) {
        return None;
    }
    #[cfg(feature = "review-test")]
    if !crate::icon_only_probe::io("shell", path) {
        return None;
    }
    let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
    let flags = SHGFI_SYSICONINDEX | if generic { SHGFI_USEFILEATTRIBUTES } else { 0 };
    if unsafe {
        SHGetFileInfoW(
            wide(path).as_ptr(),
            attributes,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    } == 0
    {
        return None;
    }
    if let Some(list) = List::new(2)
        && list.size() == Some((pixels as i32, pixels as i32))
        && let Some(icon) = list.icon(info.iIcon)
    {
        return Some(icon);
    }
    let list = List::new(4)?;
    let size = list.size()?;
    if size.0 < pixels as i32 || size.1 < pixels as i32 {
        return None;
    }
    let original = list.icon(info.iIcon)?;
    resize(&original, size.0, pixels as i32)
}
fn generic(entry: &Entry, pixels: u32) -> Option<Icon> {
    let folder = entry.kind == "folder";
    let name = if folder {
        "folder"
    } else if entry.target.to_ascii_lowercase().ends_with(".bat") {
        "file.bat"
    } else {
        "file.exe"
    };
    shell(
        name,
        if folder {
            FILE_ATTRIBUTE_DIRECTORY
        } else {
            FILE_ATTRIBUTE_NORMAL
        },
        true,
        pixels,
    )
}
pub(super) fn acquire(entry: &Entry, pixels: u32) -> Icon {
    let pixels = pixels.clamp(1, 256);
    let apartment = Apartment::new();
    let path = expand(&entry.target);
    if path_decisions::unc(&path) {
        return generic(entry, pixels).unwrap_or(Icon(null_mut()));
    }
    if path_decisions::may_read_link(&entry.kind, &path) {
        // If we cannot inspect a shortcut safely, do not let Shell follow an
        // unknown (possibly remote) target while asking for the link's icon.
        if !apartment.0 {
            return generic(entry, pixels).unwrap_or(Icon(null_mut()));
        }
        let Some(link) = details(&path) else {
            return generic(entry, pixels).unwrap_or(Icon(null_mut()));
        };
        let explicit = expand(&link.icon_path);
        let target = expand(&link.target);
        if path_decisions::unc(&explicit) || path_decisions::unc(&target) {
            return generic(entry, pixels).unwrap_or(Icon(null_mut()));
        }
        if !explicit.is_empty()
            && let Some(icon) = extract(&explicit, link.icon_index, pixels)
        {
            return icon;
        }
        if !target.is_empty()
            && exists(&target)
            && let Some(icon) = direct(&target, pixels)
                .or_else(|| shell(&target, FILE_ATTRIBUTE_NORMAL, false, pixels))
        {
            return icon;
        }
    }
    direct(&path, pixels)
        .or_else(|| shell(&path, FILE_ATTRIBUTE_NORMAL, false, pixels))
        .or_else(|| generic(entry, pixels))
        .unwrap_or(Icon(null_mut()))
}
fn details(path: &str) -> Option<LinkDetails> {
    #[cfg(feature = "review-test")]
    if !crate::icon_only_probe::io("link_load", path) {
        return None;
    }
    unsafe { read_link_for_icon(path) }
}
fn exists(path: &str) -> bool {
    #[cfg(feature = "review-test")]
    if !crate::icon_only_probe::io("exists", path) {
        return false;
    }
    std::path::Path::new(path).exists()
}

struct Bitmap(HBITMAP);
impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.0);
        }
    }
}
struct Dc(HDC);
impl Drop for Dc {
    fn drop(&mut self) {
        unsafe {
            DeleteDC(self.0);
        }
    }
}
struct Image(*mut GpBitmap);
impl Drop for Image {
    fn drop(&mut self) {
        unsafe {
            GdipDisposeImage(self.0.cast());
        }
    }
}
struct Graphics(*mut GpGraphics);
impl Drop for Graphics {
    fn drop(&mut self) {
        unsafe {
            GdipDeleteGraphics(self.0);
        }
    }
}

fn resize(icon: &Icon, source: i32, pixels: i32) -> Option<Icon> {
    unsafe {
        let dc = Dc(CreateCompatibleDC(null_mut()));
        if dc.0.is_null() {
            return None;
        }
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = source;
        info.bmiHeader.biHeight = -source;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        let mut bits = null_mut();
        let bitmap = Bitmap(CreateDIBSection(
            dc.0,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            null_mut(),
            0,
        ));
        if bitmap.0.is_null() || bits.is_null() {
            return None;
        }
        let old = SelectObject(dc.0, bitmap.0);
        std::ptr::write_bytes(bits, 0, (source * source * 4) as usize);
        let drawn = DrawIconEx(dc.0, 0, 0, icon.0, source, source, 0, null_mut(), DI_NORMAL);
        GdiFlush();
        SelectObject(dc.0, old);
        if drawn == 0 {
            return None;
        }
        let data =
            std::slice::from_raw_parts_mut(bits.cast::<u8>(), (source * source * 4) as usize);
        if data.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0) {
            // Old non-alpha icons use the AND mask. Convert it before high quality scaling.
            let mut details: ICONINFO = std::mem::zeroed();
            if GetIconInfo(icon.0, &mut details) == 0 {
                return None;
            }
            let _color = Bitmap(details.hbmColor);
            let mask = Bitmap(details.hbmMask);
            let mut mask_data = vec![0u8; data.len()];
            let read = GetDIBits(
                dc.0,
                mask.0,
                0,
                source as u32,
                mask_data.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            );
            if read == 0 {
                return None;
            }
            for (pixel, mask) in data
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(mask_data.as_chunks::<4>().0.iter())
            {
                pixel[3] = if mask[0] == 0 { 255 } else { 0 };
            }
        }
        let mut original = null_mut();
        if GdipCreateBitmapFromScan0(
            source,
            source,
            source * 4,
            PIXEL_FORMAT_32BPP_PARGB,
            bits.cast(),
            &mut original,
        ) != 0
        {
            return None;
        }
        let original = Image(original);
        let mut output = null_mut();
        if GdipCreateBitmapFromScan0(
            pixels,
            pixels,
            0,
            PIXEL_FORMAT_32BPP_PARGB,
            null(),
            &mut output,
        ) != 0
        {
            return None;
        }
        let output = Image(output);
        let mut graphics = null_mut();
        if GdipGetImageGraphicsContext(output.0.cast(), &mut graphics) != 0 {
            return None;
        }
        let graphics = Graphics(graphics);
        GdipSetInterpolationMode(graphics.0, InterpolationModeHighQualityBicubic);
        GdipSetPixelOffsetMode(graphics.0, PixelOffsetModeHalf);
        if GdipDrawImageRectI(graphics.0, original.0.cast(), 0, 0, pixels, pixels) != 0 {
            return None;
        }
        drop(graphics);
        let mut raw = null_mut();
        let hr = GdipCreateHICONFromBitmap(output.0, &mut raw);
        let icon = Icon(raw);
        (hr == 0 && !raw.is_null()).then_some(icon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(source: i32, alpha: bool) -> Icon {
        unsafe {
            let dc = Dc(CreateCompatibleDC(null_mut()));
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = source;
            info.bmiHeader.biHeight = -source;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            let mut bits = null_mut();
            let color = Bitmap(CreateDIBSection(
                dc.0,
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                null_mut(),
                0,
            ));
            assert!(!color.0.is_null());
            let pixels =
                std::slice::from_raw_parts_mut(bits.cast::<u8>(), (source * source * 4) as usize);
            pixels.fill(0);
            let stride = ((source + 15) / 16 * 2) as usize;
            let mut masks = vec![255u8; stride * source as usize];
            for y in source / 4..source * 3 / 4 {
                for x in source / 4..source * 3 / 4 {
                    let index = ((y * source + x) * 4) as usize;
                    pixels[index + 2] = if alpha { 128 } else { 255 };
                    pixels[index + 3] = if alpha { 128 } else { 0 };
                    masks[y as usize * stride + x as usize / 8] &= !(0x80 >> (x % 8));
                }
            }
            let mask = Bitmap(CreateBitmap(source, source, 1, 1, masks.as_ptr().cast()));
            let details = ICONINFO {
                fIcon: 1,
                xHotspot: 0,
                yHotspot: 0,
                hbmColor: color.0,
                hbmMask: mask.0,
            };
            let icon = Icon(CreateIconIndirect(&details));
            assert!(!icon.0.is_null());
            icon
        }
    }
    fn color_at(icon: &Icon, size: i32, x: i32, y: i32, background: u32) -> u32 {
        unsafe {
            let screen = GetDC(null_mut());
            let dc = Dc(CreateCompatibleDC(screen));
            let bitmap = Bitmap(CreateCompatibleBitmap(screen, size, size));
            ReleaseDC(null_mut(), screen);
            let old = SelectObject(dc.0, bitmap.0);
            let brush = Bitmap(CreateSolidBrush(background));
            let rect = RECT {
                left: 0,
                top: 0,
                right: size,
                bottom: size,
            };
            FillRect(dc.0, &rect, brush.0);
            assert_ne!(
                DrawIconEx(dc.0, 0, 0, icon.0, size, size, 0, null_mut(), DI_NORMAL),
                0
            );
            let value = GetPixel(dc.0, x, y);
            SelectObject(dc.0, old);
            value
        }
    }
    #[test]
    fn shrink_preserves_alpha_and_legacy_and_mask() {
        let _gdi = crate::paint::GdiPlus::new().unwrap();
        for alpha in [false, true] {
            let original = sample(256, alpha);
            for px in [48, 60, 72] {
                let resized = resize(&original, 256, px).expect("resize failed");
                assert_eq!(color_at(&resized, px, 1, 1, 0x00ffffff), 0x00ffffff);
                assert_eq!(color_at(&resized, px, 1, 1, 0), 0);
                assert_ne!(color_at(&resized, px, px / 2, px / 2, 0), 0);
                if alpha {
                    assert_ne!(
                        color_at(&resized, px, px / 2, px / 2, 0),
                        color_at(&resized, px, px / 2, px / 2, 0x00ffffff)
                    );
                }
            }
        }
    }
    #[test]
    fn extraction_requires_s_ok_and_a_handle() {
        assert!(extracted(0, Icon(null_mut())).is_none());
        for result in [1, -1, i32::MIN] {
            let shared = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
            let owned = unsafe { CopyIcon(shared) };
            assert!(!owned.is_null());
            assert!(extracted(result, Icon(owned)).is_none());
        }
        let owned = unsafe { CopyIcon(LoadIconW(null_mut(), IDI_APPLICATION)) };
        assert!(extracted(0, Icon(owned)).is_some());
    }
}
