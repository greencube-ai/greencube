use std::path::PathBuf;

use sysinfo::System;

use crate::models::{self, ModelEntry};

const BYTES_PER_GIB: u64 = 1024 * 1024 * 1024;

#[cfg(windows)]
#[repr(C)]
struct SystemPowerStatus {
    ac_line_status: u8,
    battery_flag: u8,
    battery_life_percent: u8,
    system_status_flag: u8,
    battery_life_time: u32,
    battery_full_life_time: u32,
}

#[cfg(windows)]
#[link(name = "Kernel32")]
unsafe extern "system" {
    fn GetSystemPowerStatus(system_power_status: *mut SystemPowerStatus) -> i32;
}

/// Everything we know about the hardware, plus which model was selected.
#[derive(Debug, Clone, Copy)]
pub struct HardwareInfo {
    pub total_ram_gb: u64,
    pub cpu_threads: u64,
    pub has_battery: bool,
    pub is_laptop_likely: bool,
    pub on_battery_power: bool,
    pub selected_model: &'static ModelEntry,
}

impl HardwareInfo {
    /// Full path to the selected model's canonical GGUF file.
    /// C:\models is the agreed location for now - Phase 5 will make this configurable.
    pub fn model_path(&self) -> PathBuf {
        models::models_dir().join(self.selected_model.filename)
    }
}

pub fn select_model_for_ram(total_ram_gb: u64) -> &'static ModelEntry {
    models::recommended_model(total_ram_gb)
}

pub fn select_model_for_hardware(
    total_ram_gb: u64,
    cpu_threads: u64,
    is_laptop_likely: bool,
    on_battery_power: bool,
) -> &'static ModelEntry {
    let base = models::recommended_model(total_ram_gb);
    let mut index = models::MODELS
        .iter()
        .position(|model| model.id == base.id)
        .unwrap_or(0);

    if on_battery_power && index > 0 {
        index -= 1;
    } else if is_laptop_likely && cpu_threads <= 8 && index >= 3 {
        index -= 1;
    }

    &models::MODELS[index]
}

fn normalize_total_ram_gb(total_memory_bytes: u64) -> u64 {
    // Round to the nearest GiB so nominal 16 GB / 32 GB machines are not
    // misclassified into the tier below because of firmware or GPU reservations.
    (total_memory_bytes + (BYTES_PER_GIB / 2)) / BYTES_PER_GIB
}

pub fn recommendation_reason(info: &HardwareInfo) -> &'static str {
    if info.on_battery_power {
        "Battery power detected, so GreenCube recommends a lighter model for better responsiveness and thermals."
    } else if info.is_laptop_likely
        && info.cpu_threads <= 8
        && info.selected_model.id == "gemma4-26b-moe"
    {
        "Portable device with limited CPU threads detected, so GreenCube avoids the heaviest default model."
    } else if info.is_laptop_likely {
        "Portable device detected, but it is plugged in, so GreenCube uses the standard recommendation."
    } else {
        "No battery-backed portable profile was detected, so GreenCube uses the standard recommendation."
    }
}

#[cfg(windows)]
fn detect_power_profile() -> (bool, bool) {
    let mut status = SystemPowerStatus {
        ac_line_status: 255,
        battery_flag: 255,
        battery_life_percent: 255,
        system_status_flag: 0,
        battery_life_time: u32::MAX,
        battery_full_life_time: u32::MAX,
    };

    let ok = unsafe { GetSystemPowerStatus(&mut status) } != 0;
    if !ok {
        return (false, false);
    }

    let has_battery = !matches!(status.battery_flag, 128 | 255);
    let on_battery_power = has_battery && status.ac_line_status == 0;
    (has_battery, on_battery_power)
}

#[cfg(not(windows))]
fn detect_power_profile() -> (bool, bool) {
    (false, false)
}

/// Read the local hardware profile and decide which model tier to recommend.
///
/// Base selection starts from RAM, then may downshift on portable devices:
///   - on battery power: drop one tier
///   - on very high-RAM laptops with limited CPU threads: avoid the heaviest tier
pub fn detect() -> HardwareInfo {
    let mut sys = System::new_all();
    sys.refresh_memory();
    let cpu_threads = sys.cpus().len() as u64;

    let total_ram_gb = normalize_total_ram_gb(sys.total_memory());
    let (has_battery, on_battery_power) = detect_power_profile();
    let is_laptop_likely = has_battery;
    let selected_model = select_model_for_hardware(
        total_ram_gb,
        cpu_threads,
        is_laptop_likely,
        on_battery_power,
    );

    HardwareInfo {
        total_ram_gb,
        cpu_threads,
        has_battery,
        is_laptop_likely,
        on_battery_power,
        selected_model,
    }
}

/// Check which model files are actually present on disk, and return the best
/// available one. Always falls back to a smaller model - never a bigger one -
/// so we do not load a model the machine cannot handle.
///
/// Returns (full_path, display_name), or None if no model files are found.
pub fn find_available_model() -> Option<(String, String)> {
    let hw = detect();

    for model in models::fallback_models_from(hw.selected_model) {
        if let Some(path) = models::installed_path(model) {
            return Some((
                path.to_string_lossy().to_string(),
                model.display_name.to_string(),
            ));
        }
    }

    None
}

/// Returns the best available fast model and, if a reasoning pair exists on disk,
/// also the reasoning model.  Both are returned as (path, display_name).
///
/// For 32+ GB machines the pairing is always: Gemma 4 26B MoE (fast) + Gemma 4 31B (reasoning).
/// Both models can run on any 32+ GB machine; the MoE handles quick queries while the 31B
/// handles tasks that need deeper reasoning.  Only one is held in memory at a time.
pub fn find_model_pair() -> (Option<(String, String)>, Option<(String, String)>) {
    let hw = detect();
    let preferred = hw.selected_model;

    // When the selected tier is the dense Gemma 31B, prefer using the 26B MoE as
    // the fast model and keep the 31B as the reasoning upgrade when both are on disk.
    if preferred.id == "gemma4-31b" {
        let moe = models::MODELS.iter().find(|m| m.id == "gemma4-26b-moe");
        let dense = Some(preferred);

        let moe_path = moe.and_then(|m| models::installed_path(m));
        let dense_path = dense.and_then(|m| models::installed_path(m));

        // If the MoE is on disk, use it as the fast model.
        if let (Some(m), Some(path)) = (moe, moe_path) {
            let fast = Some((
                path.to_string_lossy().to_string(),
                m.display_name.to_string(),
            ));
            let reasoning = if let (Some(d), Some(dpath)) = (dense, dense_path) {
                Some((
                    dpath.to_string_lossy().to_string(),
                    d.display_name.to_string(),
                ))
            } else {
                None
            };
            return (fast, reasoning);
        }

        // MoE not on disk yet — fall back to just the 31B with no reasoning upgrade.
        if let (Some(d), Some(dpath)) = (dense, dense_path) {
            return (
                Some((
                    dpath.to_string_lossy().to_string(),
                    d.display_name.to_string(),
                )),
                None,
            );
        }
    }

    let fast_entry = models::fallback_models_from(preferred)
        .into_iter()
        .find(|m| models::installed_path(m).is_some());

    let fast = fast_entry.and_then(|m| {
        models::installed_path(m)
            .map(|p| (p.to_string_lossy().to_string(), m.display_name.to_string()))
    });

    let reasoning = fast_entry
        .and_then(models::find_reasoning_pair)
        .and_then(|rm| {
            models::installed_path(rm)
                .map(|p| (p.to_string_lossy().to_string(), rm.display_name.to_string()))
        });

    (fast, reasoning)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_hardware() {
        let info = detect();

        println!("\n--- Hardware Detection ---");
        println!("Total RAM : {} GB", info.total_ram_gb);
        println!("CPU threads: {}", info.cpu_threads);
        println!("Battery   : {}", info.has_battery);
        println!("On battery: {}", info.on_battery_power);
        println!("Model     : {}", info.selected_model.display_name);
        println!("File      : {}", info.selected_model.filename);
        println!("Path      : {}", info.model_path().display());
        println!("--------------------------");

        // RAM should be a sane value (more than 0, less than 10TB)
        assert!(info.total_ram_gb > 0, "RAM should be detectable");
        assert!(info.total_ram_gb < 10_000, "RAM value looks wrong");
    }

    #[test]
    fn test_select_model_for_ram_uses_expected_tiers() {
        assert_eq!(select_model_for_ram(8).id, "phi4-mini");
        assert_eq!(select_model_for_ram(13).id, "phi4-mini");
        assert_eq!(select_model_for_ram(14).id, "phi4-mini");
        assert_eq!(select_model_for_ram(15).id, "phi4-mini");
        assert_eq!(select_model_for_ram(16).id, "qwen3-14b");
        assert_eq!(select_model_for_ram(32).id, "gemma4-26b-moe");
        assert_eq!(select_model_for_ram(39).id, "gemma4-26b-moe");
        assert_eq!(select_model_for_ram(40).id, "gemma4-31b");
        assert_eq!(select_model_for_ram(64).id, "gemma4-31b");
    }

    #[test]
    fn test_select_model_for_hardware_downshifts_on_battery() {
        assert_eq!(
            select_model_for_hardware(16, 12, true, true).id,
            "phi4-mini"
        );
        assert_eq!(
            select_model_for_hardware(32, 12, true, true).id,
            "qwen3-14b"
        );
        assert_eq!(
            select_model_for_hardware(40, 12, true, true).id,
            "gemma4-26b-moe"
        );
    }

    #[test]
    fn test_select_model_for_hardware_avoids_heaviest_model_on_thin_laptops() {
        assert_eq!(
            select_model_for_hardware(64, 8, true, false).id,
            "gemma4-26b-moe"
        );
        assert_eq!(
            select_model_for_hardware(64, 16, true, false).id,
            "gemma4-31b"
        );
    }

    #[test]
    fn test_normalize_total_ram_gb_rounds_nominal_ram_sizes() {
        assert_eq!(normalize_total_ram_gb(14 * BYTES_PER_GIB), 14);
        assert_eq!(
            normalize_total_ram_gb((15 * BYTES_PER_GIB) + (800 * 1024 * 1024)),
            16
        );
        assert_eq!(
            normalize_total_ram_gb((31 * BYTES_PER_GIB) + (700 * 1024 * 1024)),
            32
        );
    }
}
