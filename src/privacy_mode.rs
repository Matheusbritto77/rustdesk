use crate::ui_interface::get_option;
#[cfg(windows)]
use crate::{
    display_service,
    ipc::{connect, Data},
    platform::is_installed,
};
#[cfg(windows)]
use hbb_common::tokio;
use hbb_common::{anyhow::anyhow, bail, lazy_static, tokio::sync::oneshot, ResultType};
use serde_derive::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[cfg(windows)]
pub mod win_exclude_from_capture;
#[cfg(windows)]
mod win_input;
#[cfg(windows)]
pub mod win_mag;
#[cfg(windows)]
pub mod win_topmost_window;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(windows)]
mod win_virtual_display;
#[cfg(windows)]
pub use win_virtual_display::restore_reg_connectivity;

pub const INVALID_PRIVACY_MODE_CONN_ID: i32 = 0;
pub const OCCUPIED: &'static str = "Privacy occupied by another one.";
pub const TURN_OFF_OTHER_ID: &'static str =
    "Failed to turn off privacy mode that belongs to someone else.";
pub const NO_PHYSICAL_DISPLAYS: &'static str = "no_need_privacy_mode_no_physical_displays_tip";

pub const PRIVACY_MODE_IMPL_WIN_MAG: &str = "privacy_mode_impl_mag";
pub const PRIVACY_MODE_IMPL_WIN_EXCLUDE_FROM_CAPTURE: &str =
    "privacy_mode_impl_exclude_from_capture";
pub const PRIVACY_MODE_IMPL_WIN_VIRTUAL_DISPLAY: &str = "privacy_mode_impl_virtual_display";

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", content = "c")]
pub enum PrivacyModeState {
    OffSucceeded,
    OffByPeer,
    OffUnknown,
}

lazy_static::lazy_static! {
    pub static ref PRIVACY_MODE_LOGO_DATA: Arc<std::sync::RwLock<Vec<u8>>> = Arc::new(std::sync::RwLock::new(Vec::new()));
}

#[derive(Debug, Clone, Default)]
pub struct PrivacyCustomization {
    pub is_custom: bool,
    pub bg_color: String,
    pub custom_message: String,
    pub logo_data: Vec<u8>,
}

impl PrivacyCustomization {
    pub fn load_from_local() -> Self {
        let mut bg_color = crate::ui_interface::get_option("privacy_mode_bg_color".to_string());
        if bg_color.is_empty() {
            bg_color = hbb_common::config::LocalConfig::get_option("privacy_mode_bg_color");
        }

        let mut custom_message = crate::ui_interface::get_option("privacy_mode_custom_message".to_string());
        if custom_message.is_empty() {
            custom_message = hbb_common::config::LocalConfig::get_option("privacy_mode_custom_message");
        }

        let mut logo_path = crate::ui_interface::get_option("privacy_mode_logo_path".to_string());
        if logo_path.is_empty() {
            logo_path = hbb_common::config::LocalConfig::get_option("privacy_mode_logo_path");
        }

        // 1. Check in-memory logo data
        let mut logo_data = PRIVACY_MODE_LOGO_DATA.read().unwrap().clone();

        // 2. If memory is empty, try base64 option
        if logo_data.is_empty() {
            let mut b64 = crate::ui_interface::get_option("privacy_mode_logo_base64".to_string());
            if b64.is_empty() {
                b64 = hbb_common::config::LocalConfig::get_option("privacy_mode_logo_base64");
            }
            if !b64.is_empty() {
                if let Ok(bytes) = hbb_common::base64::decode(&b64) {
                    *PRIVACY_MODE_LOGO_DATA.write().unwrap() = bytes.clone();
                    logo_data = bytes;
                }
            }
        }

        // 3. If still empty, read from logo_path file on disk
        if logo_data.is_empty() && !logo_path.is_empty() {
            // Also test without '/Volumes/Macintosh HD' if present
            let path_candidates = vec![
                std::path::PathBuf::from(&logo_path),
                if logo_path.starts_with("/Volumes/Macintosh HD/") {
                    std::path::PathBuf::from(logo_path.trim_start_matches("/Volumes/Macintosh HD"))
                } else {
                    std::path::PathBuf::from(&logo_path)
                },
            ];
            for p in path_candidates {
                if let Ok(bytes) = std::fs::read(&p) {
                    hbb_common::log::info!("Loaded {} bytes from logo path {:?}", bytes.len(), p);
                    *PRIVACY_MODE_LOGO_DATA.write().unwrap() = bytes.clone();
                    logo_data = bytes;
                    break;
                }
            }
        }

        let is_custom = !logo_data.is_empty()
            || (!custom_message.is_empty() && custom_message != "Modo de Privacidade Ativo")
            || (!bg_color.is_empty() && bg_color != "#000000");

        Self {
            is_custom: is_custom || !logo_data.is_empty(),
            bg_color,
            custom_message,
            logo_data,
        }
    }

    pub fn save_to_local(&self) {
        crate::ui_interface::set_option(
            "privacy_mode_is_custom".to_string(),
            if self.is_custom { "Y" } else { "N" }.to_string(),
        );
        hbb_common::config::LocalConfig::set_option(
            "privacy_mode_is_custom".to_string(),
            if self.is_custom { "Y" } else { "N" }.to_string(),
        );

        if !self.bg_color.is_empty() {
            crate::ui_interface::set_option("privacy_mode_bg_color".to_string(), self.bg_color.clone());
            hbb_common::config::LocalConfig::set_option("privacy_mode_bg_color".to_string(), self.bg_color.clone());
        }
        if !self.custom_message.is_empty() {
            crate::ui_interface::set_option("privacy_mode_custom_message".to_string(), self.custom_message.clone());
            hbb_common::config::LocalConfig::set_option("privacy_mode_custom_message".to_string(), self.custom_message.clone());
        }
        if !self.logo_data.is_empty() {
            *PRIVACY_MODE_LOGO_DATA.write().unwrap() = self.logo_data.clone();
            hbb_common::log::info!("Stored privacy mode logo in memory ({} bytes)", self.logo_data.len());

            let logo_path = hbb_common::config::Config::path("privacy_mode_logo.png");
            if let Ok(_) = std::fs::write(&logo_path, &self.logo_data) {
                hbb_common::log::info!("Saved custom privacy mode logo to disk {:?}", logo_path);
                crate::ui_interface::set_option("privacy_mode_logo_path".to_string(), logo_path.to_string_lossy().to_string());
                hbb_common::config::LocalConfig::set_option("privacy_mode_logo_path".to_string(), logo_path.to_string_lossy().to_string());
            }
        }
    }

    pub fn is_custom_configured() -> bool {
        if !PRIVACY_MODE_LOGO_DATA.read().unwrap().is_empty() {
            return true;
        }
        let is_custom_opt = crate::ui_interface::get_option("privacy_mode_is_custom".to_string());
        if is_custom_opt == "Y" || hbb_common::config::LocalConfig::get_option("privacy_mode_is_custom") == "Y" {
            return true;
        }
        let logo_path = crate::ui_interface::get_option("privacy_mode_logo_path".to_string());
        if !logo_path.is_empty() && std::path::Path::new(&logo_path).exists() {
            return true;
        }
        let msg = crate::ui_interface::get_option("privacy_mode_custom_message".to_string());
        if !msg.is_empty() && msg != "Modo de Privacidade Ativo" {
            return true;
        }
        let bg = crate::ui_interface::get_option("privacy_mode_bg_color".to_string());
        if !bg.is_empty() && bg != "#000000" && bg != "#18181B" {
            return true;
        }
        false
    }
}

pub trait PrivacyMode: Sync + Send {
    fn is_async_privacy_mode(&self) -> bool;

    fn init(&self) -> ResultType<()>;
    fn clear(&mut self);
    fn turn_on_privacy(&mut self, conn_id: i32) -> ResultType<bool>;
    fn turn_off_privacy(&mut self, conn_id: i32, state: Option<PrivacyModeState>)
        -> ResultType<()>;

    fn pre_conn_id(&self) -> i32;

    fn get_impl_key(&self) -> &str;

    #[inline]
    fn check_on_conn_id(&self, conn_id: i32) -> ResultType<bool> {
        let pre_conn_id = self.pre_conn_id();
        if pre_conn_id == conn_id {
            return Ok(true);
        }
        if pre_conn_id != INVALID_PRIVACY_MODE_CONN_ID {
            bail!(OCCUPIED);
        }
        Ok(false)
    }

    #[inline]
    fn check_off_conn_id(&self, conn_id: i32) -> ResultType<()> {
        let pre_conn_id = self.pre_conn_id();
        if pre_conn_id != INVALID_PRIVACY_MODE_CONN_ID
            && conn_id != INVALID_PRIVACY_MODE_CONN_ID
            && pre_conn_id != conn_id
        {
            bail!(TURN_OFF_OTHER_ID)
        }
        Ok(())
    }
}

lazy_static::lazy_static! {
    pub static ref DEFAULT_PRIVACY_MODE_IMPL: String = {
        #[cfg(windows)]
        {
            if win_exclude_from_capture::is_supported() {
                PRIVACY_MODE_IMPL_WIN_EXCLUDE_FROM_CAPTURE
            } else {
                if display_service::is_privacy_mode_mag_supported() {
                    PRIVACY_MODE_IMPL_WIN_MAG
                } else {
                    if is_installed() {
                        PRIVACY_MODE_IMPL_WIN_VIRTUAL_DISPLAY
                    } else {
                        ""
                    }
                }
            }.to_owned()
        }
        #[cfg(not(windows))]
        {
            #[cfg(target_os = "macos")]
            {
                macos::PRIVACY_MODE_IMPL.to_owned()
            }
            #[cfg(not(target_os = "macos"))]
            {
                "".to_owned()
            }
        }
    };

    static ref PRIVACY_MODE: Arc<Mutex<Option<Box<dyn PrivacyMode>>>> = {
        let mut cur_impl = get_option("privacy-mode-impl-key".to_owned());
        if !get_supported_privacy_mode_impl().iter().any(|(k, _)| k == &cur_impl) {
            cur_impl = DEFAULT_PRIVACY_MODE_IMPL.to_owned();
        }

        let privacy_mode = match PRIVACY_MODE_CREATOR.lock().unwrap().get(&(&cur_impl as &str)) {
            Some(creator) => Some(creator(&cur_impl)),
            None => None,
        };
        Arc::new(Mutex::new(privacy_mode))
    };
}

pub type PrivacyModeCreator = fn(impl_key: &str) -> Box<dyn PrivacyMode>;
lazy_static::lazy_static! {
    static ref PRIVACY_MODE_CREATOR: Arc<Mutex<HashMap<&'static str, PrivacyModeCreator>>> = {
        #[cfg(not(windows))]
        let mut map: HashMap<&'static str, PrivacyModeCreator> = HashMap::new();
        #[cfg(target_os = "macos")]
        {
            map.insert(macos::PRIVACY_MODE_IMPL, |impl_key: &str| {
                Box::new(macos::PrivacyModeImpl::new(impl_key))
            });
        }
        #[cfg(windows)]
        let mut map: HashMap<&'static str, PrivacyModeCreator> = HashMap::new();
        #[cfg(windows)]
        {
            if win_exclude_from_capture::is_supported() {
                map.insert(win_exclude_from_capture::PRIVACY_MODE_IMPL, |impl_key: &str| {
                    Box::new(win_exclude_from_capture::PrivacyModeImpl::new(impl_key))
                });
            } else {
                map.insert(win_mag::PRIVACY_MODE_IMPL, |impl_key: &str| {
                    Box::new(win_mag::PrivacyModeImpl::new(impl_key))
                });
            }

            map.insert(win_virtual_display::PRIVACY_MODE_IMPL, |impl_key: &str| {
                    Box::new(win_virtual_display::PrivacyModeImpl::new(impl_key))
                });
        }
        Arc::new(Mutex::new(map))
    };
}

#[inline]
pub fn init() -> Option<ResultType<()>> {
    Some(PRIVACY_MODE.lock().unwrap().as_ref()?.init())
}

#[inline]
pub fn clear() -> Option<()> {
    Some(PRIVACY_MODE.lock().unwrap().as_mut()?.clear())
}

#[inline]
pub fn switch(impl_key: &str) {
    let mut privacy_mode_lock = PRIVACY_MODE.lock().unwrap();
    if let Some(privacy_mode) = privacy_mode_lock.as_ref() {
        if privacy_mode.get_impl_key() == impl_key {
            return;
        }
    }

    if let Some(creator) = PRIVACY_MODE_CREATOR.lock().unwrap().get(impl_key) {
        *privacy_mode_lock = Some(creator(impl_key));
    }
}

fn get_supported_impl(impl_key: &str) -> String {
    let supported_impls = get_supported_privacy_mode_impl();
    if supported_impls.iter().any(|(k, _)| k == &impl_key) {
        return impl_key.to_owned();
    };
    // TODO: Is it a good idea to use fallback here? Because user do not know the fallback.
    // fallback
    let mut cur_impl = get_option("privacy-mode-impl-key".to_owned());
    if !get_supported_privacy_mode_impl()
        .iter()
        .any(|(k, _)| k == &cur_impl)
    {
        // fallback
        cur_impl = DEFAULT_PRIVACY_MODE_IMPL.to_owned();
    }
    cur_impl
}

pub async fn turn_on_privacy(impl_key: &str, conn_id: i32) -> Option<ResultType<bool>> {
    if is_async_privacy_mode() {
        turn_on_privacy_async(impl_key.to_string(), conn_id).await
    } else {
        turn_on_privacy_sync(impl_key, conn_id)
    }
}

#[inline]
fn is_async_privacy_mode() -> bool {
    PRIVACY_MODE
        .lock()
        .unwrap()
        .as_ref()
        .map_or(false, |m| m.is_async_privacy_mode())
}

#[inline]
async fn turn_on_privacy_async(impl_key: String, conn_id: i32) -> Option<ResultType<bool>> {
    let (tx, rx) = oneshot::channel();
    std::thread::spawn(move || {
        let res = turn_on_privacy_sync(&impl_key, conn_id);
        let _ = tx.send(res);
    });
    // Wait at most 7.5 seconds for the result.
    // Because it may take a long time to turn on the privacy mode with amyuni idd.
    // Some laptops may take time to plug in a virtual display.
    match hbb_common::timeout(7500, rx).await {
        Ok(res) => match res {
            Ok(res) => res,
            Err(e) => Some(Err(anyhow!(e.to_string()))),
        },
        Err(e) => Some(Err(anyhow!(e.to_string()))),
    }
}

fn turn_on_privacy_sync(impl_key: &str, conn_id: i32) -> Option<ResultType<bool>> {
    // Check if privacy mode is already on or occupied by another one
    let mut privacy_mode_lock = PRIVACY_MODE.lock().unwrap();

    // Check or switch privacy mode implementation
    let impl_key = get_supported_impl(impl_key);

    let mut cur_impl_key = "".to_string();
    if let Some(privacy_mode) = privacy_mode_lock.as_ref() {
        cur_impl_key = privacy_mode.get_impl_key().to_string();
        let check_on_conn_id = privacy_mode.check_on_conn_id(conn_id);
        match check_on_conn_id.as_ref() {
            Ok(true) => {
                if cur_impl_key == impl_key {
                    // Same peer, same implementation.
                    return Some(Ok(true));
                } else {
                    // Same peer, switch to new implementation.
                }
            }
            Err(_) => return Some(check_on_conn_id),
            _ => {}
        }
    }

    if cur_impl_key != impl_key {
        if let Some(creator) = PRIVACY_MODE_CREATOR
            .lock()
            .unwrap()
            .get(&(&impl_key as &str))
        {
            if let Some(privacy_mode) = privacy_mode_lock.as_mut() {
                privacy_mode.clear();
            }

            *privacy_mode_lock = Some(creator(&impl_key));
        } else {
            return Some(Err(anyhow!("Unsupported privacy mode: {}", impl_key)));
        }
    }

    // turn on privacy mode
    Some(privacy_mode_lock.as_mut()?.turn_on_privacy(conn_id))
}

#[inline]
pub fn turn_off_privacy(conn_id: i32, state: Option<PrivacyModeState>) -> Option<ResultType<()>> {
    Some(
        PRIVACY_MODE
            .lock()
            .unwrap()
            .as_mut()?
            .turn_off_privacy(conn_id, state),
    )
}

#[inline]
pub fn check_on_conn_id(conn_id: i32) -> Option<ResultType<bool>> {
    Some(
        PRIVACY_MODE
            .lock()
            .unwrap()
            .as_ref()?
            .check_on_conn_id(conn_id),
    )
}

#[cfg(windows)]
async fn set_privacy_mode_state_async(
    conn_id: i32,
    state: PrivacyModeState,
    impl_key: String,
    ms_timeout: u64,
) -> ResultType<()> {
    let mut c = connect(ms_timeout, "_cm").await?;
    c.send(&Data::PrivacyModeState((conn_id, state, impl_key)))
        .await
}

#[cfg(windows)]
fn set_privacy_mode_state(
    conn_id: i32,
    state: PrivacyModeState,
    impl_key: String,
    ms_timeout: u64,
) -> ResultType<()> {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            set_privacy_mode_state_async(conn_id, state, impl_key, ms_timeout).await.ok();
        });
        Ok(())
    } else {
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build();
            if let Ok(rt) = rt {
                rt.block_on(set_privacy_mode_state_async(conn_id, state, impl_key, ms_timeout)).ok();
            }
        });
        Ok(())
    }
}

pub fn get_supported_privacy_mode_impl() -> Vec<(&'static str, &'static str)> {
    #[cfg(target_os = "windows")]
    {
        let mut vec_impls = Vec::new();

        if win_exclude_from_capture::is_supported() {
            vec_impls.push((
                PRIVACY_MODE_IMPL_WIN_EXCLUDE_FROM_CAPTURE,
                "privacy_mode_impl_mag_tip",
            ));
        } else {
            if display_service::is_privacy_mode_mag_supported() {
                vec_impls.push((PRIVACY_MODE_IMPL_WIN_MAG, "privacy_mode_impl_mag_tip"));
            }
        }

        if is_installed() && crate::platform::windows::is_self_service_running() {
            vec_impls.push((
                PRIVACY_MODE_IMPL_WIN_VIRTUAL_DISPLAY,
                "privacy_mode_impl_virtual_display_tip",
            ));
        }

        vec_impls
    }
    #[cfg(target_os = "macos")]
    {
        // No translation is intended for privacy_mode_impl_macos_tip as it is a 
        // placeholder for macOS specific privacy mode implementation which currently
        // doesn't provide multiple modes like Windows does.
        vec![(macos::PRIVACY_MODE_IMPL, "privacy_mode_impl_macos_tip")]
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Vec::new()
    }
}

#[inline]
pub fn get_cur_impl_key() -> Option<String> {
    PRIVACY_MODE
        .lock()
        .unwrap()
        .as_ref()
        .map(|pm| pm.get_impl_key().to_owned())
}

#[inline]
pub fn is_current_privacy_mode_impl(impl_key: &str) -> bool {
    PRIVACY_MODE
        .lock()
        .unwrap()
        .as_ref()
        .map(|pm| pm.get_impl_key() == impl_key)
        .unwrap_or(false)
}

#[inline]
#[cfg(not(windows))]
pub fn check_privacy_mode_err(
    _privacy_mode_id: i32,
    _display_idx: usize,
    _timeout_millis: u64,
) -> String {
    "".to_owned()
}

#[inline]
#[cfg(windows)]
pub fn check_privacy_mode_err(
    privacy_mode_id: i32,
    display_idx: usize,
    timeout_millis: u64,
) -> String {
    // win magnifier implementation requires a test of creating a capturer.
    if is_current_privacy_mode_impl(PRIVACY_MODE_IMPL_WIN_MAG) {
        crate::video_service::test_create_capturer(privacy_mode_id, display_idx, timeout_millis)
    } else {
        "".to_owned()
    }
}

#[inline]
pub fn is_privacy_mode_supported() -> bool {
    !DEFAULT_PRIVACY_MODE_IMPL.is_empty()
}

#[inline]
pub fn get_privacy_mode_conn_id() -> Option<i32> {
    PRIVACY_MODE
        .lock()
        .unwrap()
        .as_ref()
        .map(|pm| pm.pre_conn_id())
}

#[inline]
pub fn is_in_privacy_mode() -> bool {
    PRIVACY_MODE
        .lock()
        .unwrap()
        .as_ref()
        .map(|pm| pm.pre_conn_id() != INVALID_PRIVACY_MODE_CONN_ID)
        .unwrap_or(false)
}
