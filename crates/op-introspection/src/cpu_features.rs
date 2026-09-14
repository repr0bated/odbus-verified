// CPU feature detection and BIOS lock analysis
// Detects hidden/disabled CPU features that could be unlocked

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::process::Command;

/// Type alias for complex feature check results
type FeatureCheckResult = Result<Option<(CpuFeature, Option<BiosLock>, Option<Recommendation>)>>;

/// CPU feature analysis report
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CpuFeatureAnalysis {
    /// CPU model information
    pub cpu_model: CpuModel,

    /// Features present in CPU but potentially disabled
    pub features: Vec<CpuFeature>,

    /// BIOS locks detected
    pub bios_locks: Vec<BiosLock>,

    /// Recommendations for enabling hidden features
    pub recommendations: Vec<Recommendation>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CpuModel {
    pub vendor: String,     // "Intel", "AMD"
    pub family: String,     // CPU family
    pub model_name: String, // Full model string
    pub stepping: u32,
    pub microcode: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CpuFeature {
    pub name: String,           // "VT-x", "AMD-V", "VT-d", "SGX", etc.
    pub technical_name: String, // "vmx", "svm", "iommu", "sgx"
    pub category: FeatureCategory,
    pub status: FeatureStatus,
    pub bios_locked: bool, // True if BIOS prevents enabling
    pub unlock_method: Option<UnlockMethod>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum FeatureCategory {
    Virtualization,  // VT-x, AMD-V, VT-d
    Security,        // SGX, TXT, SME, SEV
    Performance,     // Turbo Boost, SpeedStep
    PowerManagement, // C-states, P-states
    Debugging,       // Performance counters, debug registers
    Other,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum FeatureStatus {
    /// Feature is enabled and working
    Enabled,

    /// Feature supported by CPU but disabled
    DisabledByBios,

    /// Feature supported but blocked by BIOS lock
    LockedByBios,

    /// Feature not supported by CPU
    NotSupported,

    /// Feature partially enabled (some aspects locked)
    PartiallyEnabled,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BiosLock {
    pub register: String, // "MSR 0x3A", "CPUID leaf 0x7"
    pub lock_bit: String, // "Bit 0 (Lock)"
    pub affected_features: Vec<String>,
    pub locked: bool,
    pub lock_method: String, // "MSR lock bit", "BIOS setting", "Vendor fuse"
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UnlockMethod {
    pub method: String,
    pub risk_level: RiskLevel,
    pub commands: Vec<String>,
    pub description: String,
    pub requires_reboot: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum RiskLevel {
    Safe,         // No risk, reversible
    Low,          // Minimal risk, easily reversible
    Medium,       // Some risk, may cause instability
    High,         // Significant risk, may brick BIOS
    VendorLocked, // Cannot be unlocked (hardware fuse)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Recommendation {
    pub priority: Priority,
    pub feature: String,
    pub reason: String,
    pub benefit: String,
    pub action: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Critical, // Essential for operation (e.g., VT-x for virtualization)
    High,     // Significant benefit
    Medium,   // Nice to have
    Low,      // Minor improvement
}

/// Analyzer for CPU features and BIOS locks
pub struct CpuFeatureAnalyzer;

impl Default for CpuFeatureAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuFeatureAnalyzer {
    pub fn new() -> Self {
        Self
    }

    /// Analyze CPU features and BIOS locks
    pub fn analyze(&self) -> Result<CpuFeatureAnalysis> {
        let cpu_model = self.detect_cpu_model()?;
        let cpuinfo_flags = self.read_cpuinfo_flags()?;
        let msr_available = self.check_msr_available();

        let mut features = Vec::new();
        let mut bios_locks = Vec::new();
        let mut recommendations = Vec::new();

        // Check virtualization features (VT-x/AMD-V)
        let virt_feature = self.check_virtualization(&cpu_model, &cpuinfo_flags, msr_available)?;
        if let Some((feature, lock, rec)) = virt_feature {
            features.push(feature.clone());
            if let Some(l) = lock {
                bios_locks.push(l);
            }
            if let Some(r) = rec {
                recommendations.push(r);
            }
        }

        // Check IOMMU (VT-d/AMD-Vi)
        let iommu_feature = self.check_iommu(&cpuinfo_flags)?;
        if let Some((feature, rec)) = iommu_feature {
            features.push(feature);
            if let Some(r) = rec {
                recommendations.push(r);
            }
        }

        // Check Intel SGX (Software Guard Extensions)
        if cpu_model.vendor == "Intel" {
            if let Some((feature, lock, rec)) = self.check_sgx(&cpuinfo_flags, msr_available)? {
                features.push(feature);
                if let Some(l) = lock {
                    bios_locks.push(l);
                }
                if let Some(r) = rec {
                    recommendations.push(r);
                }
            }
        }

        // Check Turbo Boost / Precision Boost
        let turbo_feature = self.check_turbo(&cpu_model, msr_available)?;
        if let Some((feature, rec)) = turbo_feature {
            features.push(feature);
            if let Some(r) = rec {
                recommendations.push(r);
            }
        }

        // Check AMD SME/SEV (Secure Memory Encryption)
        if cpu_model.vendor == "AMD" {
            if let Some((feature, rec)) = self.check_amd_encryption(&cpuinfo_flags)? {
                features.push(feature);
                if let Some(r) = rec {
                    recommendations.push(r);
                }
            }
        }

        // Sort recommendations by priority
        recommendations.sort_by_key(|r| r.priority.clone());

        Ok(CpuFeatureAnalysis {
            cpu_model,
            features,
            bios_locks,
            recommendations,
        })
    }

    /// Detect CPU model information
    fn detect_cpu_model(&self) -> Result<CpuModel> {
        let cpuinfo =
            fs::read_to_string("/proc/cpuinfo").context("Failed to read /proc/cpuinfo")?;

        let mut vendor = "Unknown".to_string();
        let mut model_name = "Unknown".to_string();
        let mut family = "Unknown".to_string();
        let mut stepping = 0u32;
        let mut microcode = "Unknown".to_string();

        for line in cpuinfo.lines() {
            if line.starts_with("vendor_id") {
                vendor = line
                    .split(':')
                    .nth(1)
                    .unwrap_or("Unknown")
                    .trim()
                    .to_string();
                // Normalize vendor names
                if vendor.contains("Intel") {
                    vendor = "Intel".to_string();
                } else if vendor.contains("AMD") {
                    vendor = "AMD".to_string();
                }
            } else if line.starts_with("model name") {
                model_name = line
                    .split(':')
                    .nth(1)
                    .unwrap_or("Unknown")
                    .trim()
                    .to_string();
            } else if line.starts_with("cpu family") {
                family = line
                    .split(':')
                    .nth(1)
                    .unwrap_or("Unknown")
                    .trim()
                    .to_string();
            } else if line.starts_with("stepping") {
                if let Ok(s) = line.split(':').nth(1).unwrap_or("0").trim().parse() {
                    stepping = s;
                }
            } else if line.starts_with("microcode") {
                microcode = line
                    .split(':')
                    .nth(1)
                    .unwrap_or("Unknown")
                    .trim()
                    .to_string();
            }
        }

        Ok(CpuModel {
            vendor,
            family,
            model_name,
            stepping,
            microcode,
        })
    }

    /// Read CPU flags from /proc/cpuinfo
    fn read_cpuinfo_flags(&self) -> Result<Vec<String>> {
        let cpuinfo =
            fs::read_to_string("/proc/cpuinfo").context("Failed to read /proc/cpuinfo")?;

        for line in cpuinfo.lines() {
            if line.starts_with("flags") || line.starts_with("Features") {
                let flags: Vec<String> = line
                    .split(':')
                    .nth(1)
                    .unwrap_or("")
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
                return Ok(flags);
            }
        }

        Ok(Vec::new())
    }

    /// Check if MSR (Model Specific Register) access is available
    fn check_msr_available(&self) -> bool {
        std::path::Path::new("/dev/cpu/0/msr").exists()
            || Command::new("modprobe").arg("msr").output().is_ok()
    }

    /// Check virtualization support (VT-x/AMD-V)
    fn check_virtualization(
        &self,
        cpu_model: &CpuModel,
        flags: &[String],
        msr_available: bool,
    ) -> FeatureCheckResult {
        let (feature_flag, feature_name, technical_name) = if cpu_model.vendor == "Intel" {
            ("vmx", "VT-x (Intel Virtualization)", "vmx")
        } else if cpu_model.vendor == "AMD" {
            ("svm", "AMD-V (AMD Virtualization)", "svm")
        } else {
            return Ok(None);
        };

        let cpu_supports = flags.contains(&feature_flag.to_string());

        if !cpu_supports {
            return Ok(Some((
                CpuFeature {
                    name: feature_name.to_string(),
                    technical_name: technical_name.to_string(),
                    category: FeatureCategory::Virtualization,
                    status: FeatureStatus::NotSupported,
                    bios_locked: false,
                    unlock_method: None,
                },
                None,
                None,
            )));
        }

        // Check if actually enabled (can we use KVM?)
        let kvm_enabled = std::path::Path::new("/dev/kvm").exists();

        let (status, bios_lock, recommendation) = if !kvm_enabled
            && cpu_model.vendor == "Intel"
            && msr_available
        {
            // CPU supports VT-x but /dev/kvm doesn't exist
            // Check MSR 0x3A (IA32_FEATURE_CONTROL) to see if BIOS locked it
            let msr_lock_status = self.check_intel_vmx_lock()?;

            match msr_lock_status {
                VmxLockStatus::Locked => {
                    (
                        FeatureStatus::LockedByBios,
                        Some(BiosLock {
                            register: "MSR 0x3A (IA32_FEATURE_CONTROL)".to_string(),
                            lock_bit: "Bit 0 (Lock), Bit 2 (VMX Enable)".to_string(),
                            affected_features: vec!["VT-x".to_string(), "KVM".to_string()],
                            locked: true,
                            lock_method: "BIOS MSR lock bit set before OS boot".to_string(),
                        }),
                        Some(Recommendation {
                            priority: Priority::Critical,
                            feature: "VT-x".to_string(),
                            reason: "CPU supports VT-x but BIOS has locked it via MSR".to_string(),
                            benefit: "Enable KVM virtualization, Docker, QEMU, VirtualBox with hardware acceleration".to_string(),
                            action: "BIOS Update: Check for BIOS update that exposes VT-x option, or use BIOS modification tools (advanced)".to_string(),
                        })
                    )
                },
                VmxLockStatus::DisabledUnlocked => {
                    (
                        FeatureStatus::DisabledByBios,
                        None,
                        Some(Recommendation {
                            priority: Priority::Critical,
                            feature: "VT-x".to_string(),
                            reason: "CPU supports VT-x but it is disabled (BIOS not locked)".to_string(),
                            benefit: "Enable KVM virtualization for Docker, QEMU, VirtualBox".to_string(),
                            action: "Can be enabled via MSR write: modprobe msr && wrmsr 0x3A 0x5".to_string(),
                        })
                    )
                },
                VmxLockStatus::EnabledLocked => {
                    // This shouldn't happen if /dev/kvm doesn't exist, but handle it
                    (FeatureStatus::Enabled, None, None)
                },
            }
        } else if !kvm_enabled {
            // AMD or MSR not available - just report disabled
            (
                FeatureStatus::DisabledByBios,
                None,
                Some(Recommendation {
                    priority: Priority::Critical,
                    feature: feature_name.to_string(),
                    reason: format!("CPU supports {} but /dev/kvm is not available", feature_name),
                    benefit: "Enable virtualization for KVM, Docker, QEMU".to_string(),
                    action: "Enter BIOS/UEFI setup and enable virtualization (usually under CPU or Advanced settings)".to_string(),
                })
            )
        } else {
            (FeatureStatus::Enabled, None, None)
        };

        Ok(Some((
            CpuFeature {
                name: feature_name.to_string(),
                technical_name: technical_name.to_string(),
                category: FeatureCategory::Virtualization,
                status,
                bios_locked: bios_lock.is_some(),
                unlock_method: self.create_vmx_unlock_method(&cpu_model.vendor),
            },
            bios_lock,
            recommendation,
        )))
    }

    /// Check Intel VT-x lock status via MSR
    fn check_intel_vmx_lock(&self) -> Result<VmxLockStatus> {
        // Try to read MSR 0x3A (IA32_FEATURE_CONTROL)
        let output = Command::new("rdmsr").arg("0x3A").output();

        if let Ok(out) = output {
            if out.status.success() {
                let value_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if let Ok(value) = u64::from_str_radix(&value_str, 16) {
                    let lock_bit = value & 0x1; // Bit 0: Lock
                    let vmx_enable = value & 0x4; // Bit 2: VMX Enable

                    if lock_bit == 1 {
                        if vmx_enable == 0 {
                            return Ok(VmxLockStatus::Locked);
                        } else {
                            return Ok(VmxLockStatus::EnabledLocked);
                        }
                    } else {
                        return Ok(VmxLockStatus::DisabledUnlocked);
                    }
                }
            }
        }

        // Can't read MSR, assume disabled
        Ok(VmxLockStatus::DisabledUnlocked)
    }

    fn create_vmx_unlock_method(&self, vendor: &str) -> Option<UnlockMethod> {
        if vendor == "Intel" {
            Some(UnlockMethod {
                method: "MSR Write".to_string(),
                risk_level: RiskLevel::Medium,
                commands: vec![
                    "# Load MSR kernel module".to_string(),
                    "modprobe msr".to_string(),
                    "# Enable VT-x (MSR 0x3A = 0x5: Lock=1, VMX=1)".to_string(),
                    "wrmsr 0x3A 0x5".to_string(),
                    "# Check /dev/kvm now exists".to_string(),
                    "ls -l /dev/kvm".to_string(),
                ],
                description: "Write to IA32_FEATURE_CONTROL MSR to enable VT-x. Only works if BIOS has not set lock bit. Requires reboot to persist.".to_string(),
                requires_reboot: true,
            })
        } else {
            Some(UnlockMethod {
                method: "BIOS Setting".to_string(),
                risk_level: RiskLevel::Safe,
                commands: vec![
                    "# Reboot and enter BIOS/UEFI setup".to_string(),
                    "# Navigate to CPU or Advanced settings".to_string(),
                    "# Enable SVM (AMD Virtualization)".to_string(),
                ],
                description: "Enable AMD-V in BIOS settings. Usually found under CPU Configuration or Advanced settings.".to_string(),
                requires_reboot: true,
            })
        }
    }

    /// Check IOMMU (VT-d/AMD-Vi) support
    fn check_iommu(
        &self,
        _flags: &[String],
    ) -> Result<Option<(CpuFeature, Option<Recommendation>)>> {
        // Check for IOMMU support in kernel
        let dmesg_output = Command::new("dmesg")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();

        let iommu_enabled = dmesg_output.contains("IOMMU enabled")
            || dmesg_output.contains("AMD-Vi")
            || dmesg_output.contains("DMAR");

        let iommu_groups_exist = std::path::Path::new("/sys/kernel/iommu_groups").exists();

        let status = if iommu_enabled || iommu_groups_exist {
            FeatureStatus::Enabled
        } else {
            FeatureStatus::DisabledByBios
        };

        let recommendation = if status == FeatureStatus::DisabledByBios {
            Some(Recommendation {
                priority: Priority::High,
                feature: "IOMMU (VT-d/AMD-Vi)".to_string(),
                reason: "IOMMU support not detected in kernel".to_string(),
                benefit: "Enable PCI passthrough for VMs, improved device isolation and security".to_string(),
                action: "Enable VT-d (Intel) or AMD-Vi (AMD) in BIOS, add intel_iommu=on or amd_iommu=on to kernel parameters".to_string(),
            })
        } else {
            None
        };

        Ok(Some((
            CpuFeature {
                name: "IOMMU (VT-d/AMD-Vi)".to_string(),
                technical_name: "iommu".to_string(),
                category: FeatureCategory::Virtualization,
                status,
                bios_locked: false,
                unlock_method: None,
            },
            recommendation,
        )))
    }

    /// Check Intel SGX (Software Guard Extensions)
    fn check_sgx(&self, flags: &[String], _msr_available: bool) -> FeatureCheckResult {
        let sgx_supported = flags.contains(&"sgx".to_string());

        if !sgx_supported {
            return Ok(None);
        }

        // Check if SGX is enabled
        let sgx_enabled = std::path::Path::new("/dev/sgx").exists()
            || std::path::Path::new("/dev/sgx_enclave").exists();

        let (status, lock, rec) = if !sgx_enabled {
            (
                FeatureStatus::DisabledByBios,
                None,
                Some(Recommendation {
                    priority: Priority::Medium,
                    feature: "Intel SGX".to_string(),
                    reason: "CPU supports SGX but it is disabled".to_string(),
                    benefit:
                        "Enable secure enclaves for confidential computing, secrets management"
                            .to_string(),
                    action: "Enable Intel SGX in BIOS (usually under Security or CPU settings)"
                        .to_string(),
                }),
            )
        } else {
            (FeatureStatus::Enabled, None, None)
        };

        Ok(Some((
            CpuFeature {
                name: "Intel SGX (Software Guard Extensions)".to_string(),
                technical_name: "sgx".to_string(),
                category: FeatureCategory::Security,
                status,
                bios_locked: lock.is_some(),
                unlock_method: None,
            },
            lock,
            rec,
        )))
    }

    /// Check Turbo Boost / Precision Boost
    fn check_turbo(
        &self,
        cpu_model: &CpuModel,
        _msr_available: bool,
    ) -> Result<Option<(CpuFeature, Option<Recommendation>)>> {
        // Check if turbo is currently enabled
        let turbo_enabled = if cpu_model.vendor == "Intel" {
            // Check /sys/devices/system/cpu/intel_pstate/no_turbo
            if let Ok(contents) =
                fs::read_to_string("/sys/devices/system/cpu/intel_pstate/no_turbo")
            {
                contents.trim() == "0" // no_turbo=0 means turbo is enabled
            } else {
                // Assume enabled if can't check
                true
            }
        } else {
            // AMD: check cpufreq boost
            if let Ok(contents) = fs::read_to_string("/sys/devices/system/cpu/cpufreq/boost") {
                contents.trim() == "1"
            } else {
                true
            }
        };

        let feature_name = if cpu_model.vendor == "Intel" {
            "Intel Turbo Boost"
        } else {
            "AMD Precision Boost"
        };

        let status = if turbo_enabled {
            FeatureStatus::Enabled
        } else {
            FeatureStatus::DisabledByBios
        };

        let recommendation = if !turbo_enabled {
            Some(Recommendation {
                priority: Priority::Medium,
                feature: feature_name.to_string(),
                reason: "Turbo/Boost is disabled".to_string(),
                benefit: "Improve single-threaded performance by 20-30%".to_string(),
                action: format!("Enable {} in BIOS or via sysfs", feature_name),
            })
        } else {
            None
        };

        Ok(Some((
            CpuFeature {
                name: feature_name.to_string(),
                technical_name: "turbo".to_string(),
                category: FeatureCategory::Performance,
                status,
                bios_locked: false,
                unlock_method: None,
            },
            recommendation,
        )))
    }

    /// Check AMD SME/SEV (Secure Memory Encryption)
    fn check_amd_encryption(
        &self,
        flags: &[String],
    ) -> Result<Option<(CpuFeature, Option<Recommendation>)>> {
        let sme_supported = flags.contains(&"sme".to_string());
        let sev_supported = flags.contains(&"sev".to_string());

        if !sme_supported && !sev_supported {
            return Ok(None);
        }

        // Check if actually enabled
        let mem_encrypt = fs::read_to_string("/sys/kernel/mm/mem_encrypt/active")
            .unwrap_or_else(|_| "0".to_string());
        let enabled = mem_encrypt.trim() == "1";

        let status = if enabled {
            FeatureStatus::Enabled
        } else {
            FeatureStatus::DisabledByBios
        };

        let recommendation = if !enabled {
            Some(Recommendation {
                priority: Priority::High,
                feature: "AMD SME/SEV".to_string(),
                reason: "CPU supports memory encryption but it is disabled".to_string(),
                benefit: "Encrypt system memory, protect against physical attacks, enable confidential computing".to_string(),
                action: "Enable AMD Memory Guard/SME in BIOS, add mem_encrypt=on to kernel parameters".to_string(),
            })
        } else {
            None
        };

        Ok(Some((
            CpuFeature {
                name: "AMD SME/SEV (Secure Memory Encryption)".to_string(),
                technical_name: "sme/sev".to_string(),
                category: FeatureCategory::Security,
                status,
                bios_locked: false,
                unlock_method: None,
            },
            recommendation,
        )))
    }
}

#[derive(Debug, PartialEq)]
enum VmxLockStatus {
    Locked,           // BIOS locked VT-x disabled
    EnabledLocked,    // BIOS locked VT-x enabled
    DisabledUnlocked, // Not locked, can be enabled
}
