use anyhow::Result;
use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionaryRef;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::window::{
    kCGNullWindowID, kCGWindowListExcludeDesktopElements,
    kCGWindowListOptionOnScreenOnly, CGWindowListCopyWindowInfo,
};

pub struct WindowInfo {
    pub id: u32,
    pub name: String,
    pub owner: String,
    pub bounds: (i32, i32, u32, u32), // x, y, width, height
}

pub fn list_windows() -> Result<Vec<WindowInfo>> {
    let options = kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements;

    let window_list = unsafe { CGWindowListCopyWindowInfo(options, kCGNullWindowID) };

    if window_list.is_null() {
        return Ok(Vec::new());
    }

    let windows: Vec<WindowInfo> = unsafe {
        let count = core_foundation::array::CFArrayGetCount(window_list as _);
        let mut result = Vec::new();

        for i in 0..count {
            let dict = core_foundation::array::CFArrayGetValueAtIndex(window_list as _, i)
                as CFDictionaryRef;

            if let Some(info) = parse_window_dict(dict) {
                // Filter out windows without names or very small windows
                if !info.name.is_empty() && info.bounds.2 > 100 && info.bounds.3 > 100 {
                    result.push(info);
                }
            }
        }

        core_foundation::base::CFRelease(window_list as _);
        result
    };

    Ok(windows)
}

unsafe fn parse_window_dict(dict: CFDictionaryRef) -> Option<WindowInfo> {
    let id = get_number(dict, "kCGWindowNumber")? as u32;
    let name = get_string(dict, "kCGWindowName").unwrap_or_default();
    let owner = get_string(dict, "kCGWindowOwnerName").unwrap_or_default();

    // Get bounds dictionary
    let bounds_key = CFString::new("kCGWindowBounds");
    let mut bounds_dict: *const std::ffi::c_void = std::ptr::null();

    if core_foundation::dictionary::CFDictionaryGetValueIfPresent(
        dict,
        bounds_key.as_concrete_TypeRef() as _,
        &mut bounds_dict,
    ) == 0
    {
        return None;
    }

    let bounds_dict = bounds_dict as CFDictionaryRef;
    let x = get_number(bounds_dict, "X").unwrap_or(0.0) as i32;
    let y = get_number(bounds_dict, "Y").unwrap_or(0.0) as i32;
    let width = get_number(bounds_dict, "Width").unwrap_or(0.0) as u32;
    let height = get_number(bounds_dict, "Height").unwrap_or(0.0) as u32;

    Some(WindowInfo {
        id,
        name,
        owner,
        bounds: (x, y, width, height),
    })
}

unsafe fn get_string(dict: CFDictionaryRef, key: &str) -> Option<String> {
    let cf_key = CFString::new(key);
    let mut value: *const std::ffi::c_void = std::ptr::null();

    if core_foundation::dictionary::CFDictionaryGetValueIfPresent(
        dict,
        cf_key.as_concrete_TypeRef() as _,
        &mut value,
    ) == 0
    {
        return None;
    }

    let cf_string = CFString::wrap_under_get_rule(value as _);
    Some(cf_string.to_string())
}

unsafe fn get_number(dict: CFDictionaryRef, key: &str) -> Option<f64> {
    let cf_key = CFString::new(key);
    let mut value: *const std::ffi::c_void = std::ptr::null();

    if core_foundation::dictionary::CFDictionaryGetValueIfPresent(
        dict,
        cf_key.as_concrete_TypeRef() as _,
        &mut value,
    ) == 0
    {
        return None;
    }

    let cf_number = CFNumber::wrap_under_get_rule(value as _);
    cf_number.to_f64()
}
