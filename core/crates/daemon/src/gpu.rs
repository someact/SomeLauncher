//! Hybrid-GPU detection for PRIME offload: a laptop whose display runs on the
//! integrated GPU and that has a discrete one. Desktops (the display on the
//! discrete GPU) and single-GPU machines need nothing.

use std::path::Path;

const NVIDIA: &str = "0x10de";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offload {
    /// NVIDIA's PRIME render offload (what `prime-run` sets)
    Nvidia,
    /// Mesa drivers (AMD, Intel): `DRI_PRIME=1`
    Mesa,
}

impl Offload {
    pub fn env(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Offload::Nvidia => &[
                ("__NV_PRIME_RENDER_OFFLOAD", "1"),
                ("__GLX_VENDOR_LIBRARY_NAME", "nvidia"),
                ("__VK_LAYER_NV_optimus", "NVIDIA_only"),
            ],
            Offload::Mesa => &[("DRI_PRIME", "1")],
        }
    }
}

/// From `/sys/class/drm` (a parameter for tests).
pub fn offload(drm: &Path) -> Option<Offload> {
    let mut gpus: Vec<(String, bool, std::path::PathBuf)> = Vec::new();
    for e in std::fs::read_dir(drm).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.strip_prefix("card").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) {
            continue;
        }
        let dev = e.path().join("device");
        let Ok(canon) = dev.canonicalize() else { continue };
        if gpus.iter().any(|g| g.2 == canon) {
            continue;
        }
        let read = |f: &str| std::fs::read_to_string(dev.join(f)).map(|s| s.trim().to_string()).unwrap_or_default();
        gpus.push((read("vendor"), read("boot_vga") == "1", canon));
    }
    let boot = gpus.iter().position(|g| g.1).unwrap_or(0);
    let display = gpus.get(boot)?;
    if display.0 == NVIDIA {
        return None;
    }
    let other = gpus.iter().enumerate().find(|(i, _)| *i != boot).map(|(_, g)| g)?;
    Some(if other.0 == NVIDIA { Offload::Nvidia } else { Offload::Mesa })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(drm: &Path, name: &str, vendor: &str, boot: bool) {
        let dev = drm.join(format!("devices/{name}"));
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::write(dev.join("vendor"), format!("{vendor}\n")).unwrap();
        std::fs::write(dev.join("boot_vga"), if boot { "1\n" } else { "0\n" }).unwrap();
        std::fs::create_dir_all(drm.join(name)).unwrap();
        std::os::unix::fs::symlink(&dev, drm.join(name).join("device")).unwrap();
    }

    #[test]
    fn only_laptops_with_the_display_on_the_igpu_offload() {
        let t = tempfile::tempdir().unwrap();
        let desktop = t.path().join("desktop");
        card(&desktop, "card1", NVIDIA, true);
        assert_eq!(offload(&desktop), None);

        let intel_nvidia = t.path().join("laptop1");
        card(&intel_nvidia, "card0", "0x8086", true);
        card(&intel_nvidia, "card1", NVIDIA, false);
        std::fs::create_dir_all(intel_nvidia.join("card0-eDP-1")).unwrap();
        assert_eq!(offload(&intel_nvidia), Some(Offload::Nvidia));

        let amd_amd = t.path().join("laptop2");
        card(&amd_amd, "card0", "0x1002", true);
        card(&amd_amd, "card1", "0x1002", false);
        assert_eq!(offload(&amd_amd), Some(Offload::Mesa));

        let both_on_desktop = t.path().join("desktop2");
        card(&both_on_desktop, "card0", "0x8086", false);
        card(&both_on_desktop, "card1", NVIDIA, true);
        assert_eq!(offload(&both_on_desktop), None);
        assert_eq!(offload(&t.path().join("none")), None);
    }
}
