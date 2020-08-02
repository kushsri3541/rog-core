// Return show-stopping errors, otherwise map error to a log level

use crate::{config::Config, error::RogError};
use log::{error, info, warn};
use std::error::Error;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::str::FromStr;

static FAN_TYPE_1_PATH: &str = "/sys/devices/platform/asus-nb-wmi/throttle_thermal_policy";
static FAN_TYPE_2_PATH: &str = "/sys/devices/platform/asus-nb-wmi/fan_boost_mode";
static AMD_BOOST_PATH: &str = "/sys/devices/system/cpu/cpufreq/boost";
static BAT_CHARGE_PATH: &str = "/sys/class/power_supply/BAT0/charge_control_end_threshold";

/// ROG device controller
///
/// For the GX502GW the LED setup sequence looks like:
///
/// -` LED_INIT1`
/// - `LED_INIT3`
/// - `LED_INIT4`
/// - `LED_INIT2`
/// - `LED_INIT4`
pub struct RogCore {}

impl RogCore {
    pub fn new(vendor: u16, product: u16) -> Self {
        RogCore {}
    }

    fn get_fan_path() -> Result<&'static str, std::io::Error> {
        if Path::new(FAN_TYPE_1_PATH).exists() {
            Ok(FAN_TYPE_1_PATH)
        } else if Path::new(FAN_TYPE_2_PATH).exists() {
            Ok(FAN_TYPE_2_PATH)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Fan mode not available",
            ))
        }
    }

    pub fn fan_mode_reload(&mut self, config: &mut Config) -> Result<(), Box<dyn Error>> {
        let path = RogCore::get_fan_path()?;
        let mut file = OpenOptions::new().write(true).open(path)?;
        file.write_all(format!("{:?}\n", config.fan_mode).as_bytes())
            .unwrap_or_else(|err| error!("Could not write to {}, {:?}", path, err));
        self.set_pstate_for_fan_mode(FanLevel::from(config.fan_mode), config)?;
        info!("Reloaded fan mode: {:?}", FanLevel::from(config.fan_mode));
        Ok(())
    }

    pub fn set_fan_mode(&mut self, n: u8, config: &mut Config) -> Result<(), Box<dyn Error>> {
        let path = RogCore::get_fan_path()?;
        let mut fan_ctrl = OpenOptions::new().read(true).write(true).open(path)?;

        config.fan_mode = n;
        config.write();
        fan_ctrl
            .write_all(format!("{:?}\n", config.fan_mode).as_bytes())
            .unwrap_or_else(|err| error!("Could not write to {}, {:?}", path, err));
        info!("Fan mode set to: {:?}", FanLevel::from(config.fan_mode));
        self.set_pstate_for_fan_mode(FanLevel::from(n), config)?;
        Ok(())
    }

    pub fn fan_mode_step(&mut self, config: &mut Config) -> Result<(), Box<dyn Error>> {
        // re-read the config here in case a user changed the pstate settings
        config.read();

        let mut n = config.fan_mode;
        // wrap around the step number
        if n < 2 {
            n += 1;
        } else {
            n = 0;
        }
        self.set_fan_mode(n, config)
    }

    fn set_pstate_for_fan_mode(
        &self,
        mode: FanLevel,
        config: &mut Config,
    ) -> Result<(), Box<dyn Error>> {
        // Set CPU pstate
        if let Ok(pstate) = intel_pstate::PState::new() {
            match mode {
                FanLevel::Normal => {
                    pstate.set_min_perf_pct(config.mode_performance.normal.min_percentage)?;
                    pstate.set_max_perf_pct(config.mode_performance.normal.max_percentage)?;
                    pstate.set_no_turbo(config.mode_performance.normal.no_turbo)?;
                    info!(
                        "Intel CPU Power: min: {:?}%, max: {:?}%, turbo: {:?}",
                        config.mode_performance.normal.min_percentage,
                        config.mode_performance.normal.max_percentage,
                        !config.mode_performance.normal.no_turbo
                    );
                }
                FanLevel::Boost => {
                    pstate.set_min_perf_pct(config.mode_performance.boost.min_percentage)?;
                    pstate.set_max_perf_pct(config.mode_performance.boost.max_percentage)?;
                    pstate.set_no_turbo(config.mode_performance.boost.no_turbo)?;
                    info!(
                        "Intel CPU Power: min: {:?}%, max: {:?}%, turbo: {:?}",
                        config.mode_performance.boost.min_percentage,
                        config.mode_performance.boost.max_percentage,
                        !config.mode_performance.boost.no_turbo
                    );
                }
                FanLevel::Silent => {
                    pstate.set_min_perf_pct(config.mode_performance.silent.min_percentage)?;
                    pstate.set_max_perf_pct(config.mode_performance.silent.max_percentage)?;
                    pstate.set_no_turbo(config.mode_performance.silent.no_turbo)?;
                    info!(
                        "Intel CPU Power: min: {:?}%, max: {:?}%, turbo: {:?}",
                        config.mode_performance.silent.min_percentage,
                        config.mode_performance.silent.max_percentage,
                        !config.mode_performance.silent.no_turbo
                    );
                }
            }
        } else {
            info!("Setting pstate for AMD CPU");
            // must be AMD CPU
            let mut file = OpenOptions::new()
                .write(true)
                .open(AMD_BOOST_PATH)
                .map_err(|err| {
                    warn!("Failed to open AMD boost: {:?}", err);
                    err
                })?;
            match mode {
                FanLevel::Normal => {
                    let boost = if config.mode_performance.normal.no_turbo {
                        "0"
                    } else {
                        "1"
                    }; // opposite of Intel
                    file.write_all(boost.as_bytes()).unwrap_or_else(|err| {
                        error!("Could not write to {}, {:?}", AMD_BOOST_PATH, err)
                    });
                    info!("AMD CPU Turbo: {:?}", boost);
                }
                FanLevel::Boost => {
                    let boost = if config.mode_performance.boost.no_turbo {
                        "0"
                    } else {
                        "1"
                    };
                    file.write_all(boost.as_bytes()).unwrap_or_else(|err| {
                        error!("Could not write to {}, {:?}", AMD_BOOST_PATH, err)
                    });
                    info!("AMD CPU Turbo: {:?}", boost);
                }
                FanLevel::Silent => {
                    let boost = if config.mode_performance.silent.no_turbo {
                        "0"
                    } else {
                        "1"
                    };
                    file.write_all(boost.as_bytes()).unwrap_or_else(|err| {
                        error!("Could not write to {}, {:?}", AMD_BOOST_PATH, err)
                    });
                    info!("AMD CPU Turbo: {:?}", boost);
                }
            }
        }
        Ok(())
    }

    pub fn bat_charge_limit_reload(&self, config: &mut Config) -> Result<(), Box<dyn Error>> {
        config.read();
        info!("Reloaded battery charge limit");
        self.set_charge_limit(config.bat_charge_limit, config)
    }

    pub fn set_charge_limit(&self, limit: u8, config: &mut Config) -> Result<(), Box<dyn Error>> {
        if limit < 20 || limit > 100 {
            warn!(
                "Unable to set battery charge limit, must be between 20-100: requested {}",
                limit
            );
        }

        let mut file = OpenOptions::new()
            .write(true)
            .open(BAT_CHARGE_PATH)
            .map_err(|err| {
                warn!("Failed to open battery charge limit path: {:?}", err);
                err
            })?;
        file.write_all(limit.to_string().as_bytes())
            .unwrap_or_else(|err| error!("Could not write to {}, {:?}", BAT_CHARGE_PATH, err));
        info!("Battery charge limit: {}", limit);

        config.bat_charge_limit = limit;
        config.write();

        Ok(())
    }

    /// A direct call to systemd to suspend the PC.
    ///
    /// This avoids desktop environments being required to handle it
    /// (which means it works while in a TTY also)
    pub fn suspend_with_systemd(&self) {
        std::process::Command::new("systemctl")
            .arg("suspend")
            .spawn()
            .map_or_else(|err| warn!("Failed to suspend: {}", err), |_| {});
    }

    /// A direct call to rfkill to suspend wireless devices.
    ///
    /// This avoids desktop environments being required to handle it (which
    /// means it works while in a TTY also)
    pub fn toggle_airplane_mode(&self) {
        match Command::new("rfkill").arg("list").output() {
            Ok(output) => {
                if output.status.success() {
                    if let Ok(out) = String::from_utf8(output.stdout) {
                        if out.contains(": yes") {
                            Command::new("rfkill")
                                .arg("unblock")
                                .arg("all")
                                .spawn()
                                .map_or_else(
                                    |err| warn!("Could not unblock rf devices: {}", err),
                                    |_| {},
                                );
                        } else {
                            Command::new("rfkill")
                                .arg("block")
                                .arg("all")
                                .spawn()
                                .map_or_else(
                                    |err| warn!("Could not block rf devices: {}", err),
                                    |_| {},
                                );
                        }
                    }
                } else {
                    warn!("Could not list rf devices");
                }
            }
            Err(err) => {
                warn!("Could not list rf devices: {}", err);
            }
        }
    }
}

#[derive(Debug)]
pub enum FanLevel {
    Normal,
    Boost,
    Silent,
}

impl FromStr for FanLevel {
    type Err = RogError;

    fn from_str(s: &str) -> Result<Self, RogError> {
        match s.to_lowercase().as_str() {
            "normal" => Ok(FanLevel::Normal),
            "boost" => Ok(FanLevel::Boost),
            "silent" => Ok(FanLevel::Silent),
            _ => Err(RogError::ParseFanLevel),
        }
    }
}

impl From<u8> for FanLevel {
    fn from(n: u8) -> Self {
        match n {
            0 => FanLevel::Normal,
            1 => FanLevel::Boost,
            2 => FanLevel::Silent,
            _ => FanLevel::Normal,
        }
    }
}

impl From<FanLevel> for u8 {
    fn from(n: FanLevel) -> Self {
        match n {
            FanLevel::Normal => 0,
            FanLevel::Boost => 1,
            FanLevel::Silent => 2,
        }
    }
}
