use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;

use mach2::kern_return::KERN_SUCCESS;
use mach2::traps::mach_task_self;
use objc2_core_foundation::{CFDictionary, CFRetained};
use objc2_io_kit::{
    IOConnectCallStructMethod, IOObjectRelease, IOServiceClose, IOServiceGetMatchingService,
    IOServiceMatching, IOServiceOpen, io_connect_t, kIOMainPortDefault,
};

pub type FourCc = [u8; 4];

#[derive(Debug, thiserror::Error)]
pub enum SmcError {
    #[error("AppleSMC service not found")]
    ServiceNotFound,
    #[error("{call} failed with kern_return_t {code:#x}")]
    Kern { call: &'static str, code: i32 },
    #[error("SMC key {key} not found")]
    KeyNotFound { key: String },
    #[error("SMC key {key} has unsupported type {data_type}")]
    UnsupportedType { key: String, data_type: String },
    #[error("SMC returned result byte {result:#x} for key {key}")]
    Result { key: String, result: u8 },
    #[error("invalid SMC key name {key:?}")]
    InvalidKey { key: String },
    #[error("SMC key {key}: {detail}")]
    Malformed { key: String, detail: String },
}

const SELECTOR_HANDLE_YPC_EVENT: u32 = 2;
const CMD_READ_BYTES: u8 = 5;
const CMD_READ_KEY_INFO: u8 = 9;
const RESULT_SUCCESS: u8 = 0;
const RESULT_KEY_NOT_FOUND: u8 = 0x84;
const MAX_DATA_SIZE: usize = 32;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct SmcVersion {
    major: u8,
    minor: u8,
    build: u8,
    reserved: u8,
    release: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct SmcPLimitData {
    version: u16,
    length: u16,
    cpu_p_limit: u32,
    gpu_p_limit: u32,
    mem_p_limit: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct SmcKeyInfo {
    data_size: u32,
    data_type: u32,
    data_attributes: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct SmcKeyData {
    key: u32,
    vers: SmcVersion,
    p_limit_data: SmcPLimitData,
    key_info: SmcKeyInfo,
    result: u8,
    status: u8,
    data8: u8,
    data32: u32,
    bytes: [u8; MAX_DATA_SIZE],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KeyInfo {
    size: usize,
    data_type: FourCc,
}

fn expected_len(data_type: FourCc) -> Option<usize> {
    match &data_type {
        b"flt " | b"ui32" => Some(4),
        b"sp78" | b"sp87" | b"sp96" | b"spa5" | b"fpe2" | b"ui16" => Some(2),
        b"ui8 " => Some(1),
        _ => None,
    }
}

pub fn decode(data_type: FourCc, bytes: &[u8]) -> Option<f32> {
    if expected_len(data_type)? != bytes.len() {
        return None;
    }
    let value = match (&data_type, bytes) {
        (b"flt ", &[a, b, c, d]) => f32::from_le_bytes([a, b, c, d]),
        (b"sp78", &[a, b]) => f32::from(i16::from_be_bytes([a, b])) / 256.0,
        (b"sp87", &[a, b]) => f32::from(i16::from_be_bytes([a, b])) / 128.0,
        (b"sp96", &[a, b]) => f32::from(i16::from_be_bytes([a, b])) / 64.0,
        (b"spa5", &[a, b]) => f32::from(i16::from_be_bytes([a, b])) / 32.0,
        (b"fpe2", &[a, b]) => f32::from(u16::from_be_bytes([a, b])) / 4.0,
        (b"ui8 ", &[a]) => f32::from(a),
        (b"ui16", &[a, b]) => f32::from(u16::from_be_bytes([a, b])),
        (b"ui32", &[a, b, c, d]) => u32::from_be_bytes([a, b, c, d]) as f32,
        _ => return None,
    };
    value.is_finite().then_some(value)
}

fn parse_key(key: &str) -> Result<FourCc, SmcError> {
    let code: FourCc = key
        .as_bytes()
        .try_into()
        .map_err(|_| SmcError::InvalidKey { key: key.into() })?;
    if code.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
        Ok(code)
    } else {
        Err(SmcError::InvalidKey { key: key.into() })
    }
}

fn fourcc_string(code: FourCc) -> String {
    String::from_utf8_lossy(&code).into_owned()
}

fn check_result(code: FourCc, result: u8) -> Result<(), SmcError> {
    match result {
        RESULT_SUCCESS => Ok(()),
        RESULT_KEY_NOT_FOUND => Err(SmcError::KeyNotFound {
            key: fourcc_string(code),
        }),
        other => Err(SmcError::Result {
            key: fourcc_string(code),
            result: other,
        }),
    }
}

fn key_info_from(code: FourCc, reply: &SmcKeyData) -> Result<KeyInfo, SmcError> {
    let data_type = reply.key_info.data_type.to_be_bytes();
    let size = usize::try_from(reply.key_info.data_size).unwrap_or(usize::MAX);
    if size == 0 || size > MAX_DATA_SIZE {
        return Err(SmcError::Malformed {
            key: fourcc_string(code),
            detail: format!("data size {size} outside 1..={MAX_DATA_SIZE}"),
        });
    }
    let Some(expected) = expected_len(data_type) else {
        return Err(SmcError::UnsupportedType {
            key: fourcc_string(code),
            data_type: fourcc_string(data_type),
        });
    };
    if expected != size {
        return Err(SmcError::Malformed {
            key: fourcc_string(code),
            detail: format!(
                "type {} expects {expected} bytes, key reports {size}",
                fourcc_string(data_type)
            ),
        });
    }
    Ok(KeyInfo { size, data_type })
}

pub struct SmcConnection {
    connection: io_connect_t,
    key_info: HashMap<FourCc, KeyInfo>,
}

impl SmcConnection {
    pub fn open() -> Result<Self, SmcError> {
        let matching =
            unsafe { IOServiceMatching(c"AppleSMC".as_ptr()) }.ok_or(SmcError::ServiceNotFound)?;
        let matching = unsafe { CFRetained::cast_unchecked::<CFDictionary>(matching) };
        let service = unsafe { IOServiceGetMatchingService(kIOMainPortDefault, Some(matching)) };
        if service == 0 {
            return Err(SmcError::ServiceNotFound);
        }
        let mut connection: io_connect_t = 0;
        let kr = unsafe { IOServiceOpen(service, mach_task_self(), 0, &mut connection) };
        let release_kr = IOObjectRelease(service);
        if release_kr != KERN_SUCCESS {
            log::warn!(
                "IOObjectRelease(AppleSMC service) failed with kern_return_t {release_kr:#x}"
            );
        }
        if kr != KERN_SUCCESS {
            return Err(SmcError::Kern {
                call: "IOServiceOpen",
                code: kr,
            });
        }
        Ok(SmcConnection {
            connection,
            key_info: HashMap::new(),
        })
    }

    pub fn read_f32(&mut self, key: &str) -> Result<f32, SmcError> {
        let code = parse_key(key)?;
        let info = self.key_info(code)?;
        let reply = self.call(
            code,
            &SmcKeyData {
                key: u32::from_be_bytes(code),
                key_info: SmcKeyInfo {
                    data_size: u32::try_from(info.size).unwrap_or(u32::MAX),
                    ..SmcKeyInfo::default()
                },
                data8: CMD_READ_BYTES,
                ..SmcKeyData::default()
            },
        )?;
        decode(info.data_type, &reply.bytes[..info.size]).ok_or_else(|| SmcError::Malformed {
            key: fourcc_string(code),
            detail: format!(
                "value is not a finite {} number",
                fourcc_string(info.data_type)
            ),
        })
    }

    fn key_info(&mut self, code: FourCc) -> Result<KeyInfo, SmcError> {
        if let Some(info) = self.key_info.get(&code) {
            return Ok(*info);
        }
        let reply = self.call(
            code,
            &SmcKeyData {
                key: u32::from_be_bytes(code),
                data8: CMD_READ_KEY_INFO,
                ..SmcKeyData::default()
            },
        )?;
        let info = key_info_from(code, &reply)?;
        self.key_info.insert(code, info);
        Ok(info)
    }

    fn call(&self, code: FourCc, input: &SmcKeyData) -> Result<SmcKeyData, SmcError> {
        let mut output = SmcKeyData::default();
        let mut output_len = size_of::<SmcKeyData>();
        let kr = unsafe {
            IOConnectCallStructMethod(
                self.connection,
                SELECTOR_HANDLE_YPC_EVENT,
                ptr::from_ref(input).cast::<c_void>(),
                size_of::<SmcKeyData>(),
                ptr::from_mut(&mut output).cast::<c_void>(),
                &mut output_len,
            )
        };
        if kr != KERN_SUCCESS {
            return Err(SmcError::Kern {
                call: "IOConnectCallStructMethod",
                code: kr,
            });
        }
        if output_len != size_of::<SmcKeyData>() {
            return Err(SmcError::Malformed {
                key: fourcc_string(code),
                detail: format!(
                    "reply is {output_len} bytes, expected {}",
                    size_of::<SmcKeyData>()
                ),
            });
        }
        check_result(code, output.result)?;
        Ok(output)
    }
}

impl Drop for SmcConnection {
    fn drop(&mut self) {
        let kr = IOServiceClose(self.connection);
        if kr != KERN_SUCCESS {
            log::warn!("IOServiceClose(AppleSMC) failed with kern_return_t {kr:#x}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    #[test]
    fn key_data_matches_kernel_layout() {
        assert_eq!(size_of::<SmcKeyData>(), 80);
        assert_eq!(size_of::<SmcVersion>(), 6);
        assert_eq!(size_of::<SmcPLimitData>(), 16);
        assert_eq!(size_of::<SmcKeyInfo>(), 12);
        assert_eq!(offset_of!(SmcKeyData, key), 0);
        assert_eq!(offset_of!(SmcKeyData, vers), 4);
        assert_eq!(offset_of!(SmcKeyData, vers.major), 4);
        assert_eq!(offset_of!(SmcKeyData, vers.minor), 5);
        assert_eq!(offset_of!(SmcKeyData, vers.build), 6);
        assert_eq!(offset_of!(SmcKeyData, vers.reserved), 7);
        assert_eq!(offset_of!(SmcKeyData, vers.release), 8);
        assert_eq!(offset_of!(SmcKeyData, p_limit_data), 12);
        assert_eq!(offset_of!(SmcKeyData, p_limit_data.version), 12);
        assert_eq!(offset_of!(SmcKeyData, p_limit_data.length), 14);
        assert_eq!(offset_of!(SmcKeyData, p_limit_data.cpu_p_limit), 16);
        assert_eq!(offset_of!(SmcKeyData, p_limit_data.gpu_p_limit), 20);
        assert_eq!(offset_of!(SmcKeyData, p_limit_data.mem_p_limit), 24);
        assert_eq!(offset_of!(SmcKeyData, key_info), 28);
        assert_eq!(offset_of!(SmcKeyData, key_info.data_size), 28);
        assert_eq!(offset_of!(SmcKeyData, key_info.data_type), 32);
        assert_eq!(offset_of!(SmcKeyData, key_info.data_attributes), 36);
        assert_eq!(offset_of!(SmcKeyData, result), 40);
        assert_eq!(offset_of!(SmcKeyData, status), 41);
        assert_eq!(offset_of!(SmcKeyData, data8), 42);
        assert_eq!(offset_of!(SmcKeyData, data32), 44);
        assert_eq!(offset_of!(SmcKeyData, bytes), 48);
    }

    #[test]
    fn decodes_float_little_endian() {
        let bytes = 12.5f32.to_le_bytes();
        assert_eq!(decode(*b"flt ", &bytes), Some(12.5));
    }

    #[test]
    fn rejects_non_finite_float() {
        assert_eq!(decode(*b"flt ", &f32::NAN.to_le_bytes()), None);
        assert_eq!(decode(*b"flt ", &f32::INFINITY.to_le_bytes()), None);
    }

    #[test]
    fn decodes_signed_fixed_point() {
        assert_eq!(decode(*b"sp78", &[0x2d, 0x80]), Some(45.5));
        assert_eq!(decode(*b"sp78", &[0xff, 0x00]), Some(-1.0));
        assert_eq!(decode(*b"sp87", &[0x01, 0x40]), Some(2.5));
        assert_eq!(decode(*b"sp96", &[0x02, 0x60]), Some(9.5));
        assert_eq!(decode(*b"sp96", &[0x00, 0x00]), Some(0.0));
        assert_eq!(decode(*b"sp96", &[0x80, 0x00]), Some(-512.0));
        assert_eq!(decode(*b"spa5", &[0x00, 0x50]), Some(2.5));
    }

    #[test]
    fn decodes_unsigned_types() {
        assert_eq!(decode(*b"fpe2", &[0x0b, 0xb8]), Some(750.0));
        assert_eq!(decode(*b"ui8 ", &[0xfe]), Some(254.0));
        assert_eq!(decode(*b"ui16", &[0x12, 0x34]), Some(4660.0));
        assert_eq!(decode(*b"ui32", &[0x00, 0x01, 0x00, 0x00]), Some(65536.0));
    }

    #[test]
    fn unknown_type_is_none() {
        assert_eq!(decode(*b"sp3c", &[0x01, 0x02]), None);
        assert_eq!(decode(*b"ch8*", &[0x41]), None);
        assert_eq!(decode([0; 4], &[]), None);
    }

    #[test]
    fn wrong_length_is_none() {
        assert_eq!(decode(*b"flt ", &[0, 0, 0x80]), None);
        assert_eq!(decode(*b"sp96", &[0x02]), None);
        assert_eq!(decode(*b"sp78", &[0x02, 0x60, 0x00]), None);
        assert_eq!(decode(*b"ui8 ", &[]), None);
        assert_eq!(decode(*b"ui16", &[1, 2, 3, 4]), None);
        assert_eq!(decode(*b"ui32", &[1, 2]), None);
    }

    #[test]
    fn parses_printable_four_byte_keys() {
        assert_eq!(parse_key("PCPT").unwrap(), *b"PCPT");
        assert_eq!(parse_key("#KEY").unwrap(), *b"#KEY");
        assert!(matches!(parse_key("PCP"), Err(SmcError::InvalidKey { .. })));
        assert!(matches!(
            parse_key("PCPTX"),
            Err(SmcError::InvalidKey { .. })
        ));
        assert!(matches!(
            parse_key("PC\0T"),
            Err(SmcError::InvalidKey { .. })
        ));
        assert!(matches!(parse_key("PCé"), Err(SmcError::InvalidKey { .. })));
    }

    #[test]
    fn maps_result_byte() {
        assert!(check_result(*b"PCPT", RESULT_SUCCESS).is_ok());
        assert!(matches!(
            check_result(*b"PSTR", RESULT_KEY_NOT_FOUND),
            Err(SmcError::KeyNotFound { key }) if key == "PSTR"
        ));
        assert!(matches!(
            check_result(*b"PCPT", 0x85),
            Err(SmcError::Result { result: 0x85, .. })
        ));
    }

    fn reply_with(data_type: &[u8; 4], size: u32) -> SmcKeyData {
        SmcKeyData {
            key_info: SmcKeyInfo {
                data_size: size,
                data_type: u32::from_be_bytes(*data_type),
                data_attributes: 0,
            },
            ..SmcKeyData::default()
        }
    }

    #[test]
    fn validates_key_info() {
        assert_eq!(
            key_info_from(*b"PCPT", &reply_with(b"sp96", 2)).unwrap(),
            KeyInfo {
                size: 2,
                data_type: *b"sp96"
            }
        );
        assert!(matches!(
            key_info_from(*b"VC0C", &reply_with(b"sp3c", 2)),
            Err(SmcError::UnsupportedType { data_type, .. }) if data_type == "sp3c"
        ));
        assert!(matches!(
            key_info_from(*b"PCPT", &reply_with(b"sp96", 4)),
            Err(SmcError::Malformed { .. })
        ));
        assert!(matches!(
            key_info_from(*b"PCPT", &reply_with(b"flt ", 0)),
            Err(SmcError::Malformed { .. })
        ));
        assert!(matches!(
            key_info_from(*b"PCPT", &reply_with(b"flt ", 33)),
            Err(SmcError::Malformed { .. })
        ));
    }

    #[test]
    fn errors_render_key_and_hex_codes() {
        let kern = SmcError::Kern {
            call: "IOConnectCallStructMethod",
            code: 0xe00002c2_u32 as i32,
        };
        assert_eq!(
            kern.to_string(),
            "IOConnectCallStructMethod failed with kern_return_t 0xe00002c2"
        );
    }
}
