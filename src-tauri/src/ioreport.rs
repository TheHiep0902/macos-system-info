use std::ffi::c_void;
use std::ptr::{self, NonNull};

use objc2_core_foundation::{
    CFArray, CFDictionary, CFMutableArray, CFMutableDictionary, CFRetained, CFString, CFType, Type,
};

use crate::sys::SysError;

const CHANNELS_KEY: &str = "IOReportChannels";
const FORMAT_SIMPLE: u8 = 1;

#[link(name = "IOReport", kind = "dylib")]
unsafe extern "C" {
    fn IOReportCopyChannelsInGroup(
        group: &CFString,
        subgroup: Option<&CFString>,
        channel_id: u64,
        reserved_a: u64,
        reserved_b: u64,
    ) -> Option<NonNull<CFMutableDictionary>>;

    fn IOReportCreateSubscription(
        allocator: *const c_void,
        desired: &CFMutableDictionary,
        subscribed: *mut *mut CFMutableDictionary,
        channel_id: u64,
        options: *const CFType,
    ) -> Option<NonNull<CFType>>;

    fn IOReportCreateSamples(
        subscription: &CFType,
        subscribed: &CFMutableDictionary,
        options: *const CFType,
    ) -> Option<NonNull<CFDictionary>>;

    fn IOReportChannelGetChannelName(channel: &CFDictionary) -> Option<NonNull<CFString>>;

    fn IOReportChannelGetFormat(channel: &CFDictionary) -> u8;

    fn IOReportSimpleGetIntegerValue(channel: &CFDictionary, error: *mut c_void) -> i64;
}

fn channel_array(dictionary: &CFDictionary, key: &CFString) -> Option<CFRetained<CFArray>> {
    let typed: &CFDictionary<CFString, CFType> = unsafe { dictionary.cast_unchecked() };
    typed.get(key)?.downcast::<CFArray>().ok()
}

fn channel_at(channels: &CFArray, index: usize) -> Option<CFRetained<CFDictionary>> {
    let items: &CFArray<CFType> = unsafe { channels.cast_unchecked() };
    items.get(index)?.downcast::<CFDictionary>().ok()
}

fn channel_name(channel: &CFDictionary) -> Option<&CFString> {
    let name = unsafe { IOReportChannelGetChannelName(channel) }?;
    Some(unsafe { name.as_ref() })
}

pub struct IoReportSubscription {
    subscription: CFRetained<CFType>,
    subscribed: CFRetained<CFMutableDictionary>,
    channels_key: CFRetained<CFString>,
    names: Vec<CFRetained<CFString>>,
    labels: Vec<String>,
}

impl IoReportSubscription {
    pub fn new(group: &str, subgroup: Option<&str>, wanted: &[&str]) -> Result<Self, SysError> {
        let channels_key = CFString::from_static_str(CHANNELS_KEY);
        let group_name = CFString::from_str(group);
        let subgroup_name = subgroup.map(CFString::from_str);
        let available =
            unsafe { IOReportCopyChannelsInGroup(&group_name, subgroup_name.as_deref(), 0, 0, 0) }
                .ok_or_else(|| {
                    SysError::invalid(
                        "IOReportCopyChannelsInGroup",
                        format!("no channels for {group}/{}", subgroup.unwrap_or("*")),
                    )
                })?;
        let available = unsafe { CFRetained::from_raw(available) };

        let all = channel_array(&available, &channels_key).ok_or_else(|| {
            SysError::invalid(
                "IOReportCopyChannelsInGroup",
                "missing IOReportChannels array",
            )
        })?;
        let selected = CFMutableArray::<CFDictionary>::with_capacity(wanted.len());
        for index in 0..all.len() {
            let Some(channel) = channel_at(&all, index) else {
                continue;
            };
            if unsafe { IOReportChannelGetFormat(&channel) } != FORMAT_SIMPLE {
                continue;
            }
            let Some(name) = channel_name(&channel) else {
                continue;
            };
            let name = name.to_string();
            if wanted.contains(&name.as_str()) {
                selected.append(&channel);
            }
        }
        if selected.is_empty() {
            return Err(SysError::invalid(
                "IOReportCopyChannelsInGroup",
                format!("none of the {} requested channels exist", wanted.len()),
            ));
        }
        let desired: &CFMutableDictionary<CFString, CFType> = unsafe { available.cast_unchecked() };
        desired.set(&channels_key, &selected);

        let mut subscribed: *mut CFMutableDictionary = ptr::null_mut();
        let subscription = unsafe {
            IOReportCreateSubscription(ptr::null(), &available, &mut subscribed, 0, ptr::null())
        };
        let subscribed =
            NonNull::new(subscribed).map(|subscribed| unsafe { CFRetained::from_raw(subscribed) });
        let subscription = subscription
            .map(|subscription| unsafe { CFRetained::from_raw(subscription) })
            .ok_or_else(|| SysError::invalid("IOReportCreateSubscription", "returned NULL"))?;
        let subscribed = subscribed.ok_or_else(|| {
            SysError::invalid("IOReportCreateSubscription", "no subscribed channels")
        })?;

        let subscribed_channels = channel_array(&subscribed, &channels_key).ok_or_else(|| {
            SysError::invalid(
                "IOReportCreateSubscription",
                "missing IOReportChannels array",
            )
        })?;
        let mut names = Vec::with_capacity(subscribed_channels.len());
        let mut labels = Vec::with_capacity(subscribed_channels.len());
        for index in 0..subscribed_channels.len() {
            let name = channel_at(&subscribed_channels, index)
                .and_then(|channel| channel_name(&channel).map(Type::retain))
                .ok_or_else(|| {
                    SysError::invalid(
                        "IOReportCreateSubscription",
                        format!("subscribed channel {index} has no name"),
                    )
                })?;
            labels.push(name.to_string());
            names.push(name);
        }
        if names.is_empty() {
            return Err(SysError::invalid(
                "IOReportCreateSubscription",
                "subscription is empty",
            ));
        }

        Ok(IoReportSubscription {
            subscription,
            subscribed,
            channels_key,
            names,
            labels,
        })
    }

    pub fn channel_names(&self) -> &[String] {
        &self.labels
    }

    pub fn sample_into(&mut self, out: &mut [Option<i64>]) -> Result<(), SysError> {
        if out.len() != self.names.len() {
            return Err(SysError::invalid(
                "IOReportCreateSamples",
                format!(
                    "buffer has {} slots for {} channels",
                    out.len(),
                    self.names.len()
                ),
            ));
        }
        out.fill(None);
        let samples =
            unsafe { IOReportCreateSamples(&self.subscription, &self.subscribed, ptr::null()) }
                .ok_or_else(|| SysError::invalid("IOReportCreateSamples", "returned NULL"))?;
        let samples = unsafe { CFRetained::from_raw(samples) };
        let channels = channel_array(&samples, &self.channels_key).ok_or_else(|| {
            SysError::invalid("IOReportCreateSamples", "missing IOReportChannels array")
        })?;
        for position in 0..channels.len() {
            let Some(channel) = channel_at(&channels, position) else {
                continue;
            };
            let Some(slot) = channel_name(&channel).and_then(|name| self.slot_for(position, name))
            else {
                continue;
            };
            out[slot] = Some(unsafe { IOReportSimpleGetIntegerValue(&channel, ptr::null_mut()) });
        }
        Ok(())
    }

    fn slot_for(&self, position: usize, name: &CFString) -> Option<usize> {
        if self
            .names
            .get(position)
            .is_some_and(|expected| **expected == *name)
        {
            return Some(position);
        }
        self.names.iter().position(|expected| **expected == *name)
    }
}
