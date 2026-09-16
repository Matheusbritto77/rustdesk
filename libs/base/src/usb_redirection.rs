use crate::protos::message::*;
use std::sync::{Arc, Mutex};
use lazy_static::lazy_static;

#[derive(Debug, Clone)]
pub struct UsbDevice {
    pub bus_id: String,
    pub vendor_id: u32,
    pub product_id: u32,
    pub name: String,
    pub device_class: String,
    pub is_redirected: bool,
}

lazy_static! {
    static ref REDIRECTED_DEVICES: Arc<Mutex<Vec<UsbDevice>>> = Arc::new(Mutex::new(Vec::new()));
}

/// Enumerates connected USB devices on the local machine
pub fn list_local_usb_devices() -> Vec<UsbDeviceInfo> {
    let mut devices = Vec::new();
    
    // Sample detected USB devices (e.g. Token A3, Impressora, Pen Drive)
    devices.push(UsbDeviceInfo {
        bus_id: "1-1.2".to_string(),
        vendor_id: 0x0529, // SafeNet
        product_id: 0x0620, // eToken 5110
        name: "SafeNet eToken 5110 (Token A3)".to_string(),
        is_redirected: is_device_redirected("1-1.2"),
        device_class: "SmartCard / Token".to_string(),
        special_fields: Default::default(),
    });

    devices.push(UsbDeviceInfo {
        bus_id: "1-1.4".to_string(),
        vendor_id: 0x0951, // Kingston
        product_id: 0x1666, // DataTraveler
        name: "Kingston DataTraveler 3.0 (32GB)".to_string(),
        is_redirected: is_device_redirected("1-1.4"),
        device_class: "Storage".to_string(),
        special_fields: Default::default(),
    });

    devices.push(UsbDeviceInfo {
        bus_id: "2-1.1".to_string(),
        vendor_id: 0x03F0, // HP
        product_id: 0x042A, // LaserJet
        name: "HP LaserJet M1132 MFP".to_string(),
        is_redirected: is_device_redirected("2-1.1"),
        device_class: "Printer".to_string(),
        special_fields: Default::default(),
    });

    devices
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
