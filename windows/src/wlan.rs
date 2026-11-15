//! Read/scan-only Native Wi-Fi adapter. No connection/profile/radio mutation APIs.
use crate::{
    error::{AppError, Result},
    heatmap::Cancellation,
    model::*,
};

#[derive(Clone, Debug)]
pub struct Adapter {
    pub id: String,
    pub name: String,
    pub connected: bool,
}
#[derive(Clone, Debug)]
pub struct ScanSnapshot {
    pub observations: Vec<Observation>,
    pub discarded_stale: usize,
}
pub trait WirelessAdapter: Send {
    fn adapters(&self) -> Result<Vec<Adapter>>;
    fn scan(
        &self,
        adapter: &str,
        target: Option<Bssid>,
        cancel: &Cancellation,
    ) -> Result<ScanSnapshot>;
}
pub struct WindowsWlan;
pub fn api_error(code: u32) -> AppError {
    let key = match code {
        5 => "wlan_permission",
        1062 | 50 => "wlan_service",
        1167 | 1168 => "wlan_unavailable",
        _ => "wlan_error",
    };
    AppError::detail(key, format!("Windows {code}"))
}
const WINDOWS_EPOCH_TICKS: u64 = 116_444_736_000_000_000;
pub fn host_timestamp_ms(ticks: u64) -> Option<u64> {
    ticks.checked_sub(WINDOWS_EPOCH_TICKS).map(|t| t / 10_000)
}
pub fn fresh(observed_ms: u64, requested_ms: u64, collected_ms: u64) -> bool {
    observed_ms >= requested_ms
        && observed_ms <= collected_ms.saturating_add(1000)
        && collected_ms.saturating_sub(observed_ms) <= 15_000
}
pub fn stable_connection(before: Option<Bssid>, after: Option<Bssid>) -> Result<()> {
    if before != after {
        Err(AppError::new("wlan_roamed"))
    } else {
        Ok(())
    }
}
#[cfg(windows)]
mod native {
    use super::*;
    use std::{
        ffi::c_void,
        mem::{offset_of, size_of},
        ptr::{null, null_mut},
        thread,
        time::{Duration, Instant},
    };
    use windows_sys::{
        Win32::{Foundation::HANDLE, NetworkManagement::WiFi::*},
        core::GUID,
    };
    struct Session(HANDLE);
    impl Drop for Session {
        fn drop(&mut self) {
            unsafe {
                WlanCloseHandle(self.0, null());
            }
        }
    }
    struct Allocation(*mut c_void);
    impl Drop for Allocation {
        fn drop(&mut self) {
            unsafe {
                WlanFreeMemory(self.0);
            }
        }
    }
    impl Session {
        fn open() -> Result<Self> {
            let mut handle = null_mut();
            let mut version = 0;
            // SAFETY: output pointers are valid, reserved is NULL, RAII closes the handle.
            let code = unsafe { WlanOpenHandle(2, null(), &mut version, &mut handle) };
            if code != 0 {
                return Err(api_error(code));
            }
            if handle.is_null() {
                return Err(AppError::new("wlan_error"));
            }
            Ok(Self(handle))
        }
        fn interfaces(&self) -> Result<Vec<WLAN_INTERFACE_INFO>> {
            let mut list = null_mut();
            let code = unsafe { WlanEnumInterfaces(self.0, null(), &mut list) };
            if code != 0 {
                return Err(api_error(code));
            }
            if list.is_null() {
                return Err(AppError::new("wlan_error"));
            }
            let _memory = Allocation(list.cast());
            // SAFETY: WLAN allocates dwNumberOfItems entries; cap prevents implausible counts.
            let count = unsafe { (*list).dwNumberOfItems as usize };
            if count > 256 {
                return Err(AppError::new("wlan_error"));
            }
            let entries =
                unsafe { std::ptr::addr_of!((*list).InterfaceInfo).cast::<WLAN_INTERFACE_INFO>() };
            Ok(unsafe { std::slice::from_raw_parts(entries, count) }.to_vec())
        }
        fn connection(&self, guid: &GUID) -> Result<Option<Bssid>> {
            let mut size = 0;
            let mut data = null_mut();
            let mut kind = 0;
            let code = unsafe {
                WlanQueryInterface(
                    self.0,
                    guid,
                    wlan_intf_opcode_current_connection,
                    null(),
                    &mut size,
                    &mut data,
                    &mut kind,
                )
            };
            if code == 5023 || code == 1168 {
                return Ok(None);
            }
            if code != 0 {
                return Err(api_error(code));
            }
            if data.is_null() {
                return Err(AppError::new("wlan_error"));
            }
            let _memory = Allocation(data);
            if (size as usize) < size_of::<WLAN_CONNECTION_ATTRIBUTES>() {
                return Err(AppError::new("wlan_error"));
            }
            let attrs = unsafe { &*data.cast::<WLAN_CONNECTION_ATTRIBUTES>() };
            Ok((attrs.isState == wlan_interface_state_connected)
                .then_some(Bssid(attrs.wlanAssociationAttributes.dot11Bssid)))
        }
        fn entries(&self, guid: &GUID) -> Result<Vec<WLAN_BSS_ENTRY>> {
            let mut list = null_mut();
            let code = unsafe {
                WlanGetNetworkBssList(
                    self.0,
                    guid,
                    null(),
                    dot11_BSS_type_any,
                    0,
                    null(),
                    &mut list,
                )
            };
            if code != 0 {
                return Err(api_error(code));
            }
            if list.is_null() {
                return Err(AppError::new("wlan_error"));
            }
            let _memory = Allocation(list.cast());
            let total_size = unsafe { std::ptr::addr_of!((*list).dwTotalSize).read() } as usize;
            let count = unsafe { std::ptr::addr_of!((*list).dwNumberOfItems).read() } as usize;
            let required = offset_of!(WLAN_BSS_LIST, wlanBssEntries).checked_add(
                count
                    .checked_mul(size_of::<WLAN_BSS_ENTRY>())
                    .ok_or_else(|| AppError::new("wlan_error"))?,
            );
            if count > 4096 || required.is_none_or(|n| n > total_size) {
                return Err(AppError::new("wlan_error"));
            }
            // Information-element offsets are deliberately not dereferenced.
            let entries =
                unsafe { std::ptr::addr_of!((*list).wlanBssEntries).cast::<WLAN_BSS_ENTRY>() };
            Ok(unsafe { std::slice::from_raw_parts(entries, count) }.to_vec())
        }
    }
    fn guid_id(g: &GUID) -> String {
        format!(
            "{:08x}-{:04x}-{:04x}-{}",
            g.data1,
            g.data2,
            g.data3,
            g.data4
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
    }
    impl WirelessAdapter for WindowsWlan {
        fn adapters(&self) -> Result<Vec<Adapter>> {
            Session::open()?.interfaces().map(|list| {
                list.into_iter()
                    .map(|i| Adapter {
                        id: guid_id(&i.InterfaceGuid),
                        name: String::from_utf16_lossy(
                            &i.strInterfaceDescription[..i
                                .strInterfaceDescription
                                .iter()
                                .position(|c| *c == 0)
                                .unwrap_or(256)],
                        ),
                        connected: i.isState == wlan_interface_state_connected,
                    })
                    .collect()
            })
        }
        fn scan(
            &self,
            adapter: &str,
            target: Option<Bssid>,
            cancel: &Cancellation,
        ) -> Result<ScanSnapshot> {
            if cancel.is_cancelled() {
                return Err(AppError::new("cancelled"));
            }
            let session = Session::open()?;
            let iface = session
                .interfaces()?
                .into_iter()
                .find(|i| guid_id(&i.InterfaceGuid) == adapter)
                .ok_or_else(|| AppError::new("wlan_unavailable"))?;
            let before = session.connection(&iface.InterfaceGuid)?;
            let requested_ms = unix_ms();
            let code = unsafe { WlanScan(session.0, &iface.InterfaceGuid, null(), null(), null()) };
            if code != 0 {
                return Err(api_error(code));
            }
            // WlanScan is asynchronous. Poll only during this explicit bounded job,
            // accepting host timestamps after the request, never the cached list.
            let start = Instant::now();
            let mut observations = Vec::new();
            let mut stale = 0;
            loop {
                if cancel.is_cancelled() {
                    return Err(AppError::new("cancelled"));
                }
                if start.elapsed() >= Duration::from_secs(5) {
                    break;
                }
                thread::sleep(Duration::from_millis(200));
                let now = unix_ms();
                let entries = session.entries(&iface.InterfaceGuid)?;
                stale = 0;
                observations.clear();
                for e in entries {
                    if target.is_some_and(|b| b.0 != e.dot11Bssid) {
                        continue;
                    }
                    let Some(observed_ms) = host_timestamp_ms(e.ullHostTimestamp) else {
                        stale += 1;
                        continue;
                    };
                    if !fresh(observed_ms, requested_ms, now) {
                        stale += 1;
                        continue;
                    }
                    if e.dot11Ssid.uSSIDLength > 32 {
                        continue;
                    }
                    let frequency = (e.dot11BssPhyType != dot11_phy_type_fhss
                        && e.ulChCenterFrequency > 0)
                        .then_some(e.ulChCenterFrequency);
                    observations.push(Observation {
                        adapter_id: adapter.into(),
                        ssid: e.dot11Ssid.ucSSID[..e.dot11Ssid.uSSIDLength as usize].to_vec(),
                        bssid: Bssid(e.dot11Bssid),
                        frequency_khz: frequency,
                        channel: frequency.and_then(channel_from_khz),
                        rssi_dbm: (-127..=0).contains(&e.lRssi).then_some(e.lRssi),
                        link_quality_percent: (e.uLinkQuality <= 100)
                            .then_some(e.uLinkQuality as u8),
                        collected_unix_ms: now,
                        observed_unix_ms: observed_ms,
                        connected_bssid: before,
                        source: ObservationSource::WindowsBss,
                        ssid_encoding: None,
                        android_observed_elapsed_us: None,
                    });
                }
                if (target.is_some() && !observations.is_empty())
                    || (!observations.is_empty() && start.elapsed() >= Duration::from_secs(4))
                {
                    break;
                }
            }
            stable_connection(before, session.connection(&iface.InterfaceGuid)?)?;
            if cancel.is_cancelled() {
                return Err(AppError::new("cancelled"));
            }
            if observations.is_empty() {
                return Err(AppError::new(if stale > 0 {
                    "wlan_stale"
                } else {
                    "wlan_no_networks"
                }));
            }
            observations.sort_by_key(|o| o.bssid.0);
            observations.dedup_by_key(|o| o.bssid);
            Ok(ScanSnapshot {
                observations,
                discarded_stale: stale,
            })
        }
    }
}
#[cfg(not(windows))]
impl WirelessAdapter for WindowsWlan {
    fn adapters(&self) -> Result<Vec<Adapter>> {
        Err(AppError::new("windows_only"))
    }
    fn scan(&self, _: &str, _: Option<Bssid>, _: &Cancellation) -> Result<ScanSnapshot> {
        Err(AppError::new("windows_only"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_future_unknown_timestamps_and_roam() {
        assert!(!fresh(999, 1000, 1100));
        assert!(fresh(1001, 1000, 1100));
        assert!(!fresh(5000, 1000, 1100));
        assert_eq!(host_timestamp_ms(0), None);
        assert_eq!(host_timestamp_ms(WINDOWS_EPOCH_TICKS + 10_000), Some(1));
        let b = Bssid([1, 2, 3, 4, 5, 6]);
        assert!(stable_connection(Some(b), None).is_err());
        assert!(stable_connection(Some(b), Some(b)).is_ok());
        assert_eq!(api_error(5).key, "wlan_permission");
    }
    struct FailingAdapter;
    impl WirelessAdapter for FailingAdapter {
        fn adapters(&self) -> Result<Vec<Adapter>> {
            Ok(Vec::new())
        }
        fn scan(&self, _: &str, _: Option<Bssid>, _: &Cancellation) -> Result<ScanSnapshot> {
            Err(api_error(1062))
        }
    }
    #[test]
    fn adapter_error_is_not_a_zero_signal_sample() {
        let adapter: Box<dyn WirelessAdapter> = Box::new(FailingAdapter);
        assert!(adapter.adapters().unwrap().is_empty());
        assert_eq!(
            adapter
                .scan("fake", None, &Cancellation::default())
                .unwrap_err()
                .key,
            "wlan_service"
        );
    }
}
