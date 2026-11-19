//! Tells the app when audio devices appear or disappear, so the local outputs
//! can be refreshed without restarting.

use std::ffi::c_void;

type AudioObjectID = u32;
type OSStatus = i32;

const SYSTEM_OBJECT: AudioObjectID = 1;

const fn four_cc(code: &[u8; 4]) -> u32 {
    ((code[0] as u32) << 24) | ((code[1] as u32) << 16) | ((code[2] as u32) << 8) | (code[3] as u32)
}

#[repr(C)]
struct PropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

type ListenerProc = extern "C" fn(AudioObjectID, u32, *const PropertyAddress, *mut c_void) -> OSStatus;

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectAddPropertyListener(
        object: AudioObjectID,
        address: *const PropertyAddress,
        listener: ListenerProc,
        client_data: *mut c_void,
    ) -> OSStatus;
}

/// Calls `on_change` whenever the set of audio devices changes. The callback runs on a
/// Core Audio thread, so it should only hand work off. It stays registered until the app exits.
pub fn watch(on_change: impl Fn() + Send + Sync + 'static) -> Result<(), String> {
    let callback: Box<dyn Fn() + Send + Sync> = Box::new(on_change);
    let client = Box::into_raw(Box::new(callback)) as *mut c_void;
    let address = PropertyAddress {
        selector: four_cc(b"dev#"),
        scope: four_cc(b"glob"),
        element: 0,
    };
    let status = unsafe { AudioObjectAddPropertyListener(SYSTEM_OBJECT, &address, devices_changed, client) };
    if status == 0 {
        Ok(())
    } else {
        Err(format!("could not watch for audio devices (OSStatus {status})"))
    }
}

extern "C" fn devices_changed(
    _object: AudioObjectID,
    _count: u32,
    _addresses: *const PropertyAddress,
    client: *mut c_void,
) -> OSStatus {
    let callback = unsafe { &*(client as *const Box<dyn Fn() + Send + Sync>) };
    callback();
    0
}

#[cfg(test)]
mod hardware_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    static CHANGES: AtomicUsize = AtomicUsize::new(0);

    /// Needs Core Audio. Plug or unplug a device while it waits.
    /// Run with `cargo test -- --ignored --nocapture reports_device_changes`.
    #[test]
    #[ignore]
    fn reports_device_changes() {
        watch(|| {
            CHANGES.fetch_add(1, Ordering::SeqCst);
        })
        .expect("watch");
        std::thread::sleep(Duration::from_secs(20));
        println!("device list changes seen: {}", CHANGES.load(Ordering::SeqCst));
    }
}
