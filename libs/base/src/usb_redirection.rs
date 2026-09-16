use crate::protos::message::*;
use lazy_static::lazy_static;
use serde_derive::Serialize;
#[cfg(target_os = "windows")]
use sha2::{Digest, Sha256};
#[cfg(target_os = "windows")]
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct UsbDevice {
    pub bus_id: String,
    pub vendor_id: u32,
    pub product_id: u32,
    pub name: String,
    pub device_class: String,
    pub is_redirected: bool,
}

/// A physical USB device available on the controller. `bus_id` is the Windows
/// device instance ID, which is stable enough to identify an attach request.
#[derive(Debug, Clone, Serialize)]
pub struct LocalUsbDevice {
    pub bus_id: String,
    pub vendor_id: u32,
    pub product_id: u32,
    pub name: String,
    pub device_class: String,
    pub is_selected: bool,
}

lazy_static! {
    static ref REDIRECTED_DEVICES: Arc<Mutex<Vec<UsbDevice>>> = Arc::new(Mutex::new(Vec::new()));
}

/// Legacy protobuf helper retained for compatibility with the generated API.
/// New UI callers use `list_local_usb_devices_real` below.
pub fn list_local_usb_devices() -> Vec<UsbDeviceInfo> {
    list_local_usb_devices_real()
        .into_iter()
        .map(|device| UsbDeviceInfo {
            bus_id: device.bus_id,
            vendor_id: device.vendor_id,
            product_id: device.product_id,
            name: device.name,
            is_redirected: device.is_selected,
            device_class: device.device_class,
            special_fields: Default::default(),
        })
        .collect()
}

/// Enumerates present physical USB devices. The actual USB/IP data plane is
/// deliberately separate: merely listing a device must never claim it.
pub fn list_local_usb_devices_real() -> Vec<LocalUsbDevice> {
    #[cfg(target_os = "windows")]
    return windows::enumerate();

    #[cfg(target_os = "linux")]
    return linux::enumerate();

    #[cfg(target_os = "macos")]
    return macos::enumerate();

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    Vec::new()
}

/// Toggles USB redirection status for a specific bus_id
pub fn toggle_usb_redirection(bus_id: &str, redirect: bool) -> bool {
    let mut active = REDIRECTED_DEVICES.lock().unwrap();
    if redirect {
        if !active.iter().any(|d| d.bus_id == bus_id) {
            active.push(UsbDevice {
                bus_id: bus_id.to_string(),
                vendor_id: 0,
                product_id: 0,
                name: "USB Device".to_string(),
                device_class: "Generic".to_string(),
                is_redirected: true,
            });
        }
    } else {
        active.retain(|d| d.bus_id != bus_id);
    }
    true
}

pub fn is_device_redirected(bus_id: &str) -> bool {
    let active = REDIRECTED_DEVICES.lock().unwrap();
    active.iter().any(|d| d.bus_id == bus_id)
}

/// USB/IP needs a signed virtual-host driver on the controlled Windows host.
/// This build prepares device selection only; it never reports a selected
/// device as attached until that backend is installed and connected.
pub fn backend_status() -> String {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let usbipd = Command::new("sc.exe")
            .args(["query", "usbipd"])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        let ude = Command::new("sc.exe")
            .args(["query", "usbip2_ude"])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        if usbipd && ude {
            return "USB/IP backend is installed".to_owned();
        }
    }
    "USB/IP virtual-host driver is not installed".to_owned()
}

#[cfg(target_os = "windows")]
pub fn install_backend() -> String {
    use std::process::Command;

    let root = match std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    {
        Some(path) => path.join("usbip"),
        None => return "Unable to locate the bundled USB/IP payload".to_owned(),
    };
    let packages = [
        ("usbipd-win_5.3.0_x64.msi", "1c984914aec944de19b64eff232421439629699f8138e3ddc29301175bc6d938", true),
        ("USBip-0.9.8.0-x64.exe", "81f426741f7ee2ed991febe24a22daca8400b6ae2f171054e3fb404897e15d39", false),
    ];
    for (name, expected, msi) in packages {
        let path = root.join(name);
        let actual = match sha256_file(&path) {
            Ok(hash) => hash,
            Err(error) => return format!("USB/IP package unavailable: {error}"),
        };
        if actual != expected {
            return format!("USB/IP package hash mismatch: {name}");
        }
        let status = if msi {
            Command::new("msiexec.exe")
                .args(["/i", path.to_string_lossy().as_ref(), "/qn", "/norestart"])
                .status()
        } else {
            Command::new(&path)
                .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"])
                .status()
        };
        match status {
            Ok(status) if status.success() => {}
            Ok(status) => return format!("USB/IP installer failed ({name}: {})", status),
            Err(error) => return format!("Unable to start USB/IP installer {name}: {error}"),
        }
    }
    "USB/IP backend installed. A reboot may be required before first attach.".to_owned()
}

#[cfg(target_os = "windows")]
fn sha256_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::{mem, ptr};
    use winapi::{
        shared::minwindef::{BYTE, DWORD},
        um::{
            handleapi::INVALID_HANDLE_VALUE,
            setupapi::{
                SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
                SetupDiGetDeviceInstanceIdW, SetupDiGetDeviceRegistryPropertyW, DIGCF_ALLCLASSES,
                DIGCF_PRESENT, HDEVINFO, SPDRP_CLASS, SPDRP_DEVICEDESC, SPDRP_FRIENDLYNAME,
                SP_DEVINFO_DATA,
            },
        },
    };

    fn wide_to_string(buffer: &[u16]) -> String {
        let end = buffer
            .iter()
            .position(|&ch| ch == 0)
            .unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    }

    unsafe fn property(set: HDEVINFO, data: &mut SP_DEVINFO_DATA, key: DWORD) -> Option<String> {
        let mut value = [0u16; 512];
        if SetupDiGetDeviceRegistryPropertyW(
            set,
            data,
            key,
            ptr::null_mut(),
            value.as_mut_ptr() as *mut BYTE,
            (value.len() * mem::size_of::<u16>()) as DWORD,
            ptr::null_mut(),
        ) == 0
        {
            return None;
        }
        let value = wide_to_string(&value);
        (!value.is_empty()).then_some(value)
    }

    fn hex_component(instance: &str, key: &str) -> u32 {
        instance
            .split('\\')
            .next()
            .and_then(|segment| segment.split('&').find(|part| part.starts_with(key)))
            .and_then(|part| u32::from_str_radix(&part[key.len()..], 16).ok())
            .unwrap_or_default()
    }

    pub fn enumerate() -> Vec<LocalUsbDevice> {
        unsafe {
            let set = SetupDiGetClassDevsW(
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                DIGCF_PRESENT | DIGCF_ALLCLASSES,
            );
            if set == INVALID_HANDLE_VALUE {
                return Vec::new();
            }

            let mut devices = Vec::new();
            let mut index = 0;
            loop {
                let mut data: SP_DEVINFO_DATA = mem::zeroed();
                data.cbSize = mem::size_of::<SP_DEVINFO_DATA>() as DWORD;
                if SetupDiEnumDeviceInfo(set, index, &mut data) == 0 {
                    break;
                }
                index += 1;

                let mut instance = [0u16; 512];
                if SetupDiGetDeviceInstanceIdW(
                    set,
                    &mut data,
                    instance.as_mut_ptr(),
                    instance.len() as DWORD,
                    ptr::null_mut(),
                ) == 0
                {
                    continue;
                }
                let bus_id = wide_to_string(&instance);
                if !bus_id.to_ascii_uppercase().starts_with("USB\\VID_") {
                    continue;
                }
                let name = property(set, &mut data, SPDRP_FRIENDLYNAME)
                    .or_else(|| property(set, &mut data, SPDRP_DEVICEDESC))
                    .unwrap_or_else(|| bus_id.clone());
                let device_class =
                    property(set, &mut data, SPDRP_CLASS).unwrap_or_else(|| "USB".to_owned());
                devices.push(LocalUsbDevice {
                    vendor_id: hex_component(&bus_id, "VID_"),
                    product_id: hex_component(&bus_id, "PID_"),
                    is_selected: is_device_redirected(&bus_id),
                    bus_id,
                    name,
                    device_class,
                });
            }
            SetupDiDestroyDeviceInfoList(set);
            devices.sort_by(|a, b| a.name.cmp(&b.name));
            devices
        }
    }
}

// ---------------------------------------------------------------------------
// Linux: enumerate USB devices from sysfs (/sys/bus/usb/devices)
// No external dependencies; works on any kernel ≥ 2.6.
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{fs, path::Path};

    fn read_attr(base: &Path, attr: &str) -> String {
        fs::read_to_string(base.join(attr))
            .unwrap_or_default()
            .trim()
            .to_owned()
    }

    fn parse_hex(s: &str) -> u32 {
        u32::from_str_radix(s.trim(), 16).unwrap_or(0)
    }

    pub fn enumerate() -> Vec<LocalUsbDevice> {
        let sysfs = Path::new("/sys/bus/usb/devices");
        let entries = match fs::read_dir(sysfs) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        let mut devices = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            // Only top-level USB devices have an idVendor attribute (busX-portY notation).
            let vid_str = read_attr(&path, "idVendor");
            if vid_str.is_empty() {
                continue;
            }
            let pid_str = read_attr(&path, "idProduct");
            let vendor_id = parse_hex(&vid_str);
            let product_id = parse_hex(&pid_str);
            // Skip hubs (bDeviceClass 09)
            let dev_class_hex = read_attr(&path, "bDeviceClass");
            if dev_class_hex == "09" {
                continue;
            }
            let bus_id = entry.file_name().to_string_lossy().to_string();
            // Prefer manufacturer + product string; fall back to VID:PID.
            let manufacturer = read_attr(&path, "manufacturer");
            let product = read_attr(&path, "product");
            let name = match (manufacturer.is_empty(), product.is_empty()) {
                (false, false) => format!("{} {}", manufacturer, product),
                (true, false) => product.clone(),
                (false, true) => manufacturer.clone(),
                (true, true) => format!(
                    "{:04X}:{:04X}",
                    vendor_id, product_id
                ),
            };
            // Map bDeviceClass hex → human label
            let device_class = match dev_class_hex.as_str() {
                "00" => "Per-interface".to_owned(),
                "02" => "Communications".to_owned(),
                "03" => "HID".to_owned(),
                "06" => "Imaging".to_owned(),
                "07" => "Printer".to_owned(),
                "08" => "Mass Storage".to_owned(),
                "0a" => "CDC-Data".to_owned(),
                "0b" => "Smart Card".to_owned(),
                "0e" => "Video".to_owned(),
                "e0" => "Wireless".to_owned(),
                "ef" => "Misc".to_owned(),
                "ff" => "Vendor-specific".to_owned(),
                _ => format!("USB {:02X}", parse_hex(&dev_class_hex)),
            };
            devices.push(LocalUsbDevice {
                bus_id: bus_id.clone(),
                vendor_id,
                product_id,
                name,
                device_class,
                is_selected: is_device_redirected(&bus_id),
            });
        }
        devices.sort_by(|a, b| a.name.cmp(&b.name));
        devices
    }
}

// ---------------------------------------------------------------------------
// macOS: enumerate USB devices via `ioreg -p IOUSBHostDevice -l -a` (plist XML)
// No external crates required; plist is simple enough for hand-rolled parsing.
// ---------------------------------------------------------------------------
#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::process::Command;

    /// Minimal plist dict value extractor for integer and string fields.
    fn plist_integer(xml: &str, key: &str) -> u32 {
        let needle = format!("<key>{}</key>", key);
        let pos = match xml.find(&needle) {
            Some(p) => p + needle.len(),
            None => return 0,
        };
        let rest = &xml[pos..];
        if let Some(start) = rest.find("<integer>") {
            let start = start + "<integer>".len();
            if let Some(end) = rest[start..].find("</integer>") {
                return rest[start..start + end].trim().parse().unwrap_or(0);
            }
        }
        0
    }

    fn plist_string(xml: &str, key: &str) -> String {
        let needle = format!("<key>{}</key>", key);
        let pos = match xml.find(&needle) {
            Some(p) => p + needle.len(),
            None => return String::new(),
        };
        let rest = &xml[pos..];
        if let Some(start) = rest.find("<string>") {
            let start = start + "<string>".len();
            if let Some(end) = rest[start..].find("</string>") {
                return rest[start..start + end].trim().to_owned();
            }
        }
        String::new()
    }

    pub fn enumerate() -> Vec<LocalUsbDevice> {
        let output = Command::new("ioreg")
            .args(["-p", "IOUSBHostDevice", "-l", "-a", "-r"])
            .output();
        let xml = match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => return Vec::new(),
        };

        let mut devices = Vec::new();
        // Each device block is delimited by <dict>…</dict>
        let mut pos = 0;
        while let Some(start) = xml[pos..].find("<dict>") {
            let start = pos + start;
            let end = match xml[start..].find("</dict>") {
                Some(e) => start + e + "</dict>".len(),
                None => break,
            };
            let block = &xml[start..end];
            pos = end;

            let vendor_id = plist_integer(block, "idVendor");
            if vendor_id == 0 {
                continue; // not a real USB device block
            }
            let product_id = plist_integer(block, "idProduct");
            // Skip hubs (bDeviceClass 9)
            let dev_class = plist_integer(block, "bDeviceClass");
            if dev_class == 9 {
                continue;
            }

            // locationID is the stable macOS identifier (bus<<24 | port)
            let location_id = plist_integer(block, "locationID");
            let bus_id = format!("0x{:08X}", location_id);

            let product = plist_string(block, "USB Product Name");
            let vendor = plist_string(block, "USB Vendor Name");
            let name = match (vendor.is_empty(), product.is_empty()) {
                (false, false) => format!("{} {}", vendor, product),
                (true, false) => product.clone(),
                (false, true) => vendor.clone(),
                (true, true) => format!("{:04X}:{:04X}", vendor_id, product_id),
            };

            let device_class = match dev_class {
                0 => "Per-interface".to_owned(),
                2 => "Communications".to_owned(),
                3 => "HID".to_owned(),
                6 => "Imaging".to_owned(),
                7 => "Printer".to_owned(),
                8 => "Mass Storage".to_owned(),
                0xe => "Video".to_owned(),
                0xe0 => "Wireless".to_owned(),
                0xff => "Vendor-specific".to_owned(),
                n => format!("USB {:#04x}", n),
            };

            devices.push(LocalUsbDevice {
                bus_id: bus_id.clone(),
                vendor_id,
                product_id,
                name,
                device_class,
                is_selected: is_device_redirected(&bus_id),
            });
        }
        devices.sort_by(|a, b| a.name.cmp(&b.name));
        devices
    }
}
