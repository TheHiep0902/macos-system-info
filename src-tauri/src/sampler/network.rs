use std::net::IpAddr;
use std::ptr;
use std::time::{Duration, Instant};

use objc2_core_foundation::{CFArray, CFDictionary, CFRetained, CFString, CFType};
use objc2_system_configuration::{
    SCDynamicStore, SCNetworkInterface, kSCDynamicStoreDomainState,
    kSCDynamicStorePropNetPrimaryInterface, kSCEntNetIPv4, kSCNetworkInterfaceTypeIEEE80211,
};

use crate::model::NetworkSample;

const STORE_NAME: &str = "pulsebar";
const WIFI_LIST_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceAddr {
    pub name: String,
    pub ip: IpAddr,
}

fn is_usable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_link_local(),
        IpAddr::V6(v6) => !v6.is_loopback() && !v6.is_unicast_link_local(),
    }
}

fn usable_addrs<'a>(
    addrs: &'a [InterfaceAddr],
    name: &'a str,
) -> impl Iterator<Item = IpAddr> + 'a {
    addrs
        .iter()
        .filter(move |addr| addr.name == name && is_usable(&addr.ip))
        .map(|addr| addr.ip)
}

pub fn select_wifi(
    wifi_interfaces: &[String],
    addrs: &[InterfaceAddr],
    primary_interface: Option<String>,
) -> NetworkSample {
    let chosen = wifi_interfaces
        .iter()
        .find(|name| usable_addrs(addrs, name).next().is_some())
        .or_else(|| wifi_interfaces.first());
    let (wifi_ipv4, wifi_ipv6) = match chosen {
        Some(name) => (
            usable_addrs(addrs, name)
                .find(IpAddr::is_ipv4)
                .map(|ip| ip.to_string()),
            usable_addrs(addrs, name)
                .find(IpAddr::is_ipv6)
                .map(|ip| ip.to_string()),
        ),
        None => (None, None),
    };
    NetworkSample {
        wifi_interface: chosen.cloned(),
        is_wifi_connected: wifi_ipv4.is_some() || wifi_ipv6.is_some(),
        wifi_ipv4,
        wifi_ipv6,
        primary_interface,
    }
}

fn wifi_bsd_names() -> Vec<String> {
    let all = SCNetworkInterface::all();
    let interfaces: &CFArray<SCNetworkInterface> = unsafe { all.cast_unchecked() };
    let wifi_type: &CFString = unsafe { kSCNetworkInterfaceTypeIEEE80211 };
    interfaces
        .iter()
        .filter(|iface| {
            iface
                .interface_type()
                .is_some_and(|kind| *kind == *wifi_type)
        })
        .filter_map(|iface| iface.bsd_name())
        .map(|name| name.to_string())
        .collect()
}

fn open_store() -> Option<CFRetained<SCDynamicStore>> {
    unsafe { SCDynamicStore::new(None, &CFString::from_str(STORE_NAME), None, ptr::null_mut()) }
}

fn primary_from_store(store: &SCDynamicStore, key: &CFString) -> Option<String> {
    let value = SCDynamicStore::value(Some(store), key)?;
    let dict = value.downcast::<CFDictionary>().ok()?;
    let dict: &CFDictionary<CFString, CFType> = unsafe { dict.cast_unchecked() };
    let name = dict.get(unsafe { kSCDynamicStorePropNetPrimaryInterface })?;
    let name = name.downcast::<CFString>().ok()?;
    Some(name.to_string())
}

pub struct NetworkReader {
    store: Option<CFRetained<SCDynamicStore>>,
    primary_key: CFRetained<CFString>,
    wifi_names: Vec<String>,
    wifi_names_at: Option<Instant>,
    store_warned: bool,
    addrs_warned: bool,
}

impl NetworkReader {
    pub fn new() -> Self {
        let primary_key = SCDynamicStore::key_create_network_global_entity(
            None,
            unsafe { kSCDynamicStoreDomainState },
            unsafe { kSCEntNetIPv4 },
        );
        NetworkReader {
            store: open_store(),
            primary_key,
            wifi_names: Vec::new(),
            wifi_names_at: None,
            store_warned: false,
            addrs_warned: false,
        }
    }

    pub fn read(&mut self) -> NetworkSample {
        self.refresh_wifi_names();
        let primary = self.primary_interface();
        let addrs = self.wifi_addrs();
        select_wifi(&self.wifi_names, &addrs, primary)
    }

    fn refresh_wifi_names(&mut self) {
        let now = Instant::now();
        let fresh = self
            .wifi_names_at
            .is_some_and(|at| now.saturating_duration_since(at) < WIFI_LIST_TTL);
        if !fresh {
            self.wifi_names = wifi_bsd_names();
            self.wifi_names_at = Some(now);
        }
    }

    fn primary_interface(&mut self) -> Option<String> {
        if self.store.is_none() {
            self.store = open_store();
        }
        let Some(store) = self.store.as_deref() else {
            if !self.store_warned {
                self.store_warned = true;
                log::warn!("SCDynamicStoreCreate failed; primary interface unknown");
            }
            return None;
        };
        self.store_warned = false;
        primary_from_store(store, &self.primary_key)
    }

    fn wifi_addrs(&mut self) -> Vec<InterfaceAddr> {
        if self.wifi_names.is_empty() {
            return Vec::new();
        }
        match if_addrs::get_if_addrs() {
            Ok(interfaces) => {
                self.addrs_warned = false;
                interfaces
                    .into_iter()
                    .filter(|iface| self.wifi_names.contains(&iface.name))
                    .map(|iface| InterfaceAddr {
                        ip: iface.ip(),
                        name: iface.name,
                    })
                    .collect()
            }
            Err(err) => {
                if !self.addrs_warned {
                    self.addrs_warned = true;
                    log::warn!("listing interface addresses failed: {err}");
                }
                Vec::new()
            }
        }
    }
}

impl Default for NetworkReader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(name: &str, ip: &str) -> InterfaceAddr {
        InterfaceAddr {
            name: name.into(),
            ip: ip.parse().unwrap(),
        }
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn picks_wifi_interface_that_is_not_en0() {
        let addrs = [
            addr("en0", "10.0.0.5"),
            addr("en1", "fe80::1"),
            addr("en1", "192.168.1.23"),
            addr("en1", "2001:db8::23"),
        ];
        let sample = select_wifi(&names(&["en1"]), &addrs, Some("en0".into()));
        assert_eq!(sample.wifi_interface.as_deref(), Some("en1"));
        assert_eq!(sample.wifi_ipv4.as_deref(), Some("192.168.1.23"));
        assert_eq!(sample.wifi_ipv6.as_deref(), Some("2001:db8::23"));
        assert!(sample.is_wifi_connected);
        assert_eq!(sample.primary_interface.as_deref(), Some("en0"));
    }

    #[test]
    fn skips_loopback_and_link_local_addresses() {
        let addrs = [
            addr("en0", "169.254.10.20"),
            addr("en0", "fe80::abcd"),
            addr("en0", "febf::1"),
            addr("en0", "127.0.0.1"),
            addr("en0", "::1"),
        ];
        let sample = select_wifi(&names(&["en0"]), &addrs, None);
        assert_eq!(sample.wifi_interface.as_deref(), Some("en0"));
        assert_eq!(sample.wifi_ipv4, None);
        assert_eq!(sample.wifi_ipv6, None);
        assert!(!sample.is_wifi_connected);
    }

    #[test]
    fn prefers_first_wifi_interface_with_usable_address() {
        let addrs = [
            addr("en2", "169.254.1.1"),
            addr("en3", "172.16.0.9"),
            addr("en3", "172.16.0.10"),
        ];
        let sample = select_wifi(&names(&["en2", "en3"]), &addrs, Some("en3".into()));
        assert_eq!(sample.wifi_interface.as_deref(), Some("en3"));
        assert_eq!(sample.wifi_ipv4.as_deref(), Some("172.16.0.9"));
        assert!(sample.is_wifi_connected);
    }

    #[test]
    fn falls_back_to_first_wifi_name_when_disconnected() {
        let addrs = [addr("en0", "10.0.0.5")];
        let sample = select_wifi(&names(&["en1", "en2"]), &addrs, Some("en0".into()));
        assert_eq!(sample.wifi_interface.as_deref(), Some("en1"));
        assert!(!sample.is_wifi_connected);
        assert_eq!(sample.primary_interface.as_deref(), Some("en0"));
    }

    #[test]
    fn ipv6_only_counts_as_connected() {
        let addrs = [addr("en1", "fe80::2"), addr("en1", "2001:db8::7")];
        let sample = select_wifi(&names(&["en1"]), &addrs, None);
        assert_eq!(sample.wifi_ipv4, None);
        assert_eq!(sample.wifi_ipv6.as_deref(), Some("2001:db8::7"));
        assert!(sample.is_wifi_connected);
    }

    #[test]
    fn no_wifi_hardware_yields_empty_sample() {
        let addrs = [addr("en0", "10.0.0.5")];
        let sample = select_wifi(&[], &addrs, Some("en0".into()));
        assert_eq!(
            sample,
            NetworkSample {
                primary_interface: Some("en0".into()),
                ..NetworkSample::default()
            }
        );
    }

    #[test]
    fn live_reader_is_self_consistent() {
        let mut reader = NetworkReader::new();
        let sample = reader.read();
        let wifi = wifi_bsd_names();
        assert!(wifi.iter().all(|name| !name.is_empty()));
        if let Some(name) = &sample.wifi_interface {
            assert!(wifi.contains(name));
        }
        assert_eq!(
            sample.is_wifi_connected,
            sample.wifi_ipv4.is_some() || sample.wifi_ipv6.is_some()
        );
        if sample.is_wifi_connected {
            assert!(sample.wifi_interface.is_some());
        }
        let again = reader.read();
        assert_eq!(again.wifi_interface, sample.wifi_interface);
    }
}
