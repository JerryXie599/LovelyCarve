//! 桌面端原生窗口截图内联插件（Win32 `PrintWindow` / `BitBlt` + 纯 Rust PNG 编码）。
//!
//! 仅在 `desktop` feature 下编译。为了避免触发 `cli_rpc` 代码生成要求 CLI 也实现窗口句柄，
//! 本插件按 `preview_window.rs` 的模式注册为 Tauri InlinedPlugin (`desktop-capture`)。

use std::io::Write;

use flate2::{write::ZlibEncoder, Compression};
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::app::error::AppError;
use crate::core::casework::contest;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CaptureRegion {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CaptureWindowOptions {
    pin_id: String,
    #[serde(default)]
    region: Option<CaptureRegion>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CaptureWindowResult {
    pub pin_id: String,
    pub screenshot_rel: String,
    pub screenshot_at: String,
    pub width: u32,
    pub height: u32,
}

#[tauri::command]
async fn capture_window(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    options: CaptureWindowOptions,
) -> Result<CaptureWindowResult, AppError> {
    if options.pin_id.trim().is_empty() {
        return Err(AppError::invalid_input("pinId must not be empty"));
    }
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::not_found("Main window is not open"))?;

    let scale_factor = window.scale_factor().unwrap_or(1.0);
    let inner_size = window
        .inner_size()
        .map_err(|err| AppError::internal(format!("query inner size: {err}")))?;
    if inner_size.width == 0 || inner_size.height == 0 {
        return Err(AppError::invalid_input("Main window has zero size"));
    }

    #[cfg(windows)]
    let raw_rgba = {
        let hwnd = window
            .hwnd()
            .map_err(|err| AppError::internal(format!("get window hwnd: {err}")))?;
        let hwnd_val = hwnd.0 as isize;
        let width = inner_size.width;
        let height = inner_size.height;
        tokio::task::spawn_blocking(move || capture_hwnd_rgba(hwnd_val, width, height))
            .await
            .map_err(|err| AppError::task_panicked("capture_window", err))?
            .map_err(AppError::internal)?
    };
    #[cfg(not(windows))]
    let raw_rgba: RgbaBuffer = {
        let _ = scale_factor;
        return Err(AppError::internal(
            "Native window capture is currently supported on Windows only",
        ));
    };

    let cropped = crop_rgba(raw_rgba, options.region.as_ref(), scale_factor);
    let png_bytes = encode_rgba_png(cropped.width, cropped.height, &cropped.pixels)
        .map_err(|err| AppError::internal(format!("encode PNG: {err}")))?;

    let case_dir = {
        let guard = state.active_case.lock().await;
        guard
            .as_ref()
            .map(|c| c.dir.clone())
            .ok_or_else(|| AppError::invalid_input("no active case"))?
    };

    let _contest_guard = state.contest_lock.lock().await;
    let mut ws = contest::load_workspace(&case_dir).map_err(AppError::from)?;
    let (screenshot_rel, screenshot_at) =
        contest::save_pin_png(&case_dir, &options.pin_id, &png_bytes).map_err(AppError::from)?;
    let pin = ws
        .pins
        .iter_mut()
        .find(|p| p.id == options.pin_id)
        .ok_or_else(|| AppError::not_found(format!("pin {} not found", options.pin_id)))?;
    pin.screenshot_rel = Some(screenshot_rel.clone());
    pin.screenshot_at = Some(screenshot_at.clone());
    contest::save_workspace(&case_dir, &mut ws).map_err(AppError::from)?;

    Ok(CaptureWindowResult {
        pin_id: options.pin_id,
        screenshot_rel,
        screenshot_at,
        width: cropped.width,
        height: cropped.height,
    })
}

struct RgbaBuffer {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

fn crop_rgba(buf: RgbaBuffer, region: Option<&CaptureRegion>, scale: f64) -> RgbaBuffer {
    let Some(r) = region else {
        return buf;
    };
    if !r.x.is_finite() || !r.y.is_finite() || !r.w.is_finite() || !r.h.is_finite() {
        return buf;
    }
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let rx = ((r.x * scale).round() as i64).clamp(0, buf.width as i64) as u32;
    let ry = ((r.y * scale).round() as i64).clamp(0, buf.height as i64) as u32;
    let rw = ((r.w * scale).round() as i64).max(1) as u32;
    let rh = ((r.h * scale).round() as i64).max(1) as u32;
    let cw = rw.min(buf.width.saturating_sub(rx)).max(1);
    let ch = rh.min(buf.height.saturating_sub(ry)).max(1);
    if rx == 0 && ry == 0 && cw == buf.width && ch == buf.height {
        return buf;
    }
    let mut out = Vec::with_capacity((cw as usize) * (ch as usize) * 4);
    let stride = (buf.width as usize) * 4;
    for row in ry..(ry + ch) {
        let start = (row as usize) * stride + (rx as usize) * 4;
        let end = start + (cw as usize) * 4;
        if end <= buf.pixels.len() {
            out.extend_from_slice(&buf.pixels[start..end]);
        }
    }
    if out.len() != (cw as usize) * (ch as usize) * 4 {
        return buf;
    }
    RgbaBuffer {
        width: cw,
        height: ch,
        pixels: out,
    }
}

#[cfg(windows)]
fn capture_hwnd_rgba(hwnd_val: isize, width: u32, height: u32) -> Result<RgbaBuffer, String> {
    use std::ptr::null_mut;
    use winapi::shared::windef::{HWND, RECT};
    use winapi::um::wingdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
    };
    use winapi::um::winuser::{GetClientRect, GetDC, PrintWindow, ReleaseDC};

    const PW_CLIENTONLY: u32 = 0x00000001;
    const PW_RENDERFULLCONTENT: u32 = 0x00000002;

    let hwnd = hwnd_val as HWND;
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let (w, h) = unsafe {
        if GetClientRect(hwnd, &mut rect) != 0 && rect.right > rect.left && rect.bottom > rect.top {
            (
                (rect.right - rect.left) as u32,
                (rect.bottom - rect.top) as u32,
            )
        } else {
            (width, height)
        }
    };
    if w == 0 || h == 0 {
        return Err("client rect has zero dimension".to_string());
    }

    unsafe {
        let hdc_window = GetDC(hwnd);
        if hdc_window.is_null() {
            return Err("GetDC returned null".to_string());
        }
        let hdc_mem = CreateCompatibleDC(hdc_window);
        if hdc_mem.is_null() {
            ReleaseDC(hwnd, hdc_window);
            return Err("CreateCompatibleDC failed".to_string());
        }
        let hbm = CreateCompatibleBitmap(hdc_window, w as i32, h as i32);
        if hbm.is_null() {
            DeleteDC(hdc_mem);
            ReleaseDC(hwnd, hdc_window);
            return Err("CreateCompatibleBitmap failed".to_string());
        }
        let old = SelectObject(hdc_mem, hbm as *mut _);

        // 先尝试 PrintWindow(PW_CLIENTONLY | PW_RENDERFULLCONTENT) 捕获含 WebView2 合成的客户区
        let ok = PrintWindow(hwnd, hdc_mem, PW_CLIENTONLY | PW_RENDERFULLCONTENT) != 0;
        if !ok {
            BitBlt(hdc_mem, 0, 0, w as i32, h as i32, hdc_window, 0, 0, SRCCOPY);
        }

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [std::mem::zeroed(); 1],
        };
        let mut bgra = vec![0u8; (w as usize) * (h as usize) * 4];
        let scan_lines = GetDIBits(
            hdc_mem,
            hbm,
            0,
            h,
            bgra.as_mut_ptr() as *mut _,
            &mut bmi as *mut _,
            DIB_RGB_COLORS,
        );
        SelectObject(hdc_mem, old);
        DeleteObject(hbm as *mut _);
        DeleteDC(hdc_mem);
        ReleaseDC(hwnd, hdc_window);

        if scan_lines == 0 {
            // 最后兜底检查非空返回
            let _ = null_mut::<u8>();
            return Err("GetDIBits failed to copy bitmap pixels".to_string());
        }

        // 某些情况下 WebView2 合成层对 PrintWindow 返回全零透明帧，若全零则用屏幕 DC BitBlt 回退
        let non_zero = bgra.iter().any(|&b| b != 0);
        if !non_zero {
            let hdc_win_fb = GetDC(hwnd);
            if !hdc_win_fb.is_null() {
                let hdc_mem_fb = CreateCompatibleDC(hdc_win_fb);
                if !hdc_mem_fb.is_null() {
                    let hbm_fb = CreateCompatibleBitmap(hdc_win_fb, w as i32, h as i32);
                    if !hbm_fb.is_null() {
                        let old_fb = SelectObject(hdc_mem_fb, hbm_fb as *mut _);
                        BitBlt(
                            hdc_mem_fb, 0, 0, w as i32, h as i32, hdc_win_fb, 0, 0, SRCCOPY,
                        );
                        GetDIBits(
                            hdc_mem_fb,
                            hbm_fb,
                            0,
                            h,
                            bgra.as_mut_ptr() as *mut _,
                            &mut bmi as *mut _,
                            DIB_RGB_COLORS,
                        );
                        SelectObject(hdc_mem_fb, old_fb);
                        DeleteObject(hbm_fb as *mut _);
                    }
                    DeleteDC(hdc_mem_fb);
                }
                ReleaseDC(hwnd, hdc_win_fb);
            }
        }

        // BGRA -> RGBA（Alpha 固定为 255，确保网页与 Markdown 正常不透明渲染）
        for chunk in bgra.chunks_exact_mut(4) {
            chunk.swap(0, 2);
            chunk[3] = 255;
        }
        Ok(RgbaBuffer {
            width: w,
            height: h,
            pixels: bgra,
        })
    }
}

/// 纯 Rust RGBA8 -> PNG 编码器，复用现有 `flate2` 与 `crc32fast`。
pub(crate) fn encode_rgba_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| "image dimension overflow".to_string())?;
    if rgba.len() != expected || width == 0 || height == 0 {
        return Err("invalid RGBA buffer length or dimensions".to_string());
    }

    let stride = (width as usize) * 4;
    let mut raw_scanlines = Vec::with_capacity((stride + 1) * (height as usize));
    for row in 0..(height as usize) {
        raw_scanlines.push(0u8); // filter type 0 (None)
        let start = row * stride;
        raw_scanlines.extend_from_slice(&rgba[start..start + stride]);
    }

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(&raw_scanlines)
        .map_err(|err| format!("zlib write: {err}"))?;
    let idat = encoder
        .finish()
        .map_err(|err| format!("zlib finish: {err}"))?;

    let mut png = Vec::with_capacity(64 + idat.len());
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8; // bit depth
    ihdr[9] = 6; // color type RGBA
    ihdr[10] = 0;
    ihdr[11] = 0;
    ihdr[12] = 0;
    write_chunk(&mut png, b"IHDR", &ihdr);
    write_chunk(&mut png, b"IDAT", &idat);
    write_chunk(&mut png, b"IEND", &[]);
    Ok(png)
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(kind);
    hasher.update(data);
    out.extend_from_slice(&hasher.finalize().to_be_bytes());
}

pub(crate) fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("desktop-capture")
        .invoke_handler(tauri::generate_handler![capture_window])
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_valid_png_signature_and_dimensions() {
        let rgba = vec![
            255, 0, 0, 255, // red
            0, 255, 0, 255, // green
            0, 0, 255, 255, // blue
            255, 255, 255, 255, // white
        ];
        let png = encode_rgba_png(2, 2, &rgba).expect("encode png");
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..20], &2u32.to_be_bytes());
        assert_eq!(&png[20..24], &2u32.to_be_bytes());
        assert!(png.windows(4).any(|w| w == b"IEND"));
    }
}
