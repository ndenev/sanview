/// SES (SCSI Enclosure Services) collector for disk slot mapping
///
/// Uses FreeBSD SES ioctls to map disks to their physical enclosure slots
/// Reference: ses(4), scsi_enc.h

use anyhow::{Context, Result};
use log::{debug, warn};
use std::collections::HashMap;
use std::fs::{self, File};
use std::os::unix::io::AsRawFd;

// SES ioctl constants from /usr/include/cam/scsi/scsi_enc.h
const ENCIOC: u8 = b's' - 0o40;  // ('s' - 040)

// Define ioctl numbers using nix's _IO macro equivalent
#[allow(non_snake_case)]
const fn _IO(group: u8, num: u8) -> libc::c_ulong {
    0x20000000 | ((group as libc::c_ulong) << 8) | (num as libc::c_ulong)
}

const ENCIOC_GETNELM: libc::c_ulong = _IO(ENCIOC, 1);
const ENCIOC_GETELMMAP: libc::c_ulong = _IO(ENCIOC, 2);
const ENCIOC_GETELMDEVNAMES: libc::c_ulong = _IO(ENCIOC, 10);

// Element types from scsi_enc.h
const ELMTYP_DEVICE: u32 = 0x01;        // Device Slot
const ELMTYP_ARRAY_DEV: u32 = 0x17;     // Array Device Slot

// FFI structures matching /usr/include/cam/scsi/scsi_enc.h
#[repr(C)]
#[derive(Debug, Clone)]
struct EnciocElement {
    elm_idx: libc::c_uint,
    elm_subenc_id: libc::c_uint,
    elm_type: libc::c_uint,  // elm_type_t
}

#[repr(C)]
struct EnciocElmDevnames {
    elm_idx: libc::c_uint,
    elm_names_size: libc::size_t,
    elm_names_len: libc::size_t,
    elm_devnames: *mut libc::c_char,
}

#[derive(Debug, Clone)]
pub struct SesSlotInfo {
    pub slot: usize,           // Physical slot number
    pub device_name: String,   // Device name (e.g., "da0")
    pub enclosure: String,     // Enclosure identifier (e.g., "ses0")
}

pub struct SesCollector;

impl SesCollector {
    pub fn new() -> Self {
        Self
    }

    /// Collect slot mappings from all SES devices
    /// Returns a map of device_name -> SesSlotInfo
    ///
    /// Note: For dual-controller arrays, both controllers see the same physical
    /// enclosure but report different device names (different paths). We scan all
    /// controllers to get complete coverage, but only keep one slot assignment per device.
    pub fn collect(&self) -> Result<HashMap<String, SesSlotInfo>> {
        let mut slot_map = HashMap::new();

        // Find all /dev/ses* devices
        let ses_devices = self.find_ses_devices()?;

        for ses_dev in &ses_devices {
            debug!("Scanning enclosure {}", ses_dev);
            match self.scan_enclosure(ses_dev) {
                Ok(mappings) => {
                    for (device_name, slot_info) in mappings {
                        // Only insert if we haven't seen this device yet
                        // This gives priority to the first SES device (typically ses0)
                        slot_map.entry(device_name).or_insert(slot_info);
                    }
                }
                Err(e) => {
                    warn!("Failed to scan {}: {}", ses_dev, e);
                }
            }
        }

        debug!("Collected slot mappings for {} devices from {} enclosures",
               slot_map.len(), ses_devices.len());
        Ok(slot_map)
    }

    fn find_ses_devices(&self) -> Result<Vec<String>> {
        let mut devices = Vec::new();

        for entry in fs::read_dir("/dev")? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with("ses") && !name_str.contains('.') {
                devices.push(format!("/dev/{}", name_str));
            }
        }

        debug!("Found {} SES devices", devices.len());
        Ok(devices)
    }

    fn scan_enclosure(&self, dev_path: &str) -> Result<HashMap<String, SesSlotInfo>> {
        let mut mappings = HashMap::new();

        let file = File::open(dev_path)
            .with_context(|| format!("Failed to open {}", dev_path))?;
        let fd = file.as_raw_fd();

        // Get number of elements
        let mut nelm: libc::c_uint = 0;
        let ret = unsafe { libc::ioctl(fd, ENCIOC_GETNELM, &mut nelm) };
        if ret < 0 {
            return Err(anyhow::anyhow!("ENCIOC_GETNELM failed"));
        }

        debug!("{}: {} elements", dev_path, nelm);

        // Get element map
        let mut elements: Vec<EnciocElement> = vec![
            EnciocElement {
                elm_idx: 0,
                elm_subenc_id: 0,
                elm_type: 0,
            };
            nelm as usize
        ];

        let ret = unsafe { libc::ioctl(fd, ENCIOC_GETELMMAP, elements.as_mut_ptr()) };
        if ret < 0 {
            return Err(anyhow::anyhow!("ENCIOC_GETELMMAP failed"));
        }

        // Extract enclosure name for logging
        let enc_name = dev_path.strip_prefix("/dev/").unwrap_or(dev_path);

        // Scan device elements and use element index as slot number
        for element in elements.iter() {
            // Only interested in device slots
            if element.elm_type != ELMTYP_DEVICE && element.elm_type != ELMTYP_ARRAY_DEV {
                continue;
            }

            // Use element index as slot number (matches physical slot labeling)
            let slot = element.elm_idx as usize;

            // Get device names for this element
            if let Ok(dev_names) = self.get_element_devnames(fd, element.elm_idx) {
                for dev_name in dev_names {
                    // Only map da* and nda* devices
                    if dev_name.starts_with("da") || dev_name.starts_with("nda") {
                        debug!("{}: Element {} -> slot {}  ({})",
                               enc_name, element.elm_idx, slot, dev_name);

                        mappings.insert(
                            dev_name.clone(),
                            SesSlotInfo {
                                slot,
                                device_name: dev_name,
                                enclosure: enc_name.to_string(),
                            },
                        );
                    }
                }
            }
        }

        Ok(mappings)
    }

    fn get_element_devnames(&self, fd: libc::c_int, elm_idx: libc::c_uint)
        -> Result<Vec<String>> {

        const BUF_SIZE: usize = 512;
        let mut buffer = vec![0u8; BUF_SIZE];

        let mut devnames = EnciocElmDevnames {
            elm_idx,
            elm_names_size: BUF_SIZE,
            elm_names_len: 0,
            elm_devnames: buffer.as_mut_ptr() as *mut libc::c_char,
        };

        let ret = unsafe { libc::ioctl(fd, ENCIOC_GETELMDEVNAMES, &mut devnames) };
        if ret < 0 {
            return Ok(Vec::new());  // Element has no devices
        }

        if devnames.elm_names_len == 0 {
            return Ok(Vec::new());
        }

        // Parse comma-separated device names
        let names_cstr = unsafe {
            std::ffi::CStr::from_ptr(buffer.as_ptr() as *const libc::c_char)
        };

        let names_str = names_cstr.to_string_lossy();
        let devices: Vec<String> = names_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(devices)
    }
}

impl Default for SesCollector {
    fn default() -> Self {
        Self::new()
    }
}
