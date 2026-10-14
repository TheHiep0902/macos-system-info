use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    AppleSilicon,
    Intel,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterInfo {
    pub label: String,
    pub name: String,
    pub core_count: u32,
    pub max_mhz: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticInfo {
    pub platform: Platform,
    pub arch: String,
    pub translated: bool,
    pub cpu_brand: String,
    pub physical_cores: u32,
    pub logical_cores: u32,
    pub clusters: Vec<ClusterInfo>,
    pub topology_assumed: bool,
    pub nominal_mhz: Option<u32>,
    pub max_mhz: Option<u32>,
    pub total_memory_bytes: u64,
    pub os_version: String,
    pub model_identifier: String,
    pub boot_time_unix: i64,
    pub power_source_kind: Option<String>,
    pub freq_source_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum MetricState<T> {
    Ok(T),
    Unavailable { reason: String },
    Loading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FreqSource {
    IoReport,
    Nominal,
}

impl FreqSource {
    pub fn as_str(self) -> &'static str {
        match self {
            FreqSource::IoReport => "ioReport",
            FreqSource::Nominal => "nominal",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreSample {
    pub id: u32,
    pub cluster: String,
    pub usage_pct: f32,
    pub freq_mhz: Option<u32>,
    pub active_ratio: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSample {
    pub label: String,
    pub freq_mhz: Option<u32>,
    pub usage_pct: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrequencySample {
    pub source: FreqSource,
    pub avg_mhz: Option<u32>,
    pub min_mhz: Option<u32>,
    pub max_mhz: Option<u32>,
    pub per_core_available: bool,
    pub speed_limit_pct: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuSample {
    pub total_usage_pct: f32,
    pub frequency: MetricState<FrequencySample>,
    pub clusters: Vec<ClusterSample>,
    pub cores: Vec<CoreSample>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerSample {
    pub cpu_watts: f32,
    pub smoothed_cpu_watts: f32,
    pub source: String,
    pub scope: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySample {
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub used_pct: f32,
    pub app_bytes: u64,
    pub wired_bytes: u64,
    pub compressed_bytes: u64,
    pub cached_bytes: u64,
    pub swap_used_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSample {
    pub id: String,
    pub name: String,
    pub mount_point: String,
    pub fs_type: String,
    pub is_boot: bool,
    pub is_removable: bool,
    pub is_network: bool,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub available_important_bytes: Option<u64>,
    pub used_bytes: u64,
    pub used_pct: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSample {
    pub wifi_interface: Option<String>,
    pub wifi_ipv4: Option<String>,
    pub wifi_ipv6: Option<String>,
    pub is_wifi_connected: bool,
    pub primary_interface: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub seq: u64,
    pub timestamp_ms: i64,
    pub cpu: CpuSample,
    pub power: MetricState<PowerSample>,
    pub memory: MemorySample,
    pub disks: Vec<DiskSample>,
    pub network: NetworkSample,
    pub uptime_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const FIXTURE: &str = include_str!("../../src/mocks/contract-snapshot.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("contract fixture is valid JSON")
    }

    fn static_info() -> StaticInfo {
        StaticInfo {
            platform: Platform::AppleSilicon,
            arch: "arm64".into(),
            translated: false,
            cpu_brand: "Apple M3 Pro".into(),
            physical_cores: 12,
            logical_cores: 12,
            clusters: vec![
                ClusterInfo {
                    label: "E".into(),
                    name: "Efficiency".into(),
                    core_count: 6,
                    max_mhz: Some(2748),
                },
                ClusterInfo {
                    label: "P".into(),
                    name: "Performance".into(),
                    core_count: 6,
                    max_mhz: None,
                },
            ],
            topology_assumed: false,
            nominal_mhz: None,
            max_mhz: Some(4056),
            total_memory_bytes: 38_654_705_664,
            os_version: "15.6.1".into(),
            model_identifier: "Mac15,6".into(),
            boot_time_unix: 1_760_400_000,
            power_source_kind: Some("ioreport".into()),
            freq_source_kind: None,
        }
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            seq: 42,
            timestamp_ms: 1_760_400_123_456,
            cpu: CpuSample {
                total_usage_pct: 23.5,
                frequency: MetricState::Ok(FrequencySample {
                    source: FreqSource::IoReport,
                    avg_mhz: Some(2400),
                    min_mhz: Some(1020),
                    max_mhz: Some(3504),
                    per_core_available: true,
                    speed_limit_pct: Some(100),
                }),
                clusters: vec![
                    ClusterSample {
                        label: "E".into(),
                        freq_mhz: Some(1020),
                        usage_pct: 12.25,
                    },
                    ClusterSample {
                        label: "P".into(),
                        freq_mhz: None,
                        usage_pct: 34.75,
                    },
                ],
                cores: vec![
                    CoreSample {
                        id: 0,
                        cluster: "E".into(),
                        usage_pct: 12.5,
                        freq_mhz: Some(1020),
                        active_ratio: Some(0.25),
                    },
                    CoreSample {
                        id: 6,
                        cluster: "P".into(),
                        usage_pct: 50.5,
                        freq_mhz: None,
                        active_ratio: None,
                    },
                ],
            },
            power: MetricState::Ok(PowerSample {
                cpu_watts: 8.5,
                smoothed_cpu_watts: 8.25,
                source: "ioreport".into(),
                scope: "cpu".into(),
            }),
            memory: MemorySample {
                used_bytes: 23_773_642_752,
                total_bytes: 38_654_705_664,
                used_pct: 61.5,
                app_bytes: 15_032_385_536,
                wired_bytes: 4_294_967_296,
                compressed_bytes: 4_446_289_920,
                cached_bytes: 8_589_934_592,
                swap_used_bytes: 0,
            },
            disks: vec![DiskSample {
                id: "fsid-16777234-26".into(),
                name: "Macintosh HD".into(),
                mount_point: "/".into(),
                fs_type: "apfs".into(),
                is_boot: true,
                is_removable: false,
                is_network: false,
                total_bytes: 994_662_584_320,
                available_bytes: 500_000_000_000,
                available_important_bytes: None,
                used_bytes: 494_662_584_320,
                used_pct: 49.75,
            }],
            network: NetworkSample {
                wifi_interface: Some("en0".into()),
                wifi_ipv4: Some("192.168.1.23".into()),
                wifi_ipv6: None,
                is_wifi_connected: true,
                primary_interface: Some("en0".into()),
            },
            uptime_secs: 123_456,
        }
    }

    #[test]
    fn static_info_matches_contract_fixture() {
        let actual = serde_json::to_value(static_info()).unwrap();
        assert_eq!(actual, fixture()["staticInfo"]);
    }

    #[test]
    fn snapshot_matches_contract_fixture() {
        let actual = serde_json::to_value(snapshot()).unwrap();
        assert_eq!(actual, fixture()["snapshot"]);
    }

    #[test]
    fn metric_state_variants_match_contract_fixture() {
        let unavailable: MetricState<PowerSample> = MetricState::Unavailable {
            reason: "Máy ảo không cung cấp cảm biến công suất".into(),
        };
        let loading: MetricState<FrequencySample> = MetricState::Loading;
        let nominal = MetricState::Ok(FrequencySample {
            source: FreqSource::Nominal,
            avg_mhz: None,
            min_mhz: None,
            max_mhz: None,
            per_core_available: false,
            speed_limit_pct: Some(100),
        });
        let fixture = fixture();
        assert_eq!(
            serde_json::to_value(unavailable).unwrap(),
            fixture["powerUnavailable"]
        );
        assert_eq!(
            serde_json::to_value(loading).unwrap(),
            fixture["frequencyLoading"]
        );
        assert_eq!(
            serde_json::to_value(nominal).unwrap(),
            fixture["frequencyNominal"]
        );
    }

    #[test]
    fn intel_platform_serializes_camel_case() {
        assert_eq!(
            serde_json::to_value(Platform::Intel).unwrap(),
            json!("intel")
        );
        assert_eq!(FreqSource::IoReport.as_str(), "ioReport");
        assert_eq!(
            serde_json::to_value(FreqSource::Nominal).unwrap(),
            json!(FreqSource::Nominal.as_str())
        );
    }
}
