//! Local application windows are shared by CLI and desktop callers, independently of Bridges.
use crate::HostError;
#[cfg(not(windows))]
use anyhow::bail;
use anyhow::{Context, Result};

#[cfg(all(test, windows))]
mod tests;

pub struct HostInfo {
    pub candidate: crate::HostCandidate,
    pub window: Option<WindowInfo>,
    /// Absent unless the caller explicitly requests image capture.
    pub preview: Option<WindowPreview>,
}

#[derive(serde::Serialize)]
#[cfg_attr(feature = "typescript", derive(specta::Type))]
pub struct WindowInfo {
    pub title: String,
    pub minimized: bool,
}

/// Callers choose how to encode the PNG for their own presentation boundary.
pub enum WindowPreview {
    Png(Vec<u8>),
    Unavailable(String),
}

// Timed-out native calls retain their permits until they actually finish.
static CAPTURE_SLOTS: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(2)));

pub async fn host_info(pid: u32, include_preview: bool) -> Result<HostInfo, HostError> {
    let mut info = tokio::task::spawn_blocking(move || {
        Ok::<_, HostError>(HostInfo {
            candidate: local_candidate(pid)?,
            window: window_info(pid),
            preview: None,
        })
    })
    .await
    .context("host lookup stopped")??;
    if include_preview {
        info.preview = Some(match capture_preview(pid).await {
            Ok(png) => WindowPreview::Png(png),
            Err(error) => WindowPreview::Unavailable(format!("{error:#}")),
        });
    }
    Ok(info)
}

async fn capture_preview(pid: u32) -> Result<Vec<u8>> {
    let permit = CAPTURE_SLOTS
        .clone()
        .try_acquire_owned()
        .map_err(|_| anyhow::anyhow!("window previews are busy"))?;
    let capture = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        capture(pid)
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), capture)
        .await
        .map_err(|_| anyhow::anyhow!("window preview timed out"))??
}

pub fn focus_application(pid: u32) -> Result<(), HostError> {
    Ok(focus(local_candidate(pid)?.pid)?)
}

fn local_candidate(pid: u32) -> Result<crate::HostCandidate, HostError> {
    crate::candidate(pid).ok_or(HostError::NotAHost(pid))
}

#[cfg(not(windows))]
fn window_info(_pid: u32) -> Option<WindowInfo> {
    None
}

#[cfg(not(windows))]
fn capture(_pid: u32) -> Result<Vec<u8>> {
    bail!("window previews are unavailable on this platform")
}
#[cfg(not(windows))]
fn focus(_pid: u32) -> Result<()> {
    bail!("window switching is unavailable on this platform")
}

#[cfg(windows)]
use platform::{capture, focus, window_info};

#[cfg(windows)]
mod platform {
    use super::WindowInfo;
    use anyhow::{Context, Result, bail, ensure};
    use std::{mem::size_of, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, RECT},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
            DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SelectObject,
        },
        Storage::Xps::PrintWindow,
        UI::WindowsAndMessaging::{
            EnumWindows, GW_OWNER, GetWindow, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
            GetWindowThreadProcessId, IsHungAppWindow, IsIconic, IsWindowVisible, PW_RENDERFULLCONTENT, SW_RESTORE,
            SetForegroundWindow, ShowWindowAsync,
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
        ensure!(!search.hwnd.is_null(), "no application window is available");
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
                "windows did not allow this application to take focus"
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

    pub fn window_info(pid: u32) -> Option<WindowInfo> {
        let hwnd = find(pid).ok()?;
        let title = unsafe {
            let mut buffer = vec![0u16; (GetWindowTextLengthW(hwnd).max(0) + 1) as usize];
            let count = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
            String::from_utf16_lossy(&buffer[..count.max(0) as usize])
        };
        Some(WindowInfo {
            title,
            minimized: unsafe { IsIconic(hwnd) != 0 },
        })
    }

    pub fn capture(pid: u32) -> Result<Vec<u8>> {
        let hwnd = find(pid)?;
        unsafe {
            ensure!(IsIconic(hwnd) == 0, "window is minimized");
            ensure!(IsHungAppWindow(hwnd) == 0, "application is not responding");
            let mut rect = RECT::default();
            ensure!(GetWindowRect(hwnd, &mut rect) != 0, "window bounds are unavailable");
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            ensure!(
                width > 0
                    && height > 0
                    && width <= 8192
                    && height <= 8192
                    && i64::from(width) * i64::from(height) <= 32_000_000,
                "window size is not supported for preview"
            );
            let mut canvas = Canvas {
                window: hwnd,
                source: GetDC(hwnd),
                dc: null_mut(),
                bitmap: null_mut(),
                previous: null_mut(),
            };
            ensure!(!canvas.source.is_null(), "window surface is unavailable");
            canvas.dc = CreateCompatibleDC(canvas.source);
            ensure!(!canvas.dc.is_null(), "could not allocate preview surface");
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
            canvas.bitmap = CreateDIBSection(canvas.source, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            ensure!(
                !canvas.bitmap.is_null() && !bits.is_null(),
                "could not allocate preview bitmap"
            );
            canvas.previous = SelectObject(canvas.dc, canvas.bitmap);
            std::ptr::write_bytes(bits as *mut u8, 0, width as usize * height as usize * 4);
            // Capture this window only; never fall back to a screen crop containing other windows.
            ensure!(
                PrintWindow(hwnd, canvas.dc, PW_RENDERFULLCONTENT) != 0,
                "application did not provide a window preview"
            );
            let source = std::slice::from_raw_parts(bits as *const u8, width as usize * height as usize * 4);
            let scale = (640.0 / f64::from(width)).min(480.0 / f64::from(height)).min(1.0);
            let out_width = (f64::from(width) * scale).round().max(1.0) as usize;
            let out_height = (f64::from(height) * scale).round().max(1.0) as usize;
            let mut rgb = Vec::with_capacity(out_width * out_height * 3);
            for y in 0..out_height {
                for x in 0..out_width {
                    let i = ((y * height as usize / out_height) * width as usize + x * width as usize / out_width) * 4;
                    rgb.extend_from_slice(&[source[i + 2], source[i + 1], source[i]]);
                }
            }
            if rgb.iter().all(|value| *value == 0) {
                bail!("application did not provide visible preview content");
            }
            let mut png = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut png, out_width as u32, out_height as u32);
                encoder.set_color(png::ColorType::Rgb);
                encoder.set_depth(png::BitDepth::Eight);
                encoder
                    .write_header()?
                    .write_image_data(&rgb)
                    .context("could not encode window preview")?;
            }
            Ok(png)
        }
    }
}
