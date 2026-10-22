use crate::log;
use serde::Deserialize;
use std::collections::HashMap;
use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::Threading::GetSystemTimes;
use wmi::WMIConnection;

#[derive(Debug, Clone, Copy, Default)]
pub struct Loads {
    pub cpu: Option<u8>,
    pub gpu: Option<u8>,
}

#[derive(Deserialize)]
struct GpuEngine {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "UtilizationPercentage")]
    utilization: u64,
}

pub struct LoadSampler {
    previous: Option<(u64, u64, u64)>,
}

impl LoadSampler {
    pub fn new() -> Self {
        Self { previous: None }
    }

    pub fn sample(&mut self) -> Loads {
        Loads {
            cpu: self.cpu(),
            gpu: self.gpu(),
        }
    }

    fn cpu(&mut self) -> Option<u8> {
        let mut idle = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut kernel = idle;
        let mut user = idle;
        if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
            log::error(format_args!(
                "cpu load: GetSystemTimes failed: {}",
                std::io::Error::last_os_error()
            ));
            return None;
        }

        let idle = filetime(idle);
        let kernel = filetime(kernel);
        let user = filetime(user);
        let previous = self.previous.replace((idle, kernel, user))?;

        let idle_delta = idle.saturating_sub(previous.0);
        let total = kernel.saturating_sub(previous.1) + user.saturating_sub(previous.2);
        if total == 0 {
            log::debug(format_args!("cpu load: no sample delta yet"));
            return None;
        }

        let busy = total.saturating_sub(idle_delta);
        let load = ((busy as f64 / total as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8;
        log::debug(format_args!(
            "cpu load: idle_delta={idle_delta} total_delta={total} -> {load}%"
        ));
        Some(load)
    }

    fn gpu(&self) -> Option<u8> {
        let con = match WMIConnection::with_namespace_path("ROOT\\CIMV2") {
            Ok(con) => con,
            Err(error) => {
                log::error(format_args!("gpu load: CIMV2 connection failed: {error:#}"));
                return None;
            }
        };
        let query = "SELECT Name, UtilizationPercentage \
                     FROM Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine";
        let engines: Vec<GpuEngine> = match con.raw_query(query) {
            Ok(engines) => engines,
            Err(error) => {
                log::error(format_args!("gpu load: GPUEngine query failed: {error:#}"));
                return None;
            }
        };
        log::debug(format_args!("gpu load: {} engine row(s)", engines.len()));

        let mut per_adapter: HashMap<&str, u64> = HashMap::new();
        for engine in &engines {
            if !engine.name.contains("engtype_3D") {
                continue;
            }
            let Some(rest) = engine.name.split("_luid_").nth(1) else {
                continue;
            };
            let Some(luid) = rest.split("_phys_").next() else {
                continue;
            };
            *per_adapter.entry(luid).or_default() += engine.utilization;
        }

        let load = per_adapter.values().copied().max().unwrap_or(0);
        log::debug(format_args!(
            "gpu load: adapters={per_adapter:?} -> {}%",
            load.min(100)
        ));
        Some(load.min(100) as u8)
    }
}

fn filetime(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}
