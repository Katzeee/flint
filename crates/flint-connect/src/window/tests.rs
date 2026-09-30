use super::platform;
use std::ptr::null_mut;
use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, IsIconic, ShowWindow, SW_SHOWMINNOACTIVE,
        WS_OVERLAPPEDWINDOW,
    },
};

struct TestWindow(HWND);

impl Drop for TestWindow {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0);
        }
    }
}

#[test]
fn minimized_window_reports_why_capture_is_unavailable_without_restoring_it() {
    let window = TestWindow(unsafe {
        CreateWindowExW(
            0,
            windows_sys::w!("STATIC"),
            windows_sys::w!("Flint preview fixture"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            640,
            480,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
        )
    });
    assert!(!window.0.is_null());
    unsafe {
        ShowWindow(window.0, SW_SHOWMINNOACTIVE);
    }
    assert_ne!(unsafe { IsIconic(window.0) }, 0);
    let info = platform::window_info(std::process::id()).unwrap();
    assert_eq!(info.title, "Flint preview fixture");
    assert!(info.minimized);
    assert_eq!(
        platform::capture(std::process::id())
            .unwrap_err()
            .to_string(),
        "Window is minimized"
    );
    assert_ne!(unsafe { IsIconic(window.0) }, 0);
}
