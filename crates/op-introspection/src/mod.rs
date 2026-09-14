// Comprehensive system introspection
// Discovers D-Bus services, non-D-Bus services, and conversion opportunities

mod cpu_features;
pub use cpu_features::*;

mod hierarchical;
pub use hierarchical::*;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;
use zbus::Connection;

/// System introspection report
#[derive(Debug, Serialize, Deserialize)]
pub struct IntrospectionReport {
    /// D-Bus services currently managed by op-dbus
    pub managed_dbus_services: Vec<DbusServiceInfo>,

    /// D-Bus services discovered but not yet managed
    pub unmanaged_dbus_services: Vec<DbusServiceInfo>,

    /// Non-D-Bus systemd services that could be converted
    pub conversion_candidates: Vec<ConversionCandidate>,

    /// Kernel and hardware configuration
    pub system_config: SystemConfiguration,

    /// Summary statistics
    pub summary: IntrospectionSummary,
}

/// System-level configuration (kernel parameters, CPU settings, etc.)
#[derive(Debug, Serialize, Deserialize)]
pub struct SystemConfiguration {
    /// Kernel command line parameters
    pub kernel_cmdline: Vec<String>,

    /// CPU vulnerability mitigations status
    pub cpu_mitigations: Vec<CpuMitigation>,

    /// CPU features and BIOS locks analysis
    pub cpu_features: Option<CpuFeatureAnalysis>,

    /// Loaded kernel modules
    pub loaded_modules: Vec<String>,

    /// QEMU/KVM configuration (if applicable)
    pub virtualization: Option<VirtualizationConfig>,

    /// Hardware model info (for BIOS workarounds)
    pub hardware: HardwareInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CpuMitigation {
    pub vulnerability: String, // e.g., "spectre_v2", "meltdown"
    pub status: String,        // e.g., "Mitigation: ...", "Vulnerable", "Not affected"
    pub mitigation_active: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VirtualizationConfig {
    pub hypervisor: String,    // "kvm", "qemu", "xen", etc.
    pub vm_count: usize,       // Number of VMs
    pub cpu_passthrough: bool, // Host CPU features passed to guests
    pub nested_virt: bool,     // Nested virtualization enabled
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HardwareInfo {
    pub vendor: String,            // e.g., "Samsung", "Dell", "HP"
    pub model: String,             // e.g., "360 Pro", "XPS 13"
    pub bios_version: String,      // For tracking buggy BIOS
    pub known_issues: Vec<String>, // Known hardware issues
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DbusServiceInfo {
    pub service_name: String,
    pub bus_type: String, // "system" or "session"
    pub object_path: String,
    pub interfaces: Vec<InterfaceInfo>,
    pub management_status: ManagementStatus,
    pub recommended_plugin: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InterfaceInfo {
    pub name: String,
    pub methods: Vec<String>,
    pub properties: Vec<String>,
    pub signals: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ManagementStatus {
    /// Managed by built-in plugin
    ManagedBuiltIn { plugin_name: String },

    /// Managed by auto-generated plugin
    ManagedAuto,

    /// Discovered but not managed
    Unmanaged { reason: String },

    /// Could be managed with new plugin
    ConversionCandidate,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConversionCandidate {
    pub service_name: String,
    pub service_type: String,      // systemd, docker, etc.
    pub current_interface: String, // how it's currently managed
    pub dbus_opportunity: String,  // why it could use D-Bus
    pub complexity: ConversionComplexity,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ConversionComplexity {
    Easy,   // Just needs wrapper
    Medium, // Requires some refactoring
    Hard,   // Significant work needed
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IntrospectionSummary {
    pub total_dbus_services: usize,
    pub managed_services: usize,
    pub unmanaged_services: usize,
    pub conversion_candidates: usize,
    pub management_coverage: f32, // Percentage of services managed
}

pub struct SystemIntrospector;

impl Default for SystemIntrospector {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemIntrospector {
    pub fn new() -> Self {
        Self {}
    }

    /// Generate comprehensive introspection report
    pub async fn introspect_system(&self) -> Result<IntrospectionReport> {
        println!("🔍 Introspecting system...\n");

        // Discover D-Bus services
        let (system_services, session_services) = self.discover_dbus_services().await?;

        // Categorize D-Bus services
        let (managed, unmanaged) = self.categorize_dbus_services(system_services, session_services);

        // Find non-D-Bus services that could be converted
        let candidates = self.find_conversion_candidates().await?;

        // Gather system configuration (kernel, CPU, hardware)
        let system_config = self.gather_system_config().await?;

        let summary = IntrospectionSummary {
            total_dbus_services: managed.len() + unmanaged.len(),
            managed_services: managed.len(),
            unmanaged_services: unmanaged.len(),
            conversion_candidates: candidates.len(),
            management_coverage: if managed.len() + unmanaged.len() > 0 {
                (managed.len() as f32 / (managed.len() + unmanaged.len()) as f32) * 100.0
            } else {
                0.0
            },
        };

        Ok(IntrospectionReport {
            managed_dbus_services: managed,
            unmanaged_dbus_services: unmanaged,
            conversion_candidates: candidates,
            system_config,
            summary,
        })
    }

    /// Gather system-level configuration (kernel, CPU, hardware)
    async fn gather_system_config(&self) -> Result<SystemConfiguration> {
        println!("  🖥️  Gathering system configuration...");

        // Kernel command line
        let kernel_cmdline = self.read_kernel_cmdline()?;
        println!(
            "    ✓ Read kernel command line ({} parameters)",
            kernel_cmdline.len()
        );

        // CPU mitigations
        let cpu_mitigations = self.read_cpu_mitigations()?;
        println!(
            "    ✓ Checked CPU mitigations ({} vulnerabilities)",
            cpu_mitigations.len()
        );

        // Loaded modules
        let loaded_modules = self.read_loaded_modules()?;
        println!(
            "    ✓ Read loaded modules ({} modules)",
            loaded_modules.len()
        );

        // Virtualization config
        let virtualization = self.detect_virtualization()?;
        if virtualization.is_some() {
            println!("    ✓ Detected virtualization support");
        }

        // Hardware info
        let hardware = self.read_hardware_info()?;
        println!(
            "    ✓ Read hardware info ({} {})",
            hardware.vendor, hardware.model
        );

        // CPU feature analysis (detect hidden/locked BIOS features)
        let cpu_features = match CpuFeatureAnalyzer::new().analyze() {
            Ok(analysis) => {
                let locked_count = analysis.bios_locks.len();
                let disabled_count = analysis
                    .features
                    .iter()
                    .filter(|f| {
                        matches!(
                            f.status,
                            FeatureStatus::DisabledByBios | FeatureStatus::LockedByBios
                        )
                    })
                    .count();

                if locked_count > 0 || disabled_count > 0 {
                    println!(
                        "    ⚠️  CPU feature analysis: {} disabled, {} BIOS-locked",
                        disabled_count, locked_count
                    );
                } else {
                    println!("    ✓ CPU feature analysis complete");
                }
                Some(analysis)
            }
            Err(e) => {
                println!("    ⚠️  CPU feature analysis failed: {}", e);
                None
            }
        };

        Ok(SystemConfiguration {
            kernel_cmdline,
            cpu_mitigations,
            cpu_features,
            loaded_modules,
            virtualization,
            hardware,
        })
    }

    /// Read kernel command line from /proc/cmdline
    fn read_kernel_cmdline(&self) -> Result<Vec<String>> {
        let cmdline =
            std::fs::read_to_string("/proc/cmdline").context("Failed to read /proc/cmdline")?;

        Ok(cmdline.split_whitespace().map(|s| s.to_string()).collect())
    }

    /// Read CPU vulnerability mitigations from /sys
    fn read_cpu_mitigations(&self) -> Result<Vec<CpuMitigation>> {
        let vulnerabilities_dir = std::path::Path::new("/sys/devices/system/cpu/vulnerabilities");

        if !vulnerabilities_dir.exists() {
            return Ok(Vec::new());
        }

        let mut mitigations = Vec::new();

        for entry in std::fs::read_dir(vulnerabilities_dir)? {
            let entry = entry?;
            let vulnerability = entry.file_name().to_string_lossy().to_string();
            let status = std::fs::read_to_string(entry.path())?.trim().to_string();

            let mitigation_active =
                status.contains("Mitigation:") || status.contains("Not affected");

            mitigations.push(CpuMitigation {
                vulnerability,
                status,
                mitigation_active,
            });
        }

        Ok(mitigations)
    }

    /// Read loaded kernel modules from /proc/modules
    fn read_loaded_modules(&self) -> Result<Vec<String>> {
        let modules =
            std::fs::read_to_string("/proc/modules").context("Failed to read /proc/modules")?;

        Ok(modules
            .lines()
            .filter_map(|line| {
                // Format: "module_name size used_by_count ..."
                line.split_whitespace().next().map(|s| s.to_string())
            })
            .collect())
    }

    /// Detect virtualization configuration
    fn detect_virtualization(&self) -> Result<Option<VirtualizationConfig>> {
        // Check if KVM module is loaded
        let modules = self.read_loaded_modules()?;
        let has_kvm = modules.iter().any(|m| m.contains("kvm"));

        if !has_kvm {
            return Ok(None);
        }

        // Check for running QEMU/KVM VMs
        let vm_count = self.count_running_vms()?;

        // Check if CPU virtualization features are available
        let cpuinfo =
            std::fs::read_to_string("/proc/cpuinfo").context("Failed to read /proc/cpuinfo")?;
        let cpu_passthrough = cpuinfo.contains("vmx") || cpuinfo.contains("svm");

        // Check for nested virtualization
        let nested_virt = self.check_nested_virt()?;

        Ok(Some(VirtualizationConfig {
            hypervisor: "kvm".to_string(),
            vm_count,
            cpu_passthrough,
            nested_virt,
        }))
    }

    fn count_running_vms(&self) -> Result<usize> {
        // Count QEMU processes
        let output = Command::new("pgrep").arg("-c").arg("qemu").output();

        match output {
            Ok(out) if out.status.success() => {
                let count_str = String::from_utf8_lossy(&out.stdout);
                Ok(count_str.trim().parse().unwrap_or(0))
            }
            _ => Ok(0),
        }
    }

    fn check_nested_virt(&self) -> Result<bool> {
        // Check Intel nested virtualization
        if let Ok(contents) = std::fs::read_to_string("/sys/module/kvm_intel/parameters/nested") {
            if contents.trim() == "Y" || contents.trim() == "1" {
                return Ok(true);
            }
        }

        // Check AMD nested virtualization
        if let Ok(contents) = std::fs::read_to_string("/sys/module/kvm_amd/parameters/nested") {
            if contents.trim() == "1" {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Read hardware information
    fn read_hardware_info(&self) -> Result<HardwareInfo> {
        // Read DMI info
        let vendor = self
            .read_dmi_field("sys_vendor")
            .unwrap_or_else(|_| "Unknown".to_string());
        let model = self
            .read_dmi_field("product_name")
            .unwrap_or_else(|_| "Unknown".to_string());
        let bios_version = self
            .read_dmi_field("bios_version")
            .unwrap_or_else(|_| "Unknown".to_string());

        // Check for known problematic hardware
        let known_issues = self.check_known_hardware_issues(&vendor, &model);

        Ok(HardwareInfo {
            vendor,
            model,
            bios_version,
            known_issues,
        })
    }

    fn read_dmi_field(&self, field: &str) -> Result<String> {
        let path = format!("/sys/devices/virtual/dmi/id/{}", field);
        std::fs::read_to_string(&path)
            .map(|s| s.trim().to_string())
            .context(format!("Failed to read {}", path))
    }

    fn check_known_hardware_issues(&self, vendor: &str, model: &str) -> Vec<String> {
        let mut issues = Vec::new();

        // Samsung 360 Pro known issues
        if vendor.contains("SAMSUNG") && model.contains("360") {
            issues.push("Buggy BIOS: Requires acpi=off kernel parameter".to_string());
            issues.push("Power management: Use intel_idle.max_cstate=1".to_string());
            issues.push("PCIe ASPM: Use pcie_aspm=off".to_string());
            issues.push("Graphics: May need i915.enable_psr=0".to_string());
        }

        // Dell XPS 13 9370 (Killer WiFi issues)
        if vendor.contains("Dell") && model.contains("XPS") && model.contains("9370") {
            issues.push("Killer WiFi: May need pcie_port_pm=off".to_string());
            issues.push("Thunderbolt: Check BIOS settings".to_string());
        }

        // Lenovo ThinkPad X1 Carbon Gen 7 (S3 sleep issues)
        if vendor.contains("LENOVO") && model.contains("X1 Carbon") {
            issues.push("S3 sleep: May need mem_sleep_default=deep in BIOS".to_string());
        }

        issues
    }

    /// Discover all D-Bus services on system and session buses
    async fn discover_dbus_services(&self) -> Result<(Vec<String>, Vec<String>)> {
        println!("  📡 Discovering D-Bus services...");

        // Use existing auto_plugin discovery for system services (if MCP enabled)
        #[cfg(feature = "mcp")]
        let system_services =
            match crate::state::auto_plugin::PluginDiscovery::discover_services().await {
                Ok(services) => {
                    println!("    ✓ Found {} services on system bus", services.len());
                    services
                }
                Err(e) => {
                    log::warn!("Failed to use auto_plugin discovery: {}, falling back", e);
                    // Fallback to direct discovery
                    let system_conn = Connection::system().await?;
                    let services = self.list_dbus_names(&system_conn).await?;
                    println!(
                        "    ✓ Found {} services on system bus (fallback)",
                        services.len()
                    );
                    services
                }
            };

        // Direct discovery when MCP is not available
        #[cfg(not(feature = "mcp"))]
        let system_services = {
            let system_conn = Connection::system().await?;
            let services = self.list_dbus_names(&system_conn).await?;
            println!("    ✓ Found {} services on system bus", services.len());
            services
        };

        // Session bus (may not exist in server environments)
        let session_services = match Connection::session().await {
            Ok(conn) => {
                let services = self.list_dbus_names(&conn).await?;
                println!("    ✓ Found {} services on session bus", services.len());
                services
            }
            Err(_) => {
                println!("    ⊗ Session bus not available");
                Vec::new()
            }
        };

        Ok((system_services, session_services))
    }

    /// List all D-Bus service names on a connection
    async fn list_dbus_names(&self, conn: &Connection) -> Result<Vec<String>> {
        use zbus::fdo::DBusProxy;

        let proxy = DBusProxy::new(conn).await?;
        let names = proxy.list_names().await?;

        // Filter out temporary names (starting with :) and convert to String
        Ok(names
            .into_iter()
            .filter(|name| !name.starts_with(':'))
            .filter(|name| name.as_str() != "org.freedesktop.DBus")
            .map(|name| name.to_string())
            .collect())
    }

    /// Categorize D-Bus services as managed or unmanaged
    fn categorize_dbus_services(
        &self,
        system_services: Vec<String>,
        session_services: Vec<String>,
    ) -> (Vec<DbusServiceInfo>, Vec<DbusServiceInfo>) {
        let mut managed = Vec::new();
        let mut unmanaged = Vec::new();

        // Process system services
        for service in system_services {
            let info = self.analyze_dbus_service(&service, "system");
            match &info.management_status {
                ManagementStatus::ManagedBuiltIn { .. } | ManagementStatus::ManagedAuto => {
                    managed.push(info);
                }
                _ => {
                    unmanaged.push(info);
                }
            }
        }

        // Process session services
        for service in session_services {
            let info = self.analyze_dbus_service(&service, "session");
            match &info.management_status {
                ManagementStatus::ManagedBuiltIn { .. } | ManagementStatus::ManagedAuto => {
                    managed.push(info);
                }
                _ => {
                    unmanaged.push(info);
                }
            }
        }

        (managed, unmanaged)
    }

    /// Analyze a single D-Bus service
    fn analyze_dbus_service(&self, service_name: &str, bus_type: &str) -> DbusServiceInfo {
        let management_status = if service_name == "org.freedesktop.systemd1" {
            ManagementStatus::ManagedBuiltIn {
                plugin_name: "systemd".to_string(),
            }
        } else if service_name == "org.freedesktop.login1" {
            ManagementStatus::ManagedBuiltIn {
                plugin_name: "login1".to_string(),
            }
        } else if self.can_auto_generate(service_name) {
            // Service can be managed by auto-generated plugin
            ManagementStatus::ManagedAuto
        } else {
            ManagementStatus::Unmanaged {
                reason: "No plugin available, not auto-discoverable".to_string(),
            }
        };

        let recommended_plugin = self.recommend_plugin(service_name);

        DbusServiceInfo {
            service_name: service_name.to_string(),
            bus_type: bus_type.to_string(),
            object_path: format!("/{}", service_name.replace('.', "/")),
            interfaces: Vec::new(), // Would introspect in full implementation
            management_status,
            recommended_plugin,
        }
    }

    /// Check if a service can be auto-generated (uses same logic as PluginDiscovery)
    fn can_auto_generate(&self, service_name: &str) -> bool {
        // Reuse the same logic from auto_plugin.rs
        if service_name.starts_with(':') {
            return false; // Temporary unique names
        }

        if service_name == "org.freedesktop.DBus" {
            return false; // DBus daemon itself
        }

        if service_name.starts_with("org.freedesktop.DBus.") {
            return false; // DBus internal services
        }

        // Well-known freedesktop services can be auto-generated
        if service_name.starts_with("org.freedesktop.") {
            return true;
        }

        // Custom services with reverse domain names
        if service_name.contains('.') && !service_name.starts_with("org.freedesktop.systemd1.") {
            return true;
        }

        false
    }

    /// Recommend a plugin for a D-Bus service
    fn recommend_plugin(&self, service_name: &str) -> Option<String> {
        match service_name {
            "org.freedesktop.NetworkManager" => Some("networkmanager".to_string()),
            "org.freedesktop.PackageKit" => Some("packagekit".to_string()),
            "org.freedesktop.UPower" => Some("upower".to_string()),
            "org.freedesktop.UDisks2" => Some("udisks2".to_string()),
            "org.bluez" => Some("bluetooth".to_string()),
            _ => None,
        }
    }

    /// Find non-D-Bus services that could be converted
    async fn find_conversion_candidates(&self) -> Result<Vec<ConversionCandidate>> {
        println!("  🔍 Analyzing non-D-Bus services...");

        let mut candidates = Vec::new();

        // Get all systemd units
        let units = self.get_runit_services()?;
        println!("    ✓ Found {} systemd units", units.len());

        // Analyze each unit for conversion potential
        for unit in units {
            if let Some(candidate) = self.analyze_for_conversion(&unit) {
                candidates.push(candidate);
            }
        }

        println!("    ✓ Found {} conversion candidates", candidates.len());

        Ok(candidates)
    }

    /// List the host's runit services.
    ///
    /// This host boots runit as PID 1; there is no systemd and no `systemctl`.
    /// Definitions are directories under `/etc/runit/sv`, so the list is a
    /// directory read rather than a subprocess.
    fn get_runit_services(&self) -> Result<Vec<String>> {
        let entries = match std::fs::read_dir(op_core::runit::SV_DIR) {
            Ok(entries) => entries,
            Err(_) => return Ok(Vec::new()),
        };

        let mut services: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        services.sort();
        Ok(services)
    }

    /// Analyze a systemd unit for D-Bus conversion potential
    fn analyze_for_conversion(&self, unit: &str) -> Option<ConversionCandidate> {
        // Skip units already managed via D-Bus
        if unit.contains("dbus") || unit.contains("systemd") {
            return None;
        }

        // Check if this is a candidate for D-Bus conversion
        let (dbus_opportunity, complexity) = match unit {
            // Package managers
            u if u.contains("packagekit") => (
                "PackageKit provides D-Bus interface for package management".to_string(),
                ConversionComplexity::Easy,
            ),
            u if u.contains("apt") || u.contains("dnf") || u.contains("yum") => (
                "Could expose package management via D-Bus (use PackageKit)".to_string(),
                ConversionComplexity::Medium,
            ),

            // Network services
            u if u.contains("docker") => (
                "Docker could expose management API via D-Bus".to_string(),
                ConversionComplexity::Medium,
            ),
            u if u.contains("containerd") => (
                "Container runtime could benefit from D-Bus IPC".to_string(),
                ConversionComplexity::Hard,
            ),

            // Web servers
            u if u.contains("nginx") || u.contains("apache") || u.contains("httpd") => (
                "Web server status/reload could be exposed via D-Bus".to_string(),
                ConversionComplexity::Easy,
            ),

            // Databases
            u if u.contains("postgres") || u.contains("mysql") || u.contains("mariadb") => (
                "Database management could benefit from D-Bus interface".to_string(),
                ConversionComplexity::Medium,
            ),

            // VPN services
            u if u.contains("wg-quick") || u.contains("openvpn") || u.contains("wireguard") => (
                "VPN control could be exposed via D-Bus".to_string(),
                ConversionComplexity::Medium,
            ),

            // Backup services
            u if u.contains("restic") || u.contains("borgbackup") || u.contains("backup") => (
                "Backup service could expose status via D-Bus".to_string(),
                ConversionComplexity::Easy,
            ),

            _ => return None, // Not a conversion candidate
        };

        Some(ConversionCandidate {
            service_name: unit.to_string(),
            service_type: "systemd".to_string(),
            current_interface: "systemctl / systemd D-Bus (indirect)".to_string(),
            dbus_opportunity,
            complexity,
        })
    }

    /// Print human-readable report
    pub fn print_report(&self, report: &IntrospectionReport) {
        println!("\n═══════════════════════════════════════════════════════════════");
        println!("   op-dbus System Introspection Report");
        println!("═══════════════════════════════════════════════════════════════\n");

        // Hardware Info
        println!("🖥️  HARDWARE & SYSTEM CONFIGURATION");
        println!("──────────────────────────────────────────────────────────────");
        println!("  Vendor:       {}", report.system_config.hardware.vendor);
        println!("  Model:        {}", report.system_config.hardware.model);
        println!(
            "  BIOS Version: {}",
            report.system_config.hardware.bios_version
        );

        if !report.system_config.hardware.known_issues.is_empty() {
            println!("\n  ⚠️  KNOWN HARDWARE ISSUES:");
            for issue in &report.system_config.hardware.known_issues {
                println!("    • {}", issue);
            }
        }

        // Virtualization info
        if let Some(virt) = &report.system_config.virtualization {
            println!(
                "\n  Virtualization: {} ({} VMs running)",
                virt.hypervisor, virt.vm_count
            );
            println!(
                "    CPU Passthrough: {}",
                if virt.cpu_passthrough {
                    "✓ Yes"
                } else {
                    "⊗ No"
                }
            );
            println!(
                "    Nested Virt:     {}",
                if virt.nested_virt {
                    "✓ Yes"
                } else {
                    "⊗ No"
                }
            );
        }
        println!();

        // CPU Mitigations
        println!("🛡️  CPU VULNERABILITY MITIGATIONS");
        println!("──────────────────────────────────────────────────────────────");
        let active_mitigations = report
            .system_config
            .cpu_mitigations
            .iter()
            .filter(|m| m.mitigation_active)
            .count();
        let total_vulnerabilities = report.system_config.cpu_mitigations.len();
        println!(
            "  {} of {} vulnerabilities mitigated\n",
            active_mitigations, total_vulnerabilities
        );

        for mitigation in &report.system_config.cpu_mitigations {
            let status_icon = if mitigation.mitigation_active {
                "✓"
            } else {
                "⚠"
            };
            let vuln_name = mitigation.vulnerability.replace('_', " ");
            println!("  {} {}: {}", status_icon, vuln_name, mitigation.status);
        }
        println!();

        // CPU Features and BIOS Locks
        if let Some(cpu_analysis) = &report.system_config.cpu_features {
            println!("🔓 CPU FEATURES & BIOS LOCKS");
            println!("──────────────────────────────────────────────────────────────");
            println!(
                "  CPU: {} (Family {})",
                cpu_analysis.cpu_model.model_name, cpu_analysis.cpu_model.family
            );
            println!("  Microcode: {}\n", cpu_analysis.cpu_model.microcode);

            // Show disabled/locked features first (most important)
            let critical_features: Vec<_> = cpu_analysis
                .features
                .iter()
                .filter(|f| {
                    matches!(
                        f.status,
                        FeatureStatus::DisabledByBios | FeatureStatus::LockedByBios
                    )
                })
                .collect();

            if !critical_features.is_empty() {
                println!("  ⚠️  DISABLED/LOCKED FEATURES:");
                for feature in critical_features {
                    let status_icon = match feature.status {
                        FeatureStatus::LockedByBios => "🔒",
                        FeatureStatus::DisabledByBios => "⊗",
                        _ => "?",
                    };
                    let status_text = match feature.status {
                        FeatureStatus::LockedByBios => "BIOS Locked",
                        FeatureStatus::DisabledByBios => "Disabled by BIOS",
                        _ => "Unknown",
                    };
                    println!(
                        "    {} {} ({}): {}",
                        status_icon, feature.name, feature.technical_name, status_text
                    );
                }
                println!();
            }

            // Show BIOS locks with details
            if !cpu_analysis.bios_locks.is_empty() {
                println!("  🔒 BIOS LOCKS DETECTED:");
                for lock in &cpu_analysis.bios_locks {
                    println!("    Register: {}", lock.register);
                    println!("      Lock Bit: {}", lock.lock_bit);
                    println!("      Affects: {}", lock.affected_features.join(", "));
                    println!("      Method: {}", lock.lock_method);
                    println!();
                }
            }

            // Show enabled features
            let enabled_features: Vec<_> = cpu_analysis
                .features
                .iter()
                .filter(|f| matches!(f.status, FeatureStatus::Enabled))
                .collect();

            if !enabled_features.is_empty() {
                println!("  ✓ ENABLED FEATURES:");
                for feature in enabled_features {
                    println!("    ✓ {} ({})", feature.name, feature.technical_name);
                }
                println!();
            }

            // Show recommendations
            if !cpu_analysis.recommendations.is_empty() {
                println!("  💡 RECOMMENDATIONS:");
                for rec in &cpu_analysis.recommendations {
                    let priority_icon = match rec.priority {
                        Priority::Critical => "🔴",
                        Priority::High => "🟠",
                        Priority::Medium => "🟡",
                        Priority::Low => "🟢",
                    };
                    println!(
                        "    {} {} - {:?} Priority",
                        priority_icon, rec.feature, rec.priority
                    );
                    println!("       Reason: {}", rec.reason);
                    println!("       Benefit: {}", rec.benefit);
                    println!("       Action: {}", rec.action);
                    println!();
                }
            }
        }

        // Kernel Parameters (show important ones)
        println!("⚙️  KERNEL CONFIGURATION");
        println!("──────────────────────────────────────────────────────────────");

        // Filter interesting kernel parameters
        let interesting_params: Vec<_> = report
            .system_config
            .kernel_cmdline
            .iter()
            .filter(|p| {
                p.contains("acpi")
                    || p.contains("idle")
                    || p.contains("aspm")
                    || p.contains("mitigation")
                    || p.contains("pci")
                    || p.contains("i915")
                    || p.contains("kvm")
            })
            .collect();

        if !interesting_params.is_empty() {
            println!(
                "  Critical kernel parameters ({}total):\n",
                report.system_config.kernel_cmdline.len()
            );
            for param in &interesting_params {
                println!("    • {}", param);
            }
        } else {
            println!("  No critical kernel parameters detected");
            println!(
                "  (Total {} parameters)",
                report.system_config.kernel_cmdline.len()
            );
        }
        println!();

        // Summary
        println!("📊 SERVICE MANAGEMENT SUMMARY");
        println!("──────────────────────────────────────────────────────────────");
        println!(
            "  Total D-Bus services:    {}",
            report.summary.total_dbus_services
        );
        println!(
            "  ✓ Managed services:      {}",
            report.summary.managed_services
        );
        println!(
            "  ⊗ Unmanaged services:    {}",
            report.summary.unmanaged_services
        );
        println!(
            "  🔄 Conversion candidates: {}",
            report.summary.conversion_candidates
        );
        println!(
            "  Coverage:                {:.1}%\n",
            report.summary.management_coverage
        );

        // Managed D-Bus services
        if !report.managed_dbus_services.is_empty() {
            println!("✅ MANAGED D-BUS SERVICES");
            println!("──────────────────────────────────────────────────────────────");

            let built_in: Vec<_> = report
                .managed_dbus_services
                .iter()
                .filter(|s| matches!(s.management_status, ManagementStatus::ManagedBuiltIn { .. }))
                .collect();

            let auto_gen: Vec<_> = report
                .managed_dbus_services
                .iter()
                .filter(|s| matches!(s.management_status, ManagementStatus::ManagedAuto))
                .collect();

            // Show built-in plugins first
            if !built_in.is_empty() {
                println!("  Built-in Plugins (read-write):");
                for service in built_in {
                    if let ManagementStatus::ManagedBuiltIn { plugin_name } =
                        &service.management_status
                    {
                        println!("    ✓ {} → {}", service.service_name, plugin_name);
                    }
                }
                println!();
            }

            // Show auto-generated plugins
            if !auto_gen.is_empty() {
                println!("  Auto-Generated Plugins (read-only):");
                for service in auto_gen {
                    println!("    🤖 {}", service.service_name);
                    if let Some(plugin) = &service.recommended_plugin {
                        println!(
                            "       Can become: {} plugin (with semantic mapping)",
                            plugin
                        );
                    }
                }
                println!();
                println!("  ℹ️  Auto-generated plugins can query state but cannot apply changes.");
                println!("     To enable writes, create a dedicated plugin or semantic mapping.");
            }
            println!();
        }

        // Unmanaged D-Bus services
        if !report.unmanaged_dbus_services.is_empty() {
            println!("🔍 UNMANAGED D-BUS SERVICES (Conversion Opportunity)");
            println!("──────────────────────────────────────────────────────────────");
            for service in &report.unmanaged_dbus_services {
                println!("  ⊗ {} ({})", service.service_name, service.bus_type);
                if let Some(plugin) = &service.recommended_plugin {
                    println!("    → Recommended plugin: {}", plugin);
                }
                if let ManagementStatus::Unmanaged { reason } = &service.management_status {
                    println!("    Reason: {}", reason);
                }
            }
            println!();
        }

        // Conversion candidates
        if !report.conversion_candidates.is_empty() {
            println!("🔄 NON-D-BUS SERVICES (Could Be Converted)");
            println!("──────────────────────────────────────────────────────────────");
            for candidate in &report.conversion_candidates {
                let complexity_emoji = match candidate.complexity {
                    ConversionComplexity::Easy => "🟢",
                    ConversionComplexity::Medium => "🟡",
                    ConversionComplexity::Hard => "🔴",
                };
                println!(
                    "  {} {} ({})",
                    complexity_emoji, candidate.service_name, candidate.service_type
                );
                println!("    Current: {}", candidate.current_interface);
                println!("    Opportunity: {}", candidate.dbus_opportunity);
            }
            println!();
        }

        println!("═══════════════════════════════════════════════════════════════\n");
    }
}
