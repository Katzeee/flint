//! Native window identification stays in the desktop adapter, outside Bridge and execution state.
use anyhow::{bail, Result};

pub fn local_pid(pid: u32, host: &str) -> Result<u32> {
    let matches_host = |candidate: &str| {
        let kind = host.to_ascii_lowercase();
        kind == candidate || (candidate == "max" && (kind == "3dsmax" || kind == "3ds max"))
    };
    if !flint_connect::candidate(pid).is_some_and(|candidate| matches_host(candidate.host)) {
        bail!("No matching local application window");
    }
    Ok(pid)
}

#[cfg(not(windows))]
pub fn preview(_pid: u32) -> Result<serde_json::Value> {
    bail!("Window previews are unavailable on this platform")
}
#[cfg(not(windows))]
pub fn focus(_pid: u32) -> Result<()> {
    bail!("Window switching is unavailable on this platform")
}

#[cfg(windows)]
pub use platform::{focus, preview};

#[cfg(windows)]
mod platform {
    use anyhow::{bail, ensure, Context, Result};
    use base64::Engine;
    use std::{mem::size_of, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, RECT},
        Graphics::Gdi::{
            CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
            SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
            HGDIOBJ,
        },
        Storage::Xps::PrintWindow,
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindow, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
            GetWindowThreadProcessId, IsHungAppWindow, IsIconic, IsWindowVisible,
            SetForegroundWindow, ShowWindowAsync, GW_OWNER, PW_RENDERFULLCONTENT, SW_RESTORE,
        },
    };

    struct Search {
        pid: u32,
        hwnd: HWND,
        area: i64,
    }

    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> i32 {
        let search = &mut *(data as *mut Search);
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == search.pid
            && IsWindowVisible(hwnd) != 0
            && GetWindow(hwnd, GW_OWNER).is_null()
            && GetWindowTextLengthW(hwnd) > 0
        {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect) != 0 {
                let area = i64::from(rect.right - rect.left) * i64::from(rect.bottom - rect.top);
                if area > search.area {
                    search.hwnd = hwnd;
                    search.area = area;
                }
            }
        }
        1
    }

    fn find(pid: u32) -> Result<HWND> {
        let mut search = Search {
            pid,
            hwnd: null_mut(),
            area: 0,
        };
        unsafe {
            EnumWindows(Some(visit), &mut search as *mut Search as LPARAM);
        }
        ensure!(!search.hwnd.is_null(), "No application window is available");
        Ok(search.hwnd)
    }

    pub fn focus(pid: u32) -> Result<()> {
        let hwnd = find(pid)?;
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindowAsync(hwnd, SW_RESTORE);
            }
            ensure!(
                SetForegroundWindow(hwnd) != 0,
                "Windows did not allow this application to take focus"
            );
        }
        Ok(())
    }

    // Release native resources even when capture or PNG encoding fails.
    struct Canvas {
        window: HWND,
        source: HDC,
        dc: HDC,
        bitmap: HBITMAP,
        previous: HGDIOBJ,
    }
    impl Drop for Canvas {
        fn drop(&mut self) {
            unsafe {
                if !self.previous.is_null() {
                    SelectObject(self.dc, self.previous);
                }
                if !self.bitmap.is_null() {
                    DeleteObject(self.bitmap);
                }
                if !self.dc.is_null() {
                    DeleteDC(self.dc);
                }
                if !self.source.is_null() {
                    ReleaseDC(self.window, self.source);
                }
            }
        }
    }

    pub fn preview(pid: u32) -> Result<serde_json::Value> {
        let hwnd = find(pid)?;
        let title = unsafe {
            let mut buffer = vec![0u16; (GetWindowTextLengthW(hwnd).max(0) + 1) as usize];
            let count = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
            String::from_utf16_lossy(&buffer[..count.max(0) as usize])
        };
        let image = capture(hwnd);
        Ok(match image {
            Ok(data_url) => {
                serde_json::json!({ "title": title, "image": data_url, "unavailable_reason": null, "can_focus": true })
            }
            Err(error) => {
                serde_json::json!({ "title": title, "image": null, "unavailable_reason": error.to_string(), "can_focus": true })
            }
        })
    }

    fn capture(hwnd: HWND) -> Result<String> {
        unsafe {
            ensure!(IsIconic(hwnd) == 0, "Window is minimized");
            ensure!(IsHungAppWindow(hwnd) == 0, "Application is not responding");
            let mut rect = RECT::default();
            ensure!(
                GetWindowRect(hwnd, &mut rect) != 0,
                "Window bounds are unavailable"
            );
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            ensure!(
                width > 0
                    && height > 0
                    && width <= 8192
                    && height <= 8192
                    && i64::from(width) * i64::from(height) <= 32_000_000,
                "Window size is not supported for preview"
            );
            let mut canvas = Canvas {
                window: hwnd,
                source: GetDC(hwnd),
                dc: null_mut(),
                bitmap: null_mut(),
                previous: null_mut(),
            };
            ensure!(!canvas.source.is_null(), "Window surface is unavailable");
            canvas.dc = CreateCompatibleDC(canvas.source);
            ensure!(!canvas.dc.is_null(), "Could not allocate preview surface");
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = null_mut();
            canvas.bitmap = CreateDIBSection(
                canvas.source,
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                null_mut(),
                0,
            );
            ensure!(
                !canvas.bitmap.is_null() && !bits.is_null(),
                "Could not allocate preview bitmap"
            );
            canvas.previous = SelectObject(canvas.dc, canvas.bitmap);
            std::ptr::write_bytes(bits as *mut u8, 0, width as usize * height as usize * 4);
            // Capture this window only; never fall back to a screen crop containing other windows.
            ensure!(
                PrintWindow(hwnd, canvas.dc, PW_RENDERFULLCONTENT) != 0,
                "Application did not provide a window preview"
            );
            let source =
                std::slice::from_raw_parts(bits as *const u8, width as usize * height as usize * 4);
            let scale = (640.0 / f64::from(width))
                .min(480.0 / f64::from(height))
                .min(1.0);
            let out_width = (f64::from(width) * scale).round().max(1.0) as usize;
            let out_height = (f64::from(height) * scale).round().max(1.0) as usize;
            let mut rgb = Vec::with_capacity(out_width * out_height * 3);
            for y in 0..out_height {
                for x in 0..out_width {
                    let i = ((y * height as usize / out_height) * width as usize
                        + x * width as usize / out_width)
                        * 4;
                    rgb.extend_from_slice(&[source[i + 2], source[i + 1], source[i]]);
                }
            }
            if rgb.iter().all(|value| *value == 0) {
                bail!("Application did not provide visible preview content");
            }
            let mut png = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut png, out_width as u32, out_height as u32);
                encoder.set_color(png::ColorType::Rgb);
                encoder.set_depth(png::BitDepth::Eight);
                encoder
                    .write_header()?
                    .write_image_data(&rgb)
                    .context("Could not encode window preview")?;
            }
            Ok(format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(png)
            ))
        }
    }
}
