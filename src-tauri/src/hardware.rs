//! What the machine offers for inference, as seen by whisper.cpp (ggml):
//! the GPUs it can use (Vulkan, Metal) and the CPU. Drives the model
//! recommendation and tells whether a "GPU" load really runs on a GPU.

use std::ffi::CStr;
use std::sync::OnceLock;

use serde::Serialize;

use crate::models::manifest::HardwareClass;

#[derive(Debug, Clone, Serialize)]
pub struct Gpu {
    pub name: String,
    /// 0 when the backend does not report it.
    pub vram_mb: u64,
    /// Shares the system memory (laptop chips): slower than a graphics card.
    pub integrated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hardware {
    pub gpus: Vec<Gpu>,
    pub cpu_threads: u32,
    pub class: HardwareClass,
}

impl Hardware {
    /// Index in `gpus`, i.e. whisper.cpp's `gpu_device`, of the fastest GPU:
    /// a graphics card before an integrated chip, then the most memory.
    /// Laptops often list the integrated chip first.
    pub fn best_gpu(&self) -> Option<usize> {
        best(&self.gpus)
    }
}

fn best(gpus: &[Gpu]) -> Option<usize> {
    gpus.iter()
        .enumerate()
        .max_by_key(|(i, g)| (!g.integrated, g.vram_mb, std::cmp::Reverse(*i)))
        .map(|(i, _)| i)
}

/// A graphics card with this much memory runs the large models.
const DEDICATED_MIN_VRAM_MB: u64 = 2_000;
/// Below this many threads, only the small models are fast enough.
const STRONG_CPU_THREADS: u32 = 8;

/// Detected once: enumerating the devices initialises the Vulkan instance.
pub fn detect() -> &'static Hardware {
    static HARDWARE: OnceLock<Hardware> = OnceLock::new();
    HARDWARE.get_or_init(|| {
        let gpus = gpus();
        let cpu_threads = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
        let class = classify(&gpus, cpu_threads);
        let hw = Hardware {
            gpus,
            cpu_threads,
            class,
        };
        tracing::info!(?hw, "hardware detected");
        hw
    })
}

fn classify(gpus: &[Gpu], cpu_threads: u32) -> HardwareClass {
    let gpu = best(gpus).and_then(|i| gpus.get(i));
    if gpu.is_some_and(|g| !g.integrated && (g.vram_mb == 0 || g.vram_mb >= DEDICATED_MIN_VRAM_MB))
    {
        HardwareClass::Gpu
    } else if gpu.is_some() || cpu_threads >= STRONG_CPU_THREADS {
        HardwareClass::CpuStrong
    } else {
        HardwareClass::CpuWeak
    }
}

fn gpus() -> Vec<Gpu> {
    use whisper_rs_sys::{
        ggml_backend_dev_count, ggml_backend_dev_get, ggml_backend_dev_get_props,
        ggml_backend_dev_props, ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_GPU as GPU,
        ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_IGPU as IGPU,
    };

    let mut out = Vec::new();
    // SAFETY: ggml owns the device registry for the whole process; devices
    // are never freed. `props` is plain data filled in by ggml, and its
    // strings point into the device, valid as long as it.
    unsafe {
        for i in 0..ggml_backend_dev_count() {
            let dev = ggml_backend_dev_get(i);
            if dev.is_null() {
                continue;
            }
            let mut props: ggml_backend_dev_props = std::mem::zeroed();
            ggml_backend_dev_get_props(dev, &mut props);
            if props.type_ != GPU && props.type_ != IGPU {
                continue;
            }
            let text = |p: *const std::os::raw::c_char| {
                if p.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(p).to_string_lossy().into_owned()
                }
            };
            let description = text(props.description);
            out.push(Gpu {
                name: if description.is_empty() {
                    text(props.name)
                } else {
                    description
                },
                vram_mb: (props.memory_total / 1_000_000) as u64,
                integrated: props.type_ == IGPU,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(vram_mb: u64, integrated: bool) -> Gpu {
        Gpu {
            name: "test".into(),
            vram_mb,
            integrated,
        }
    }

    #[test]
    fn graphics_card_gets_the_large_models() {
        assert_eq!(classify(&[gpu(8_000, false)], 4), HardwareClass::Gpu);
        // Metal and some drivers report no memory size.
        assert_eq!(classify(&[gpu(0, false)], 4), HardwareClass::Gpu);
    }

    #[test]
    fn small_or_integrated_gpu_gets_the_mid_models() {
        assert_eq!(classify(&[gpu(1_000, false)], 4), HardwareClass::CpuStrong);
        assert_eq!(classify(&[gpu(16_000, true)], 4), HardwareClass::CpuStrong);
    }

    #[test]
    fn cpu_only_depends_on_the_threads() {
        assert_eq!(classify(&[], 16), HardwareClass::CpuStrong);
        assert_eq!(classify(&[], 4), HardwareClass::CpuWeak);
    }

    #[test]
    fn best_gpu_is_the_graphics_card() {
        let laptop = [gpu(0, true), gpu(4_000, false), gpu(8_000, false)];
        assert_eq!(best(&laptop), Some(2));
        assert_eq!(best(&[gpu(0, true)]), Some(0));
        assert_eq!(best(&[]), None);
        // Same card twice: the first one, as whisper.cpp would pick.
        assert_eq!(best(&[gpu(8_000, false), gpu(8_000, false)]), Some(0));
    }

    #[test]
    fn detection_does_not_crash() {
        let hw = detect();
        assert!(hw.cpu_threads >= 1);
    }
}
