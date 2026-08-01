//! Drive / volume enumeration for "This PC" and the fast dial.

use crate::fs::DriveInfo;

/// Win32 `DRIVE_*` return values of `GetDriveTypeW`.
#[cfg(windows)]
fn drive_kind(code: u32) -> &'static str {
    match code {
        2 => "removable",
        3 => "fixed",
        4 => "network",
        5 => "cdrom",
        6 => "ramdisk",
        _ => "unknown",
    }
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn enumerate() -> Vec<DriveInfo> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };

    let mask = unsafe { GetLogicalDrives() };
    let mut drives = Vec::new();

    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{}:\\", letter);
        let root_w = wide(&root);
        let root_pcwstr = PCWSTR(root_w.as_ptr());

        let kind = drive_kind(unsafe { GetDriveTypeW(root_pcwstr) });

        // Empty card readers / optical drives fail these calls; report the
        // drive anyway with zeroed figures rather than hiding it.
        let mut label_buf = [0u16; 261];
        let label = unsafe {
            GetVolumeInformationW(
                root_pcwstr,
                Some(&mut label_buf),
                None,
                None,
                None,
                None,
            )
        }
        .ok()
        .map(|_| {
            let len = label_buf.iter().position(|&c| c == 0).unwrap_or(0);
            String::from_utf16_lossy(&label_buf[..len])
        })
        .unwrap_or_default();

        let (mut total, mut free) = (0u64, 0u64);
        let _ = unsafe {
            GetDiskFreeSpaceExW(root_pcwstr, None, Some(&mut total), Some(&mut free))
        };

        drives.push(DriveInfo {
            path: root,
            label,
            kind: kind.to_string(),
            total_bytes: total,
            free_bytes: free,
        });
    }
    drives
}

#[cfg(not(windows))]
fn enumerate() -> Vec<DriveInfo> {
    vec![DriveInfo {
        path: "/".to_string(),
        label: "Root".to_string(),
        kind: "fixed".to_string(),
        total_bytes: 0,
        free_bytes: 0,
    }]
}

/// Enumerate volumes. Runs blocking: probing a spun-down or
/// disconnected network drive can take seconds.
pub async fn list_drives() -> Vec<DriveInfo> {
    tokio::task::spawn_blocking(enumerate).await.unwrap_or_default()
}
