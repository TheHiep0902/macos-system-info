#[cfg(target_arch = "x86_64")]
fn main() -> std::process::ExitCode {
    match intel::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("probe_intel failed: {err}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn main() {
    println!("probe_intel is x86_64-only");
}

#[cfg(target_arch = "x86_64")]
mod intel {
    use std::error::Error;
    use std::thread;
    use std::time::{Duration, Instant};

    use pulsebar_lib::sampler::power::intel_ioreport::{
        IntelIoReport, IoReportFreq, IoReportPower,
    };
    use pulsebar_lib::sampler::power::intel_smc::{KEY_CANDIDATES, SmcPower};
    use pulsebar_lib::sampler::power::{FreqSource, PowerSource, SAMPLE_INTERVAL, Tick};
    use pulsebar_lib::smc::SmcConnection;
    use pulsebar_lib::sys::{sysctl_u32, sysctl_u64};

    const SAMPLES: u64 = 5;

    fn thread_cpu_time() -> Result<Duration, Box<dyn Error>> {
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let rc = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut now) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Duration::new(
            u64::try_from(now.tv_sec)?,
            u32::try_from(now.tv_nsec)?,
        ))
    }

    fn watts(value: Option<f32>) -> String {
        value.map_or_else(|| "n/a".to_string(), |w| format!("{w:.2} W"))
    }

    fn mhz(value: Option<u32>) -> String {
        value.map_or_else(|| "n/a".to_string(), |v| v.to_string())
    }

    pub fn run() -> Result<(), Box<dyn Error>> {
        let base_mhz = sysctl_u64("hw.cpufrequency")
            .ok()
            .and_then(|hz| u32::try_from(hz / 1_000_000).ok());
        let logical_cpus = sysctl_u32("hw.logicalcpu")?;
        println!("base {} MHz, {logical_cpus} logical CPUs", mhz(base_mhz));

        let setup_start = Instant::now();
        let session = IntelIoReport::open(base_mhz, logical_cpus)?;
        let mut power = IoReportPower::new(session.clone())?;
        let mut freq = IoReportFreq::new(session)?;
        println!(
            "IOReport setup {:.1} ms wall",
            setup_start.elapsed().as_secs_f64() * 1e3
        );

        match SmcPower::probe() {
            Ok(selected) => println!(
                "SmcPower::probe selected {} (scope {})",
                selected.source(),
                selected.scope().as_str()
            ),
            Err(err) => println!("SmcPower::probe failed: {err}"),
        }
        let mut smc = SmcConnection::open()?;

        let start = Instant::now();
        for seq in 0..=SAMPLES {
            let deadline = start + SAMPLE_INTERVAL * u32::try_from(seq)?;
            if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
            let tick = Tick {
                seq,
                now: Instant::now(),
            };
            let cpu_before = thread_cpu_time()?;
            let package = power.read(&tick);
            let frequency = freq.read(&tick);
            let cost = thread_cpu_time()?.saturating_sub(cpu_before);
            let smc_values: Vec<String> = KEY_CANDIDATES
                .iter()
                .map(|key| match smc.read_f32(key) {
                    Ok(value) => format!("{key}={value:.2} W"),
                    Err(err) => format!("{key}=n/a ({err})"),
                })
                .collect();
            if seq == 0 {
                println!(
                    "#0 priming: power {:?}, freq primed {}, cost {:.3} ms",
                    package,
                    frequency.as_ref().is_ok_and(Option::is_none),
                    cost.as_secs_f64() * 1e3
                );
                continue;
            }
            let package = match package {
                Ok(value) => watts(value),
                Err(err) => format!("error {err}"),
            };
            let frequency = match frequency {
                Ok(Some(reading)) => format!(
                    "avg {} min {} max {} MHz, speed limit {:?}%, per-core {:?}, active% {:?}",
                    mhz(reading.avg_mhz),
                    mhz(reading.min_mhz),
                    mhz(reading.max_mhz),
                    reading.speed_limit_pct,
                    reading.core_mhz,
                    reading
                        .core_active_ratio
                        .iter()
                        .map(|ratio| ratio.map(|r| (r * 100.0).round()))
                        .collect::<Vec<_>>(),
                ),
                Ok(None) => "primed".to_string(),
                Err(err) => format!("error {err}"),
            };
            println!(
                "#{seq} ioreport pkg {package} pp0 {} | smc {} | ioreport cpu {:.3} ms\n   {frequency}",
                watts(power.cores_watts()),
                smc_values.join(" "),
                cost.as_secs_f64() * 1e3
            );
        }
        Ok(())
    }
}
