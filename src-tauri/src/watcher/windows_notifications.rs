//! Windows PnP notifications retain even a removal/rearrival between presence polls.
//! The callback only enqueues observations; the watcher owns session cleanup.
use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, Sender};

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Register_Notification, CM_Unregister_Notification, CM_NOTIFY_ACTION,
    CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL, CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL,
    CM_NOTIFY_EVENT_DATA, CM_NOTIFY_FILTER, CM_NOTIFY_FILTER_0_0,
    CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE, CR_SUCCESS, HCMNOTIFICATION,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::HidD_GetHidGuid;

pub(super) struct Notifications {
    handle: HCMNOTIFICATION,
    context: Option<Box<Sender<bool>>>,
    rx: Receiver<bool>,
}

impl Notifications {
    pub(super) fn register() -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        // The allocation stays at this address while Windows can call back.
        let context = Box::new(tx);
        let mut guid = windows_sys::core::GUID::default();
        unsafe { HidD_GetHidGuid(&mut guid) };
        let mut filter = CM_NOTIFY_FILTER {
            cbSize: std::mem::size_of::<CM_NOTIFY_FILTER>() as u32,
            FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
            ..Default::default()
        };
        filter.u.DeviceInterface = CM_NOTIFY_FILTER_0_0 { ClassGuid: guid };
        let mut handle = std::ptr::null_mut();
        let result = unsafe {
            CM_Register_Notification(
                &filter,
                (&*context as *const Sender<bool>).cast(),
                Some(callback),
                &mut handle,
            )
        };
        if result != CR_SUCCESS {
            return Err(format!("CM_Register_Notification: CONFIGRET {result}"));
        }
        Ok(Self {
            handle,
            context: Some(context),
            rx,
        })
    }

    pub(super) fn observations(&self) -> impl Iterator<Item = bool> + '_ {
        self.rx.try_iter()
    }
}

impl Drop for Notifications {
    fn drop(&mut self) {
        // Called on the watcher thread, never inside the callback. Unregister
        // waits for pending callbacks before their context can be freed.
        let result = unsafe { CM_Unregister_Notification(self.handle) };
        if result != CR_SUCCESS {
            log::warn!("CM_Unregister_Notification: CONFIGRET {result}");
            // If unregister fails, callbacks may remain active. Retain their tiny
            // context rather than free memory Windows may still access.
            if let Some(context) = self.context.take() {
                let _ = Box::leak(context);
            }
        }
    }
}

unsafe extern "system" fn callback(
    _handle: HCMNOTIFICATION,
    context: *const c_void,
    action: CM_NOTIFY_ACTION,
    event: *const CM_NOTIFY_EVENT_DATA,
    size: u32,
) -> u32 {
    let present = match action {
        CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL => true,
        CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL => false,
        _ => return 0,
    };
    if context.is_null() {
        return 0;
    }
    if let Some(path) = unsafe { interface_path(event, size) } {
        if crate::hid::path_matches_tmp(&path) {
            let tx = unsafe { &*context.cast::<Sender<bool>>() };
            let _ = tx.send(present);
        }
    }
    0 // ERROR_SUCCESS; no veto, device I/O, or session locks in the callback.
}

/// The interface path is a variable-length, NUL-terminated UTF-16 tail. Bound
/// every read by the byte count supplied by Windows, including malformed input.
unsafe fn interface_path(event: *const CM_NOTIFY_EVENT_DATA, size: u32) -> Option<String> {
    let offset = std::mem::offset_of!(CM_NOTIFY_EVENT_DATA, u.DeviceInterface.SymbolicLink);
    let size = size as usize;
    if event.is_null() || size < offset + std::mem::size_of::<u16>() {
        return None;
    }
    if unsafe { (*event).FilterType } != CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE {
        return None;
    }
    let path = unsafe { event.cast::<u8>().add(offset).cast::<u16>() };
    let wide = unsafe { std::slice::from_raw_parts(path, (size - offset) / 2) };
    let end = wide.iter().position(|&c| c == 0)?;
    Some(String::from_utf16_lossy(&wide[..end]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(path: &str) -> (Vec<u64>, u32) {
        let offset = std::mem::offset_of!(CM_NOTIFY_EVENT_DATA, u.DeviceInterface.SymbolicLink);
        let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let size = offset + wide.len() * 2;
        // u64 storage keeps the native structure aligned; the declared event
        // length still describes only its actual variable-length interface data.
        let mut storage = vec![
            0u64;
            size.max(std::mem::size_of::<CM_NOTIFY_EVENT_DATA>())
                .div_ceil(8)
        ];
        unsafe {
            let data = storage.as_mut_ptr().cast::<CM_NOTIFY_EVENT_DATA>();
            (*data).FilterType = CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE;
            std::ptr::copy_nonoverlapping(
                wide.as_ptr(),
                data.cast::<u8>().add(offset).cast(),
                wide.len(),
            );
        }
        (storage, size as u32)
    }

    #[test]
    fn callback_retains_fast_replug_and_filters_unrelated_interfaces() {
        let (tx, rx) = mpsc::channel();
        let context = (&tx as *const Sender<bool>).cast();
        for (path, action) in [
            (
                r"\\?\HID#VID_1ED8&PID_0044&MI_02#x",
                CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL,
            ),
            (
                r"\\?\HID#VID_1ED8&PID_0044&MI_02#x",
                CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL,
            ),
            (
                r"\\?\HID#VID_1ED8&PID_0047#x",
                CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL,
            ),
        ] {
            let (storage, size) = event(path);
            assert_eq!(
                unsafe {
                    callback(
                        std::ptr::null_mut(),
                        context,
                        action,
                        storage.as_ptr().cast(),
                        size,
                    )
                },
                0
            );
        }
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![false, true]);
    }

    #[test]
    fn truncated_interface_data_is_ignored() {
        let (storage, size) = event(r"\\?\HID#VID_1ED8&PID_0044#x");
        let data = storage.as_ptr().cast();
        assert!(unsafe { interface_path(data, size) }.is_some());
        assert!(unsafe { interface_path(data, size - 2) }.is_none());
        assert!(unsafe { interface_path(data, 0) }.is_none());
        assert!(unsafe { interface_path(std::ptr::null(), size) }.is_none());
    }
}
