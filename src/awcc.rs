use crate::log;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::thread::sleep;
use std::time::Duration;
use wmi::WMIConnection;

const NAMESPACE: &str = "ROOT\\WMI";
const CLASS: &str = "AWCCWmiMethodFunction";

const OP_DESCRIBE: u8 = 0x02;
const OP_RESOURCE_ID: u8 = 0x03;
const OP_TEMPERATURE: u8 = 0x04;
const OP_FAN_RPM: u8 = 0x05;
const OP_FAN_PERCENT: u8 = 0x06;
const OP_FAN_MIN_RPM: u8 = 0x08;
const OP_FAN_MAX_RPM: u8 = 0x09;
const OP_BALANCED_PROFILE: u8 = 0x0A;
const OP_CURRENT_PROFILE: u8 = 0x0B;

const OP_ACTIVATE_PROFILE: u8 = 0x01;

const OP_TCC_MAX_TEMPERATURE: u8 = 0x01;
const OP_TCC_MAX_OFFSET: u8 = 0x02;
const OP_TCC_CURRENT_OFFSET: u8 = 0x03;
const OP_TCC_SET_OFFSET: u8 = 0x04;

const UNSUPPORTED: u32 = 0xFFFF_FF00;

pub const FAN_CPU: u8 = 0x32;
pub const FAN_GPU: u8 = 0x33;
pub const SENSOR_CPU: u8 = 0x01;
pub const SENSOR_GPU: u8 = 0x06;

pub const PROFILE_CUSTOM: u8 = 0x00;
pub const PROFILE_QUIET: u8 = 0xA3;
pub const PROFILE_PERFORMANCE: u8 = 0xA1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Profile {
    pub id: u8,
    pub key: &'static str,
    pub label: &'static str,
}

pub const PROFILES: &[Profile] = &[
    Profile {
        id: 0xA2,
        key: "battery",
        label: "Battery",
    },
    Profile {
        id: PROFILE_QUIET,
        key: "quiet",
        label: "Quiet",
    },
    Profile {
        id: 0xA0,
        key: "balanced",
        label: "Balanced",
    },
    Profile {
        id: PROFILE_PERFORMANCE,
        key: "performance",
        label: "Performance",
    },
    Profile {
        id: 0xA4,
        key: "overdrive",
        label: "Overdrive",
    },
    Profile {
        id: PROFILE_CUSTOM,
        key: "custom",
        label: "Custom",
    },
];

pub fn profile_label(id: u8) -> String {
    PROFILES
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.label.to_string())
        .unwrap_or_else(|| format!("Unknown (0x{id:02X})"))
}

pub fn fan_label(id: u8) -> String {
    match id {
        FAN_CPU => "CPU".to_string(),
        FAN_GPU => "GPU".to_string(),
        _ => format!("Fan 0x{id:02X}"),
    }
}

pub fn sensor_label(id: u8) -> String {
    match id {
        SENSOR_CPU => "CPU".to_string(),
        SENSOR_GPU => "GPU".to_string(),
        _ => format!("Sensor 0x{id:02X}"),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FanStatus {
    pub id: u8,
    pub label: String,
    pub rpm: Option<u16>,
    pub percent: Option<u8>,
    pub min_rpm: Option<u16>,
    pub max_rpm: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TempStatus {
    pub id: u8,
    pub label: String,
    pub celsius: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TccStatus {
    pub supported: bool,
    pub max_temperature: Option<i32>,
    pub max_offset: Option<i32>,
    pub offset: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceDescription {
    pub fan_count: u8,
    pub temp_count: u8,
    pub profile_count: u8,
    pub fan_ids: Vec<u8>,
    pub temp_ids: Vec<u8>,
    pub profile_ids: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub device: DeviceDescription,
    pub current_profile: u8,
    pub current_profile_name: String,
    pub balanced_profile: u8,
    pub balanced_profile_name: String,
    pub temps: Vec<TempStatus>,
    pub fans: Vec<FanStatus>,
}

#[derive(Deserialize)]
#[allow(non_camel_case_types)]
struct AWCCWmiMethodFunction {
    #[serde(rename = "__Path")]
    path: String,
    #[serde(rename = "InstanceName")]
    instance_name: String,
    #[serde(rename = "Active")]
    active: bool,
}

#[derive(Serialize)]
struct InArg2 {
    arg2: u32,
}

#[derive(Deserialize)]
struct OutArg {
    argr: u32,
}

pub struct Awcc {
    con: WMIConnection,
    path: String,
    pub instance_name: String,
    pub device: DeviceDescription,
    pub fan_limits: Vec<(u8, Option<u16>, Option<u16>)>,
}

fn encoded(op: u8, arg1: u8, arg2: u8) -> u32 {
    u32::from(op) | (u32::from(arg1) << 8) | (u32::from(arg2) << 16)
}

fn supported(raw: u32) -> Option<u32> {
    (raw < UNSUPPORTED).then_some(raw)
}

fn op_name(op: u8) -> &'static str {
    match op {
        OP_ACTIVATE_PROFILE => "activate-profile",
        OP_DESCRIBE => "describe",
        OP_RESOURCE_ID => "resource-id",
        OP_TEMPERATURE => "temperature",
        OP_FAN_RPM => "fan-rpm",
        OP_FAN_PERCENT => "fan-percent",
        OP_FAN_MIN_RPM => "fan-min-rpm",
        OP_FAN_MAX_RPM => "fan-max-rpm",
        OP_BALANCED_PROFILE => "balanced-profile",
        OP_CURRENT_PROFILE => "current-profile",
        _ => "unknown",
    }
}

fn describe_fields(raw: u32) -> (u8, u8, u8, u8) {
    let bytes = raw.to_le_bytes();
    (bytes[0], bytes[1], bytes[2], bytes[3])
}

impl Awcc {
    pub fn open() -> Result<Self> {
        log::info(format_args!("opening WMI namespace {NAMESPACE}"));
        let con = WMIConnection::with_namespace_path(NAMESPACE)
            .map_err(|error| {
                log::error(format_args!(
                    "WMI namespace {NAMESPACE} connection failed: {error:#}"
                ));
                error
            })
            .with_context(|| format!("cannot connect to WMI namespace {NAMESPACE}"))?;
        log::info(format_args!("WMI namespace {NAMESPACE} connected"));

        let query = format!("SELECT __PATH, InstanceName, Active FROM {CLASS}");
        let instances: Vec<AWCCWmiMethodFunction> = con
            .raw_query(&query)
            .map_err(|error| {
                log::error(format_args!("{CLASS} query failed: {error:#}"));
                error
            })
            .with_context(|| format!("cannot query {CLASS}"))?;
        log::info(format_args!("{CLASS}: {} instance(s)", instances.len()));

        let instance = instances.into_iter().find(|i| i.active).ok_or_else(|| {
            log::error(format_args!("no active {CLASS} instance found"));
            anyhow::anyhow!(
                "no active {CLASS} instance found (is this an Alienware/Dell G-series?)"
            )
        })?;
        log::info(format_args!(
            "active instance: {} path={}",
            instance.instance_name, instance.path
        ));

        let mut awcc = Self {
            con,
            path: instance.path,
            instance_name: instance.instance_name,
            device: DeviceDescription {
                fan_count: 0,
                temp_count: 0,
                profile_count: 0,
                fan_ids: Vec::new(),
                temp_ids: Vec::new(),
                profile_ids: Vec::new(),
            },
            fan_limits: Vec::new(),
        };

        let device = awcc.describe()?;
        let mut fan_limits = Vec::with_capacity(device.fan_ids.len());
        for &id in &device.fan_ids {
            let min = awcc.fan_min_rpm(id).unwrap_or(Some(0));
            let max = awcc.fan_max_rpm(id).unwrap_or(Some(6000));
            fan_limits.push((id, min, max));
        }
        awcc.device = device;
        awcc.fan_limits = fan_limits;

        Ok(awcc)
    }

    fn call(&self, method: &str, arg: u32) -> Result<u32> {
        let op = (arg & 0xFF) as u8;
        let arg1 = ((arg >> 8) & 0xFF) as u8;
        let arg2 = ((arg >> 16) & 0xFF) as u8;
        log::debug(format_args!(
            "{method}: arg=0x{arg:08X} op=0x{op:02X} ({}) arg1=0x{arg1:02X} arg2=0x{arg2:02X}",
            op_name(op)
        ));

        let result: std::result::Result<OutArg, _> =
            self.con.exec_instance_method::<AWCCWmiMethodFunction, _>(
                &self.path,
                method,
                InArg2 { arg2: arg },
            );
        match result {
            Ok(out) => {
                log::debug(format_args!(
                    "{method}: op=0x{op:02X} ({}) -> 0x{:08X}",
                    op_name(op),
                    out.argr
                ));
                Ok(out.argr)
            }
            Err(error) => {
                log::error(format_args!(
                    "{method}: arg=0x{arg:08X} op=0x{op:02X} ({}) failed: {error:#}",
                    op_name(op)
                ));
                Err(error).with_context(|| format!("{method}(0x{arg:08X}) failed"))
            }
        }
    }

    fn thermal_info(&self, op: u8, arg1: u8, arg2: u8) -> Result<Option<u32>> {
        Ok(supported(
            self.call("Thermal_Information", encoded(op, arg1, arg2))?,
        ))
    }

    fn thermal_control(&self, op: u8, arg1: u8, arg2: u8) -> Result<u32> {
        self.call("Thermal_Control", encoded(op, arg1, arg2))
    }

    fn tcc_control(&self, op: u8, arg1: u8) -> Result<u32> {
        self.call("TccControl", encoded(op, arg1, 0))
    }

    fn tcc_value(&self, op: u8) -> Result<Option<i32>> {
        match self.tcc_control(op, 0) {
            Ok(raw) => {
                if raw == u32::MAX {
                    log::debug(format_args!("TccControl op 0x{op:02X} returned -1"));
                    Ok(None)
                } else {
                    Ok(Some(raw as i32))
                }
            }
            Err(error) => {
                log::warn(format_args!(
                    "TccControl op 0x{op:02X} failed: {error:#}"
                ));
                Err(error)
            }
        }
    }

    pub fn tcc(&self) -> Result<TccStatus> {
        let max_temperature = match self.tcc_value(OP_TCC_MAX_TEMPERATURE)? {
            Some(value) if value > 0 => value,
            _ => {
                log::warn(format_args!(
                    "TCC control is not available on this firmware"
                ));
                return Ok(TccStatus {
                    supported: false,
                    max_temperature: None,
                    max_offset: None,
                    offset: None,
                });
            }
        };
        let max_offset = self.tcc_value(OP_TCC_MAX_OFFSET).unwrap_or(None);
        let offset = self.tcc_value(OP_TCC_CURRENT_OFFSET).unwrap_or(None);
        log::info(format_args!(
            "tcc: max_temperature={max_temperature} max_offset={max_offset:?} offset={offset:?}"
        ));
        Ok(TccStatus {
            supported: true,
            max_temperature: Some(max_temperature),
            max_offset,
            offset,
        })
    }

    pub fn set_tcc_offset(&self, offset: i32) -> Result<i32> {
        if !(0..=255).contains(&offset) {
            bail!("TCC offset {offset} is outside the valid range");
        }
        let arg = ((offset as u32) << 8) | u32::from(OP_TCC_SET_OFFSET);
        log::info(format_args!(
            "set_tcc_offset: TccControl(set, {offset}) arg=0x{arg:08X}"
        ));
        let result = self.call("TccControl", arg)?;
        log::info(format_args!("set_tcc_offset: -> 0x{result:08X}"));

        for attempt in 0..6 {
            if matches!(self.tcc_value(OP_TCC_CURRENT_OFFSET), Ok(Some(actual)) if actual == offset) {
                log::info(format_args!("set_tcc_offset: confirmed {offset} (attempt {})", attempt + 1));
                return Ok(offset);
            }
            sleep(Duration::from_millis(60));
        }

        match self.tcc_value(OP_TCC_CURRENT_OFFSET).unwrap_or(None) {
            Some(actual) if actual == offset => Ok(actual),
            Some(actual) => {
                log::warn(format_args!(
                    "set_tcc_offset: set {offset}, readback reported {actual}, accepted"
                ));
                Ok(offset)
            }
            None => Ok(offset),
        }
    }

    pub fn describe(&self) -> Result<DeviceDescription> {
        let raw = self
            .thermal_info(OP_DESCRIBE, 0, 0)?
            .context("device does not report a thermal description")?;
        let (fan_count, temp_count, _misc_count, profile_count) = describe_fields(raw);
        if fan_count > 16 || temp_count > 64 || profile_count > 64 {
            bail!(
                "implausible thermal description 0x{raw:08X} (fans={fan_count}, temps={temp_count}, profiles={profile_count})"
            );
        }

        let mut fan_ids = Vec::new();
        let mut temp_ids = Vec::new();
        let mut profile_ids = Vec::new();
        let misc_count = describe_fields(raw).2;
        let total = u16::from(fan_count)
            + u16::from(temp_count)
            + u16::from(misc_count)
            + u16::from(profile_count);
        for index in 0..total {
            if index > u16::from(u8::MAX) {
                break;
            }
            let Some(raw_id) = self.thermal_info(OP_RESOURCE_ID, index as u8, 0)? else {
                continue;
            };
            let id = (raw_id & 0xFF) as u8;
            if index < u16::from(fan_count) {
                fan_ids.push(id);
            } else if index < u16::from(fan_count) + u16::from(temp_count) {
                temp_ids.push(id);
            } else if index >= u16::from(fan_count) + u16::from(temp_count) + u16::from(misc_count)
            {
                profile_ids.push(id);
            }
        }

        log::debug(format_args!(
            "describe: fans={fan_count} temps={temp_count} profiles={profile_count} fan_ids={fan_ids:?} temp_ids={temp_ids:?} profile_ids={profile_ids:?}"
        ));

        Ok(DeviceDescription {
            fan_count,
            temp_count,
            profile_count,
            fan_ids,
            temp_ids,
            profile_ids,
        })
    }

    pub fn current_profile(&self) -> Result<u8> {
        let raw = self
            .thermal_info(OP_CURRENT_PROFILE, 0, 0)?
            .context("device does not report the current thermal profile")?;
        Ok((raw & 0xFF) as u8)
    }

    pub fn balanced_profile(&self) -> Result<u8> {
        let raw = self
            .thermal_info(OP_BALANCED_PROFILE, 0, 0)?
            .context("device does not report the balanced thermal profile")?;
        Ok((raw & 0xFF) as u8)
    }

    fn temperature(&self, id: u8) -> Result<Option<u8>> {
        Ok(self
            .thermal_info(OP_TEMPERATURE, id, 0)?
            .map(|raw| (raw & 0xFF) as u8))
    }

    fn fan_rpm(&self, id: u8) -> Result<Option<u16>> {
        Ok(self
            .thermal_info(OP_FAN_RPM, id, 0)?
            .map(|raw| (raw & 0xFFFF) as u16))
    }

    #[allow(dead_code)]
    fn fan_percent(&self, id: u8) -> Result<Option<u8>> {
        Ok(self
            .thermal_info(OP_FAN_PERCENT, id, 0)?
            .map(|raw| (raw & 0xFF) as u8))
    }

    fn fan_min_rpm(&self, id: u8) -> Result<Option<u16>> {
        Ok(self
            .thermal_info(OP_FAN_MIN_RPM, id, 0)?
            .map(|raw| (raw & 0xFFFF) as u16))
    }

    fn fan_max_rpm(&self, id: u8) -> Result<Option<u16>> {
        Ok(self
            .thermal_info(OP_FAN_MAX_RPM, id, 0)?
            .map(|raw| (raw & 0xFFFF) as u16))
    }

    pub fn status(&self) -> Result<Status> {
        let current_profile = self.current_profile()?;
        let balanced_profile = self.balanced_profile()?;

        let mut temps = Vec::with_capacity(self.device.temp_ids.len());
        for &id in &self.device.temp_ids {
            temps.push(TempStatus {
                id,
                label: sensor_label(id),
                celsius: self.temperature(id)?,
            });
        }

        let mut fans = Vec::with_capacity(self.device.fan_ids.len());
        for &id in &self.device.fan_ids {
            let limits = self.fan_limits.iter().find(|(fid, _, _)| *fid == id);
            fans.push(FanStatus {
                id,
                label: fan_label(id),
                rpm: self.fan_rpm(id)?,
                percent: None,
                min_rpm: limits.and_then(|(_, min, _)| *min),
                max_rpm: limits.and_then(|(_, _, max)| *max),
            });
        }

        Ok(Status {
            device: self.device.clone(),
            current_profile,
            current_profile_name: profile_label(current_profile),
            balanced_profile,
            balanced_profile_name: profile_label(balanced_profile),
            temps,
            fans,
        })
    }

    pub fn set_profile(&self, profile: &Profile) -> Result<u8> {
        self.set_profile_id(profile.id)
    }

    pub fn set_profile_id(&self, id: u8) -> Result<u8> {
        log::info(format_args!(
            "set_profile_id: requested 0x{id:02X} ({})",
            profile_label(id)
        ));

        if id != PROFILE_CUSTOM {
            log::info(format_args!(
                "set_profile_id: firmware profiles: {}",
                self.device
                    .profile_ids
                    .iter()
                    .map(|id| format!("0x{id:02X}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            if !self.device.profile_ids.contains(&id) {
                log::error(format_args!("set_profile_id: 0x{id:02X} not offered"));
                bail!(
                    "profile 0x{id:02X} is not offered by this firmware; supported: {}",
                    self.device
                        .profile_ids
                        .iter()
                        .map(|id| format!("0x{id:02X}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }

        let control = self.thermal_control(OP_ACTIVATE_PROFILE, id, 0)?;
        log::info(format_args!(
            "set_profile_id: Thermal_Control(activate, 0x{id:02X}) -> 0x{control:08X}"
        ));

        for attempt in 0..6 {
            let actual = self.current_profile()?;
            log::debug(format_args!(
                "set_profile_id: readback attempt {} -> 0x{actual:02X}",
                attempt + 1
            ));
            if actual == id {
                log::info(format_args!("set_profile_id: confirmed 0x{id:02X}"));
                return Ok(id);
            }
            sleep(Duration::from_millis(60));
        }

        let actual = self.current_profile()?;
        if id == PROFILE_CUSTOM || control == 0 {
            log::warn(format_args!(
                "set_profile_id: profile 0x{id:02X} activated (status 0x{control:08X}), firmware reports 0x{actual:02X} ({})",
                profile_label(actual)
            ));
            return Ok(id);
        }
        log::error(format_args!(
            "set_profile_id: sent 0x{id:02X}, firmware reports 0x{actual:02X} ({})",
            profile_label(actual)
        ));
        bail!(
            "profile 0x{id:02X} was sent but firmware still reports 0x{actual:02X} ({})",
            profile_label(actual)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_arguments_in_little_endian_byte_layout() {
        assert_eq!(encoded(0x02, 0, 0), 0x0000_0002);
        assert_eq!(encoded(0x05, 0x32, 0), 0x0000_3205);
        assert_eq!(encoded(0x02, 0x33, 0xF0), 0x00F0_3302);
    }

    #[test]
    fn decodes_device_description() {
        assert_eq!(describe_fields(0x0500_0202), (2, 2, 0, 5));
    }

    #[test]
    fn recognizes_unsupported_sentinels() {
        assert_eq!(supported(0xFFFF_FFFE), None);
        assert_eq!(supported(0xFFFF_FFFF), None);
        assert_eq!(supported(0x0000_0BB8), Some(3000));
    }

    #[test]
    fn labels_known_profiles() {
        assert_eq!(profile_label(0xA0), "Balanced");
        assert_eq!(profile_label(0xA4), "Overdrive");
        assert!(profile_label(0xEE).contains("0xEE"));
    }

    #[test]
    fn maps_awcc_modes_to_firmware_profiles() {
        let labels = PROFILES
            .iter()
            .map(|profile| profile.label)
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec![
                "Battery",
                "Quiet",
                "Balanced",
                "Performance",
                "Overdrive",
                "Custom"
            ]
        );
        let ids = PROFILES.iter().map(|profile| profile.id).collect::<Vec<_>>();
        assert_eq!(ids, vec![0xA2, 0xA3, 0xA0, 0xA1, 0xA4, 0x00]);
    }

    #[test]
    fn encodes_tcc_set_offset_argument() {
        let arg = (7i32 as u32) << 8 | u32::from(OP_TCC_SET_OFFSET);
        assert_eq!(arg, 0x0000_0704);
        assert_eq!(encoded(OP_TCC_SET_OFFSET, 7, 0), 0x0000_0704);
        assert_eq!(encoded(OP_TCC_MAX_TEMPERATURE, 0, 0), 0x0000_0001);
        assert_eq!(encoded(OP_TCC_MAX_OFFSET, 0, 0), 0x0000_0002);
        assert_eq!(encoded(OP_TCC_CURRENT_OFFSET, 0, 0), 0x0000_0003);
    }
}
