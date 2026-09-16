use super::{PrivacyMode, INVALID_PRIVACY_MODE_CONN_ID};
use crate::{platform::windows::get_user_token, privacy_mode::PrivacyModeState};
use hbb_common::{allow_err, bail, log, ResultType};
use std::{
    ffi::CString,
    io::Error,
    mem::size_of,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use winapi::{
    shared::{
        minwindef::{BOOL, FALSE, LPARAM, TRUE},
        ntdef::{HANDLE, NULL},
        windef::{HDC, HMONITOR, HWND, RECT},
    },
    um::{
        handleapi::CloseHandle,
        libloaderapi::{GetModuleHandleA, GetProcAddress},
        memoryapi::{VirtualAllocEx, WriteProcessMemory},
        processthreadsapi::{
            CreateProcessAsUserW, QueueUserAPC, ResumeThread, TerminateProcess,
            PROCESS_INFORMATION, STARTUPINFOW,
        },
        winbase::{WTSGetActiveConsoleSessionId, CREATE_SUSPENDED, DETACHED_PROCESS},
        wingdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
            CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateSolidBrush,
            DEFAULT_CHARSET, DEFAULT_PITCH, DeleteDC, DeleteObject, DIB_RGB_COLORS, FW_BOLD,
            FW_LIGHT, FW_NORMAL, OUT_DEFAULT_PRECIS, RGBQUAD, SRCCOPY, SelectObject, SetBkMode,
            SetTextColor, StretchDIBits, BitBlt, TRANSPARENT,
        },
        winnt::{MEM_COMMIT, PAGE_READWRITE},
        winuser::*,
    },
};

pub(super) const PRIVACY_MODE_IMPL: &str = "privacy_mode_impl_mag";

pub const ORIGIN_PROCESS_EXE: &'static str = "C:\\Windows\\System32\\RuntimeBroker.exe";
pub const WIN_TOPMOST_INJECTED_PROCESS_EXE: &'static str = "RuntimeBroker_rustdesk.exe";
pub const INJECTED_PROCESS_EXE: &'static str = WIN_TOPMOST_INJECTED_PROCESS_EXE;
pub(super) const PRIVACY_WINDOW_CLASS: &'static str = "RustDeskPrivacyWindowClass";
pub(super) const PRIVACY_WINDOW_NAME: &'static str = "RustDeskPrivacyWindow";
const PRIVACY_WINDOW_WAIT_MILLIS: u128 = 1_000;
const PRIVACY_WINDOW_WAIT_EXTRA_MONITOR_MILLIS: u128 = 500;
const PRIVACY_WINDOW_POLL_INTERVAL_MILLIS: u64 = 100;
const WM_RUSTDESK_SHOW_WINDOWS: u32 = WM_APP + 3;
const WM_RUSTDESK_HIDE_WINDOWS: u32 = WM_APP + 4;

struct WindowHandlers {
    hthread: u64,
    hprocess: u64,
}

impl Drop for WindowHandlers {
    fn drop(&mut self) {
        self.reset();
    }
}

impl WindowHandlers {
    fn reset(&mut self) {
        unsafe {
            if self.hprocess != 0 {
                let _res = TerminateProcess(self.hprocess as _, 0);
                CloseHandle(self.hprocess as _);
            }
            self.hprocess = 0;
            if self.hthread != 0 {
                CloseHandle(self.hthread as _);
            }
            self.hthread = 0;
        }
    }

    fn is_default(&self) -> bool {
        self.hthread == 0 && self.hprocess == 0
    }
}

pub struct PrivacyModeImpl {
    impl_key: String,
    conn_id: i32,
    handlers: WindowHandlers,
    hwnd: u64,
    painter_running: Arc<AtomicBool>,
}

fn find_window_injection_dll() -> Option<std::path::PathBuf> {
    if let Ok(exe_file) = std::env::current_exe() {
        if let Some(cur_dir) = exe_file.parent() {
            let p = cur_dir.join("WindowInjection.dll");
            if p.exists() {
                return Some(p);
            }
        }
    }
    if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
        let p = std::path::Path::new(&appdata).join("rustdesk").join("WindowInjection.dll");
        if p.exists() {
            return Some(p);
        }
    }
    let p = std::path::Path::new(r"C:\Program Files\RustDesk\WindowInjection.dll");
    if p.exists() {
        return Some(p.to_path_buf());
    }
    None
}

impl PrivacyMode for PrivacyModeImpl {
    fn is_async_privacy_mode(&self) -> bool {
        false
    }

    fn init(&self) -> ResultType<()> {
        Ok(())
    }

    fn clear(&mut self) {
        allow_err!(self.turn_off_privacy(self.conn_id, None));
    }

    fn turn_on_privacy(&mut self, conn_id: i32) -> ResultType<bool> {
        if self.check_on_conn_id(conn_id)? {
            log::debug!("Privacy mode of conn {} is already on", conn_id);
            return Ok(true);
        }

        if find_window_injection_dll().is_none() {
            bail!("WindowInjection.dll is missing");
        }

        let should_start_broker = self.handlers.is_default();
        if should_start_broker {
            log::info!("turn_on_privacy, broker not running, try start");
            self.start()?;
            std::thread::sleep(std::time::Duration::from_millis(1_000));
        }

        if let Err(e) = self.show_privacy_windows(conn_id, true) {
            self.stop();
            return Err(e);
        }
        Ok(true)
    }

    fn turn_off_privacy(
        &mut self,
        conn_id: i32,
        state: Option<PrivacyModeState>,
    ) -> ResultType<()> {
        self.stop_painter();
        self.check_off_conn_id(conn_id)?;
        super::win_input::unhook()?;
        let hwnds = find_privacy_hwnds()?;
        let hide_result = set_privacy_windows_visible(&hwnds, false);
        if hide_result.is_err() {
            self.stop();
        }

        // Continue local state cleanup even after stop(); the broker has
        // been torn down, so keeping conn_id/hwnd would leave stale state.
        if self.conn_id != INVALID_PRIVACY_MODE_CONN_ID {
            // Only publish the off state after the hide message was posted.
            // Otherwise the peer may receive a success-like state and then a
            // failed turn-off response for the same request.
            if hide_result.is_ok() {
                if let Some(state) = state {
                    allow_err!(super::set_privacy_mode_state(
                        conn_id,
                        state,
                        PRIVACY_MODE_IMPL.to_string(),
                        1_000
                    ));
                }
            }
            self.conn_id = INVALID_PRIVACY_MODE_CONN_ID.to_owned();
            self.hwnd = 0;
        }

        hide_result.map(|_| ())
    }

    #[inline]
    fn pre_conn_id(&self) -> i32 {
        self.conn_id
    }

    #[inline]
    fn get_impl_key(&self) -> &str {
        &self.impl_key
    }
}

impl PrivacyModeImpl {
    pub fn new(impl_key: &str) -> Self {
        Self {
            impl_key: impl_key.to_owned(),
            conn_id: INVALID_PRIVACY_MODE_CONN_ID,
            handlers: WindowHandlers {
                hthread: 0,
                hprocess: 0,
            },
            hwnd: 0,
            painter_running: Arc::new(AtomicBool::new(false)),
        }
    }

    #[inline]
    pub fn get_hwnd(&self) -> u64 {
        self.hwnd
    }

    pub fn start(&mut self) -> ResultType<()> {
        if self.handlers.hprocess != 0 {
            return Ok(());
        }

        log::info!("Start privacy mode window broker, check_update_broker_process");
        if let Err(e) = crate::platform::windows::check_update_broker_process() {
            log::warn!(
                "Failed to check update broker process. Privacy mode may not work properly. {}",
                e
            );
        }

        let exe_file = std::env::current_exe()?;
        let Some(cur_dir) = exe_file.parent() else {
            bail!("Cannot get parent of current exe file");
        };

        let Some(dll_file) = find_window_injection_dll() else {
            bail!("Failed to find required file WindowInjection.dll");
        };

        if wait_find_privacy_hwnds(PRIVACY_WINDOW_WAIT_MILLIS).is_ok() {
            log::info!("Privacy window is ready");
            return Ok(());
        }

        // let cmdline = cur_dir.join("MiniBroker.exe").to_string_lossy().to_string();
        let cmdline = cur_dir
            .join(INJECTED_PROCESS_EXE)
            .to_string_lossy()
            .to_string();

        unsafe {
            let cmd_utf16: Vec<u16> = cmdline.encode_utf16().chain(Some(0).into_iter()).collect();

            let mut start_info = STARTUPINFOW {
                cb: size_of::<STARTUPINFOW>() as _,
                lpReserved: NULL as _,
                lpDesktop: NULL as _,
                lpTitle: NULL as _,
                dwX: 0,
                dwY: 0,
                dwXSize: 0,
                dwYSize: 0,
                dwXCountChars: 0,
                dwYCountChars: 0,
                dwFillAttribute: 0,
                dwFlags: 0,
                wShowWindow: 0,
                cbReserved2: 0,
                lpReserved2: NULL as _,
                hStdInput: NULL as _,
                hStdOutput: NULL as _,
                hStdError: NULL as _,
            };
            let mut proc_info = PROCESS_INFORMATION {
                hProcess: NULL as _,
                hThread: NULL as _,
                dwProcessId: 0,
                dwThreadId: 0,
            };

            let session_id = WTSGetActiveConsoleSessionId();
            let token = get_user_token(session_id, true);
            let mut create_res = 0;
            if !token.is_null() {
                create_res = CreateProcessAsUserW(
                    token,
                    NULL as _,
                    cmd_utf16.as_ptr() as _,
                    NULL as _,
                    NULL as _,
                    FALSE,
                    CREATE_SUSPENDED | DETACHED_PROCESS,
                    NULL,
                    NULL as _,
                    &mut start_info,
                    &mut proc_info,
                );
                CloseHandle(token);
            }
            if 0 == create_res {
                log::warn!("CreateProcessAsUserW failed (error: {}), trying CreateProcessW", Error::last_os_error());
                let mut cmd_utf16_mut = cmd_utf16.clone();
                create_res = winapi::um::processthreadsapi::CreateProcessW(
                    NULL as _,
                    cmd_utf16_mut.as_mut_ptr(),
                    NULL as _,
                    NULL as _,
                    FALSE,
                    CREATE_SUSPENDED | DETACHED_PROCESS,
                    NULL,
                    NULL as _,
                    &mut start_info,
                    &mut proc_info,
                );
            }
            if 0 == create_res {
                bail!(
                    "Failed to create privacy window process {}, error {}",
                    cmdline,
                    Error::last_os_error()
                );
            };

            if let Err(e) = inject_dll(
                proc_info.hProcess,
                proc_info.hThread,
                dll_file.to_string_lossy().as_ref(),
            ) {
                TerminateProcess(proc_info.hProcess, 0);
                CloseHandle(proc_info.hThread);
                CloseHandle(proc_info.hProcess);
                return Err(e);
            }

            if 0xffffffff == ResumeThread(proc_info.hThread) {
                TerminateProcess(proc_info.hProcess, 0);
                CloseHandle(proc_info.hThread);
                CloseHandle(proc_info.hProcess);

                bail!(
                    "Failed to create privacy window process, error {}",
                    Error::last_os_error()
                );
            }

            self.handlers.hthread = proc_info.hThread as _;
            self.handlers.hprocess = proc_info.hProcess as _;

            if let Err(e) = wait_find_privacy_hwnds(PRIVACY_WINDOW_WAIT_MILLIS) {
                self.handlers.reset();
                return Err(e);
            }
        }

        Ok(())
    }

    #[inline]
    pub fn stop(&mut self) {
        self.stop_painter();
        self.handlers.reset();
    }

    fn show_privacy_windows(&mut self, conn_id: i32, hook_input: bool) -> ResultType<()> {
        let hwnds = wait_find_privacy_hwnds(PRIVACY_WINDOW_WAIT_MILLIS)?;
        if hwnds.is_empty() {
            bail!("No privacy window created");
        }

        if hook_input {
            super::win_input::hook()?;
        }
        match set_privacy_windows_visible(&hwnds, true) {
            Ok(_) => {
                let visible_hwnds =
                    match wait_find_visible_privacy_hwnds(PRIVACY_WINDOW_WAIT_MILLIS) {
                        Ok(hwnds) => hwnds,
                        Err(e) => {
                            allow_err!(set_privacy_windows_visible(&hwnds, false));
                            if hook_input {
                                allow_err!(super::win_input::unhook());
                            }
                            return Err(e);
                        }
                    };
                let Some(hwnd) = visible_hwnds.first() else {
                    allow_err!(set_privacy_windows_visible(&hwnds, false));
                    if hook_input {
                        allow_err!(super::win_input::unhook());
                    }
                    bail!("No visible privacy window created");
                };
                self.conn_id = conn_id;
                self.hwnd = *hwnd as _;
                self.start_painter(visible_hwnds);
                Ok(())
            }
            Err(e) => {
                allow_err!(set_privacy_windows_visible(&hwnds, false));
                if hook_input {
                    allow_err!(super::win_input::unhook());
                }
                Err(e)
            }
        }
    }

    fn start_painter(&self, hwnds: Vec<HWND>) {
        if !super::PrivacyCustomization::is_custom_configured() {
            log::info!("Privacy mode is not customized, skipping custom screen painter");
            return;
        }

        self.painter_running.store(true, Ordering::SeqCst);
        let painter_running = self.painter_running.clone();

        std::thread::spawn(move || {
            log::info!(
                "Started custom privacy painter thread for {} monitors/hwnds",
                hwnds.len()
            );
            run_custom_privacy_painter(hwnds, painter_running);
            log::info!("Custom privacy painter thread stopped");
        });
    }

    fn stop_painter(&self) {
        self.painter_running.store(false, Ordering::SeqCst);
    }
}

impl Drop for PrivacyModeImpl {
    fn drop(&mut self) {
        self.stop_painter();
        if self.conn_id != INVALID_PRIVACY_MODE_CONN_ID {
            allow_err!(self.turn_off_privacy(self.conn_id, None));
        }
    }
}

fn parse_hex_color(hex_str: &str) -> (u8, u8, u8, u32) {
    let clean = hex_str.trim().trim_start_matches('#');
    let (r, g, b) = if clean.len() == 6 {
        let r = u8::from_str_radix(&clean[0..2], 16).unwrap_or(24);
        let g = u8::from_str_radix(&clean[2..4], 16).unwrap_or(24);
        let b = u8::from_str_radix(&clean[4..6], 16).unwrap_or(27);
        (r, g, b)
    } else {
        (24, 24, 27) // Default sleek dark #18181B
    };
    let colorref = (r as u32) | ((g as u32) << 8) | ((b as u32) << 16);
    (r, g, b, colorref)
}

fn run_custom_privacy_painter(hwnds: Vec<HWND>, running: Arc<AtomicBool>) {
    let bg_color_hex = crate::ui_interface::get_option("privacy_mode_bg_color".to_string());
    let custom_msg = crate::ui_interface::get_option("privacy_mode_custom_message".to_string());
    let logo_path = crate::ui_interface::get_option("privacy_mode_logo_path".to_string());

    let (bg_r, bg_g, bg_b, bg_colorref) = parse_hex_color(&bg_color_hex);
    let title_text = if custom_msg.trim().is_empty() {
        "Modo de Privacidade Ativo".to_string()
    } else {
        custom_msg
    };

    let logo_data: Option<(i32, i32, Vec<u8>)> = if !logo_path.is_empty() {
        match image::open(&logo_path) {
            Ok(dyn_img) => {
                let rgba = dyn_img.to_rgba8();
                let (orig_w, orig_h) = rgba.dimensions();
                if orig_w > 0 && orig_h > 0 {
                    let max_w = 280.0f32;
                    let max_h = 200.0f32;
                    let scale = (max_w / orig_w as f32).min(max_h / orig_h as f32).min(1.0);
                    let target_w = ((orig_w as f32 * scale).round() as u32).max(1);
                    let target_h = ((orig_h as f32 * scale).round() as u32).max(1);

                    let scaled_rgba = if target_w != orig_w || target_h != orig_h {
                        image::imageops::resize(
                            &rgba,
                            target_w,
                            target_h,
                            image::imageops::FilterType::Triangle,
                        )
                    } else {
                        rgba
                    };

                    let (w, h) = scaled_rgba.dimensions();
                    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
                    for pixel in scaled_rgba.pixels() {
                        let [r, g, b, a] = pixel.0;
                        let a_f = a as f32 / 255.0;
                        let r_out = ((r as f32 * a_f) + (bg_r as f32 * (1.0 - a_f))) as u8;
                        let g_out = ((g as f32 * a_f) + (bg_g as f32 * (1.0 - a_f))) as u8;
                        let b_out = ((b as f32 * a_f) + (bg_b as f32 * (1.0 - a_f))) as u8;
                        bgra.push(b_out);
                        bgra.push(g_out);
                        bgra.push(r_out);
                        bgra.push(255);
                    }
                    Some((w as i32, h as i32, bgra))
                } else {
                    None
                }
            }
            Err(e) => {
                log::warn!("Failed to load privacy logo from {:?}: {}", logo_path, e);
                None
            }
        }
    } else {
        None
    };

    while running.load(Ordering::SeqCst) {
        for &hwnd in &hwnds {
            unsafe {
                if FALSE != IsWindowVisible(hwnd) {
                    paint_single_window(hwnd, bg_colorref, logo_data.as_ref(), &title_text);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

unsafe fn paint_single_window(
    hwnd: HWND,
    bg_colorref: u32,
    logo_data: Option<&(i32, i32, Vec<u8>)>,
    title_text: &str,
) {
    let hdc = GetDC(hwnd);
    if hdc.is_null() {
        return;
    }

    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if FALSE == GetClientRect(hwnd, &mut rc) {
        ReleaseDC(hwnd, hdc);
        return;
    }
    let width = rc.right - rc.left;
    let height = rc.bottom - rc.top;
    if width <= 0 || height <= 0 {
        ReleaseDC(hwnd, hdc);
        return;
    }

    let mem_dc = CreateCompatibleDC(hdc);
    if mem_dc.is_null() {
        ReleaseDC(hwnd, hdc);
        return;
    }

    let mem_bmp = CreateCompatibleBitmap(hdc, width, height);
    if mem_bmp.is_null() {
        DeleteDC(mem_dc);
        ReleaseDC(hwnd, hdc);
        return;
    }

    let old_bmp = SelectObject(mem_dc, mem_bmp as _);

    // 1. Draw solid background
    let bg_brush = CreateSolidBrush(bg_colorref);
    FillRect(mem_dc, &rc, bg_brush);
    DeleteObject(bg_brush as _);

    SetBkMode(mem_dc, TRANSPARENT as _);

    let center_y = height / 2;
    let mut current_y = center_y - 120;

    // 2. Draw logo if available
    if let Some((lw, lh, bgra)) = logo_data {
        let draw_x = (width - lw) / 2;
        let draw_y = current_y - lh / 2;
        current_y += lh / 2 + 30;

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as _,
                biWidth: *lw,
                biHeight: -(*lh),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD {
                rgbBlue: 0,
                rgbGreen: 0,
                rgbRed: 0,
                rgbReserved: 0,
            }; 1],
        };

        StretchDIBits(
            mem_dc,
            draw_x,
            draw_y,
            *lw,
            *lh,
            0,
            0,
            *lw,
            *lh,
            bgra.as_ptr() as _,
            &bmi,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    } else {
        current_y = center_y - 60;
    }

    // 3. Draw Title (Custom Message)
    let font_family: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
    let title_font = CreateFontW(
        40,
        0,
        0,
        0,
        FW_BOLD as _,
        0,
        0,
        0,
        DEFAULT_CHARSET as _,
        OUT_DEFAULT_PRECIS as _,
        CLIP_DEFAULT_PRECIS as _,
        CLEARTYPE_QUALITY as _,
        DEFAULT_PITCH as _,
        font_family.as_ptr(),
    );
    let old_font = SelectObject(mem_dc, title_font as _);
    SetTextColor(mem_dc, 0x00FFFFFF);

    let title_utf16: Vec<u16> = title_text.encode_utf16().collect();
    let mut title_rc = RECT {
        left: 40,
        top: current_y,
        right: width - 40,
        bottom: current_y + 100,
    };
    DrawTextW(
        mem_dc,
        title_utf16.as_ptr(),
        title_utf16.len() as _,
        &mut title_rc,
        DT_CENTER | DT_WORDBREAK | DT_NOPREFIX,
    );

    // 4. Draw Subtitle
    let sub_font = CreateFontW(
        22,
        0,
        0,
        0,
        FW_NORMAL as _,
        0,
        0,
        0,
        DEFAULT_CHARSET as _,
        OUT_DEFAULT_PRECIS as _,
        CLIP_DEFAULT_PRECIS as _,
        CLEARTYPE_QUALITY as _,
        DEFAULT_PITCH as _,
        font_family.as_ptr(),
    );
    SelectObject(mem_dc, sub_font as _);
    SetTextColor(mem_dc, 0x00CBD5E1);

    let sub_text = "A tela remota está protegida para garantir sua privacidade.";
    let sub_utf16: Vec<u16> = sub_text.encode_utf16().collect();
    let mut sub_rc = RECT {
        left: 40,
        top: title_rc.bottom + 10,
        right: width - 40,
        bottom: title_rc.bottom + 60,
    };
    DrawTextW(
        mem_dc,
        sub_utf16.as_ptr(),
        sub_utf16.len() as _,
        &mut sub_rc,
        DT_CENTER | DT_WORDBREAK | DT_NOPREFIX,
    );

    // 5. Draw Footer Tip
    let tip_font = CreateFontW(
        16,
        0,
        0,
        0,
        FW_LIGHT as _,
        0,
        0,
        0,
        DEFAULT_CHARSET as _,
        OUT_DEFAULT_PRECIS as _,
        CLIP_DEFAULT_PRECIS as _,
        CLEARTYPE_QUALITY as _,
        DEFAULT_PITCH as _,
        font_family.as_ptr(),
    );
    SelectObject(mem_dc, tip_font as _);
    SetTextColor(mem_dc, 0x0094A3B8);

    let tip_text = "Pressione Ctrl + P para desbloquear a tela localmente";
    let tip_utf16: Vec<u16> = tip_text.encode_utf16().collect();
    let mut tip_rc = RECT {
        left: 40,
        top: height - 60,
        right: width - 40,
        bottom: height - 20,
    };
    DrawTextW(
        mem_dc,
        tip_utf16.as_ptr(),
        tip_utf16.len() as _,
        &mut tip_rc,
        DT_CENTER | DT_SINGLELINE | DT_NOPREFIX,
    );

    BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY);

    SelectObject(mem_dc, old_font);
    DeleteObject(title_font as _);
    DeleteObject(sub_font as _);
    DeleteObject(tip_font as _);
    SelectObject(mem_dc, old_bmp);
    DeleteObject(mem_bmp as _);
    DeleteDC(mem_dc);
    ReleaseDC(hwnd, hdc);
}

unsafe fn inject_dll<'a>(hproc: HANDLE, hthread: HANDLE, dll_file: &'a str) -> ResultType<()> {
    let dll_file_utf16: Vec<u16> = dll_file.encode_utf16().chain(Some(0).into_iter()).collect();

    let buf = VirtualAllocEx(
        hproc,
        NULL as _,
        dll_file_utf16.len() * 2,
        MEM_COMMIT,
        PAGE_READWRITE,
    );
    if buf.is_null() {
        bail!("Failed VirtualAllocEx");
    }

    let mut written: usize = 0;
    if 0 == WriteProcessMemory(
        hproc,
        buf,
        dll_file_utf16.as_ptr() as _,
        dll_file_utf16.len() * 2,
        &mut written,
    ) {
        bail!("Failed WriteProcessMemory");
    }

    let kernel32_modulename = CString::new("kernel32")?;
    let hmodule = GetModuleHandleA(kernel32_modulename.as_ptr() as _);
    if hmodule.is_null() {
        bail!("Failed GetModuleHandleA");
    }

    let load_librarya_name = CString::new("LoadLibraryW")?;
    let load_librarya = GetProcAddress(hmodule, load_librarya_name.as_ptr() as _);
    if load_librarya.is_null() {
        bail!("Failed GetProcAddress of LoadLibraryW");
    }

    if 0 == QueueUserAPC(Some(std::mem::transmute(load_librarya)), hthread, buf as _) {
        bail!("Failed QueueUserAPC");
    }

    Ok(())
}

fn wait_find_privacy_hwnds(msecs: u128) -> ResultType<Vec<HWND>> {
    wait_find_privacy_hwnds_impl(msecs, false)
}

fn wait_find_visible_privacy_hwnds(msecs: u128) -> ResultType<Vec<HWND>> {
    wait_find_privacy_hwnds_impl(msecs, true)
}

fn privacy_window_wait_millis(base_millis: u128, monitor_count: usize) -> u128 {
    if base_millis == 0 {
        return 0;
    }
    // Privacy Mode 1 creates one overlay per monitor. Keep the single-monitor
    // wait as the base and add time for each extra overlay before coverage
    // verification times out.
    base_millis
        + (monitor_count.saturating_sub(1) as u128) * PRIVACY_WINDOW_WAIT_EXTRA_MONITOR_MILLIS
}

fn wait_find_privacy_hwnds_impl(msecs: u128, require_visible: bool) -> ResultType<Vec<HWND>> {
    // This verifies initial turn-on coverage. If displays change during this
    // short poll window, the DLL refreshes overlays asynchronously, while this
    // check may still time out against the geometry sampled here.
    let monitor_rects = get_monitor_rects()?;
    if monitor_rects.is_empty() {
        bail!("No privacy monitor found");
    }
    let msecs = privacy_window_wait_millis(msecs, monitor_rects.len());

    let tm_begin = Instant::now();
    loop {
        let hwnds = find_privacy_hwnds()?;
        let visible_hwnds = if require_visible {
            filter_visible_hwnds(&hwnds)
        } else {
            Vec::new()
        };
        let covered_hwnds = if require_visible {
            visible_hwnds.as_slice()
        } else {
            hwnds.as_slice()
        };
        let covered = count_covered_monitors(covered_hwnds, &monitor_rects);
        if covered == monitor_rects.len() {
            return Ok(if require_visible {
                visible_hwnds
            } else {
                hwnds
            });
        }

        if msecs == 0 || tm_begin.elapsed().as_millis() > msecs {
            let visible = if require_visible { "visible " } else { "" };
            bail!(
                "Expected {}privacy windows to cover {} monitors, covered {}, found {}",
                visible,
                monitor_rects.len(),
                covered,
                hwnds.len(),
            );
        }

        std::thread::sleep(Duration::from_millis(PRIVACY_WINDOW_POLL_INTERVAL_MILLIS));
    }
}

fn find_privacy_hwnds() -> ResultType<Vec<HWND>> {
    let class_name = CString::new(PRIVACY_WINDOW_CLASS)?;
    let wndname = CString::new(PRIVACY_WINDOW_NAME)?;
    let mut hwnds = Vec::new();
    unsafe {
        let mut after = NULL as _;
        loop {
            let hwnd = FindWindowExA(
                NULL as _,
                after,
                class_name.as_ptr() as _,
                wndname.as_ptr() as _,
            );
            if hwnd.is_null() {
                break;
            }
            hwnds.push(hwnd);
            after = hwnd;
        }
    }
    Ok(hwnds)
}

fn filter_visible_hwnds(hwnds: &[HWND]) -> Vec<HWND> {
    hwnds
        .iter()
        .copied()
        .filter(|hwnd| unsafe { FALSE != IsWindowVisible(*hwnd) })
        .collect()
}

fn set_privacy_windows_visible(hwnds: &[HWND], show: bool) -> ResultType<usize> {
    if hwnds.is_empty() {
        return Ok(0);
    };
    let message = if show {
        WM_RUSTDESK_SHOW_WINDOWS
    } else {
        WM_RUSTDESK_HIDE_WINDOWS
    };
    let mut posted = 0;
    let mut first_error = None;
    for &hwnd in hwnds {
        unsafe {
            if FALSE == PostMessageA(hwnd, message, 0, 0) {
                if first_error.is_none() {
                    first_error = Some(Error::last_os_error());
                }
            } else {
                posted += 1;
            }
        }
    }
    if let Some(error) = first_error {
        bail!(
            "Failed to post privacy window visibility message to all privacy windows, posted {}/{}, first error {}",
            posted,
            hwnds.len(),
            error,
        );
    }
    Ok(posted)
}

fn get_monitor_rects() -> ResultType<Vec<RECT>> {
    let mut rects = Vec::new();
    unsafe {
        if FALSE
            == EnumDisplayMonitors(
                NULL as _,
                NULL as _,
                Some(enum_monitor_rect_proc),
                &mut rects as *mut Vec<RECT> as LPARAM,
            )
        {
            bail!(
                "Failed EnumDisplayMonitors, error {}",
                Error::last_os_error()
            );
        }
    }
    Ok(rects)
}

unsafe extern "system" fn enum_monitor_rect_proc(
    hmon: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let rects = &mut *(lparam as *mut Vec<RECT>);
    let mut monitor_info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as _,
        rcMonitor: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        rcWork: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        dwFlags: 0,
    };
    if FALSE == GetMonitorInfoA(hmon, &mut monitor_info) {
        return FALSE;
    }
    rects.push(monitor_info.rcMonitor);
    TRUE
}

fn count_covered_monitors(hwnds: &[HWND], monitor_rects: &[RECT]) -> usize {
    let mut covered = 0;
    for monitor_rect in monitor_rects {
        for hwnd in hwnds {
            let mut window_rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            unsafe {
                if FALSE == GetWindowRect(*hwnd, &mut window_rect) {
                    log::warn!(
                        "Failed GetWindowRect for privacy window, error {}",
                        Error::last_os_error()
                    );
                    continue;
                }
            }
            if rect_covers(&window_rect, monitor_rect) {
                covered += 1;
                break;
            }
        }
    }
    covered
}

fn rect_covers(window_rect: &RECT, monitor_rect: &RECT) -> bool {
    window_rect.left <= monitor_rect.left
        && window_rect.top <= monitor_rect.top
        && window_rect.right >= monitor_rect.right
        && window_rect.bottom >= monitor_rect.bottom
}
