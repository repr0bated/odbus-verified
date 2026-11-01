# Slice 9 — System, services, and host

Source: FldSystem field inventory (read-only, cited). Integrated verbatim; canonical proposals are proposals, not renames applied to code.

Sealed/live verification is pending (door unreachable).

Base: `crates/op-plugins/src/state_plugins/` unless noted. Sensitivity: `private`=never publish raw; `hash-only`=publish only its hash / value is itself a hash commitment.

### service — `service.rs`, `service/methods.rs`
Methods: `init,run,shutdown`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|last_active|service.rs:17|Option\<u64\>|last_active|last_active_at|RFC3339 UTC timestamp\|null|private|Lifecycle struct; unit/epoch unspecified in code|
|days_since_active|service.rs:18|Option\<u64\>|—|days_since_active|u64 days|private|Derived duration, redundant with last_active_at|
|is_orphaned|service.rs:19|bool|is_* prefix|is_orphaned|bool|public|—|
|orphan_reason|service.rs:20|Option\<String\>|—|orphan_reason|string\|null|private|Free text|
|name (Init/Run/ShutdownInput)|service.rs:30,41,54|String|name everywhere|service_name|opaque service id (string)|public|Same field reused across all inputs; cf. definition_type below|
|service_type|service.rs:33|String (default)|service_type vs definition_type|service_definition_type|enum string|public|Synonym split, see (a)|
|args|service.rs:46|Vec\<String\>, max len 0|—|args|array\<string\>, must be empty|private|Compatibility-only field|
|force|service.rs:57|bool|force (Shutdown/Stop/Restart)|force|bool|public|—|
|services (ServiceSchemaState)|service.rs:139|serde_json::Value|—|services|untyped JSON|private|Entire state is untyped blob|
|definition_type (ServiceInitOutput)|service/methods.rs:15|String|definition_type vs service_type|service_definition_type|enum string|public|Output name diverges from input|
|enabled|service/methods.rs:16|bool|—|enabled|bool|public|—|
|active_state|service/methods.rs:22|String|active_state vs Incus `status`/`state`|service_active_state|enum string|public|—|
|sub_state|service/methods.rs:23|String|—|service_sub_state|enum string|public|—|
|main_pid|service/methods.rs:24|Option\<u32\>|main_pid vs procfs `pid:i32`|main_pid|u32\|null|private|Host process identifier|
|up_time|service/methods.rs:25|Option\<String\>|up_time vs procfs uptime_secs|uptime|string duration\|null|private|Unbounded formatted string, no unit type|
|operation|service/methods.rs:31|String|operation everywhere|operation|enum string|public|—|
|disposition|service/methods.rs:34|String|disposition vs enum elsewhere|disposition|enum string|public|Plain String here, typed enum in login1/hardware|
|status|service/methods.rs:35|ServiceStatus|—|service_status|record|public|Nested record|

### users — `users.rs`
Methods: `create_user,delete_user,modify_user,list_users`. Enum `AccountType{Standard,Administrator}` serde snake_case (`standard|administrator`), D-Bus codes 0/1 (users.rs:36-54).

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|provider (UsersState)|users.rs:23|String, default `"accountsservice"`|—|provider|enum string|public|State record|
|list_scope (UsersState)|users.rs:24|String, default `"cached_accounts"`|list_scope vs output `scope`|list_scope|enum string|public|—|
|username|users.rs:60,67,74,95|String|username vs login1 `user`|username|opaque principal id (string)|private|Validated nonempty/no-NUL (users.rs:127)|
|real_name|users.rs:61,76,96|String / Option\<String\>|real_name|real_name|string|private|PII|
|account_type|users.rs:62,80,88,97|AccountType / Option|—|account_type|enum `standard\|administrator`|private|Role reveals privilege|
|remove_files|users.rs:69|bool (default)|remove_files vs output remove_files_requested|remove_files|bool|private|Destructive flag|
|shell|users.rs:78,98|Option\<String\> / String|—|login_shell|path string|private|Host path|
|include_system_accounts|users.rs:86|bool (default)|vs output `system_account`|include_system_accounts|bool|public|Filter input|
|uid (UserRecord)|users.rs:94|u64|uid: u64 here, u32 in login1|user_id|u64|hash-only|Host-identifying; publish hash only|
|system_account|users.rs:99|bool|—|is_system_account|bool|private|—|
|operation|users.rs:104|String|—|operation|enum string|public|UserMutationOutput|
|changed|users.rs:105|bool|changed vs `updated/deleted/created`|changed|bool|public|Boolean verb-form inconsistency across plugins|
|user|users.rs:106|UserRecord|user vs username|user|record|private|Nested PII|
|changed_fields|users.rs:107|Vec\<String\>|—|changed_field_names|array\<string\>|private|Field names may leak attribute set|
|deleted|users.rs:114|bool|—|deleted|bool|private|DeleteUserOutput|
|remove_files_requested|users.rs:115|bool|vs input remove_files|remove_files_requested|bool|private|—|
|provider_completed|users.rs:116|bool|—|provider_completed|bool|public|Receipt-like completion flag|
|scope (ListUsersOutput)|users.rs:121|String|scope vs list_scope|list_scope|enum string|public|—|
|exhaustive|users.rs:122|bool|—|is_exhaustive|bool|public|—|
|users|users.rs:123|Vec\<UserRecord\>|—|users|array\<record\>|private|—|
|total|users.rs:124|u64|total vs total_* elsewhere|total_count|u64|public|—|

### packagekit — `packagekit.rs`
Methods: `resolve,install_packages,remove_packages,update_packages,get_updates,search_names`. Note: inputs/output at packagekit.rs:320-372 are declared after plugin registration (line ~315).

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|ensure (PackageState)|packagekit.rs:62|String `"installed"\|"removed"\|"latest"`|ensure|ensure|enum string|public|Stringly-typed enum in comment only|
|provider|packagekit.rs:63|Option\<String\> `"apt"\|"dnf"\|"pacman"`|provider (also users.rs)|package_provider|enum string\|null|public|Cross-plugin synonym|
|version (PackageState)|packagekit.rs:64|Option\<String\>|version vs state `version:u32`|package_version|string\|null|public|Name collision with state schema version|
|version (PackageKitState)|packagekit.rs:69|u32|—|schema_version|u32|public|State record: `packages: HashMap<String,PackageState>` keyed by name (line 70) — key is implicit package_name|
|packages (map key)|packagekit.rs:70|HashMap\<String,PackageState\>|packages vs package_ids|package_name (key)|string (key)|public|Unnamed field-as-key|
|filter|packagekit.rs:322,348,353|u64|filter singular|filter_bitmask|u64 bitmask|public|Unbounded opaque bitmask, PackageKit filter enum not modeled|
|packages (ResolvePackagesInput)|packagekit.rs:323|Vec\<String\>|packages vs package_ids vs values|package_names|array\<string\>|public|Name-based, pre-resolution|
|transaction_flags|packagekit.rs:328,334,342|u64|—|transaction_flags|u64 bitmask|public|Opaque bitmask|
|package_ids|packagekit.rs:329,335,343|Vec\<String\>|package_id (detail)|package_ids|array\<package_id\>|public|PackageKit `name;version;arch;data` composite string — not opaque|
|allow_deps|packagekit.rs:336|bool|—|allow_dependencies|bool|public|Abbreviation|
|autoremove|packagekit.rs:337|bool|—|autoremove|bool|public|—|
|values (SearchNamesInput)|packagekit.rs:354|Vec\<String\>|values vs packages|search_terms|array\<string\>|public|Ambiguous generic name|
|info (PackageKitPackage)|packagekit.rs:360|u32|info vs PkInfoEnum comment|package_info_code|enum u32|public|Raw D-Bus enum code|
|package_id|packagekit.rs:361|String|—|package_id|composite string|public|—|
|summary|packagekit.rs:362|String|—|summary|string|public|Free text|
|transaction (PackageKitResult)|packagekit.rs:367|String|transaction vs transaction_flags|transaction_id|opaque id (string)|hash-only|Host transaction path/id|
|packages (PackageKitResult)|packagekit.rs:368|Vec\<String\>|—|package_ids|array\<package_id\>|public|Output reuses input-style name for resolved ids|
|package_details|packagekit.rs:369|Vec\<PackageKitPackage\>|—|package_details|array\<record\>|public|—|
|exit_code|packagekit.rs:370|u32|exit_code vs incus `exit_code:i64`|exit_code|u32|public|Signedness diverges|
|runtime_ms|packagekit.rs:371|u32|runtime_ms (well-formed)|runtime_ms|u32 ms|public|Good unit-suffixed example|

### hardware — `hardware.rs`
Methods: `scan_hardware,refresh_inventory,list_devices,get_device,get_stats`. Outputs all carry `observed_at:String` + `disposition:String` `"observed"` (hardware.rs:113-153).

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|observed_at|hardware.rs:114,123,132,143,149|String|observed_at everywhere in hw/procfs|observed_at|RFC3339 UTC timestamp|private|Timestamp of host observation|
|inventory|hardware.rs:115,124|HardwareState|inventory vs state|hardware_inventory|record|private|Nested Cpu/Memory/Disk|
|cpu.model|hardware.rs:49|Option\<String\>|model vs procfs `brand`|cpu_model|string\|null|hash-only|Host-identifying (e.g. example "AMD Ryzen 9 7950X")|
|cpu.cores|hardware.rs:57|usize|cores|core_count|u64|hash-only|Host fingerprint component|
|memory.total_kb|hardware.rs:69|u64|total_kb vs procfs bytes fields|total_memory_kb|u64 KiB|hash-only|Unit in name — good; KB vs KiB ambiguous|
|memory.available_kb|hardware.rs:77|u64|available_kb|available_memory_kb|u64 KiB|hash-only|—|
|disks[].name|hardware.rs:89|String|name vs GetDeviceInput.name|disk_name|device id string|hash-only|e.g. `nvme0n1` — host device identity|
|disks[].capacity_bytes|hardware.rs:97|u64|capacity_bytes (good)|capacity_bytes|u64 bytes|hash-only|—|
|disks[].available_bytes|hardware.rs:100|u64|—|available_bytes|u64 bytes|hash-only|—|
|disks[].mountpoint|hardware.rs:109|Option\<String\>|mountpoint vs procfs MountEntry.mountpoint|mount_point|path string\|null|private|Host path|
|devices (ListDevicesOutput)|hardware.rs:133|Vec\<DiskInfo\>|devices = disks only|disks|array\<record\>|hash-only|Name overpromises: only DiskInfo, no other device types|
|name (GetDeviceInput)|hardware.rs:138|String|—|disk_name|device id string|hash-only|Lookup key|
|device|hardware.rs:144|Option\<DiskInfo\>|device singular|disk|record\|null|hash-only|—|
|disk_count|hardware.rs:152|usize|disk_count vs total|disk_count|u64|public|GetStatsOutput|

### procfs — `procfs.rs`
Methods: `list_processes,get_process_info,get_meminfo,get_cpuinfo,get_loadavg,get_uptime,get_net_dev`. State `ProcfsState` (procfs.rs:35-120) mixes typed sub-records with untyped `cpuinfo/stat/kernel/vmstat/diskstats: Value/Option<Value>`.

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|observed_at|procfs.rs:270,276,282,297,303,311,317|String|—|observed_at|RFC3339 UTC|private|On every output + ProcfsSnapshot:331|
|memory.total_kb / available_kb / used_kb|procfs.rs:126-128,283-285|u64|total_kb duplicated in hardware.rs|total_memory_kb etc.|u64 KiB|hash-only|MemInfo flattened into GetMeminfoOutput|
|load1/load5/load15|procfs.rs:134-136,304-306|f64|—|load_average_1m/5m/15m|f64|public|Unit (minutes) implicit|
|uptime_secs|procfs.rs:142,312,337|u64|uptime_secs vs service up_time String|uptime_seconds|u64 s|private|—|
|cpuinfo/stat/kernel/vmstat/diskstats|procfs.rs:68,77,102,111,120|Value / Option\<Value\>|—|(keep raw)|untyped JSON|private|Un-normalized pass-through blobs in state record|
|net_dev|procfs.rs:85|NetDev|net_dev vs method get_net_dev|network_interfaces|record|private|Snake abbreviation|
|mounts|procfs.rs:93|Option\<Vec\<MountEntry\>\>|—|mounts|array\<record\>\|null|private|—|
|MountEntry.device|procfs.rs:166|String|device (many senses)|mount_device|device path string|private|—|
|MountEntry.mountpoint|procfs.rs:167|String|mountpoint|mount_point|path string|private|—|
|MountEntry.fstype|procfs.rs:168|String|fstype|filesystem_type|enum string|public|Abbreviated|
|MountEntry.options|procfs.rs:169|String|options (raw string)|mount_options|string (comma list)|private|Unparsed|
|NetDevInterface.interface|procfs.rs:148|String|interface vs freedesktop interface_name|interface_name|device id string|hash-only|e.g. eth0 — host-identifying|
|rx_bytes/rx_packets/rx_errors, tx_*|procfs.rs:149-154|u64|—|received_bytes etc.|u64|hash-only|Traffic counters fingerprint host activity|
|ProcessSnapshot.pid|procfs.rs:261|i32|pid i32 vs service main_pid u32|process_id|i64|private|—|
|ProcessSnapshot.name|procfs.rs:262|String|name (process)|process_name|string|private|May leak user workloads|
|ProcessSnapshot.memory_bytes / virtual_memory_bytes|procfs.rs:263-264|u64|memory_bytes vs MemInfo *_kb|resident_memory_bytes / virtual_memory_bytes|u64 bytes|private|Unit convention differs from MemInfo KB within same file|
|ProcessSnapshot.status|procfs.rs:265|String|status vs incus status|process_status|enum string|private|—|
|CpuSnapshot.name|procfs.rs:290|String|name vs brand in same struct| cpu_name|string|hash-only|—|
|CpuSnapshot.brand|procfs.rs:291|Option\<String\>|brand vs hardware cpu.model|cpu_model|string\|null|hash-only|Synonym for hardware model|
|CpuSnapshot.frequency_mhz|procfs.rs:292|Option\<u64\>|frequency_mhz (good)|frequency_mhz|u64 MHz\|null|hash-only|—|
|ProcfsSnapshot.total_memory_bytes / available_memory_bytes|procfs.rs:333-334|u64|bytes here vs *_kb in MemInfo|total_memory_bytes|u64 bytes|hash-only|Same quantity, two units in one plugin|
|pid (ProcessInfoInput)|procfs.rs:682|i32|—|process_id|i64|private|Only non-empty input in plugin|

### incus — `incus.rs`, `incus_device.rs`
Methods: `create_instance,modify_instance,delete_instance,start/stop/restart/freeze/unfreeze_instance,snapshot_instance,exec_instance,add/remove/update_device,reconcile_native_creation` (+list/get via state). Wire rename: `instance_type` serialized as `"type"` (incus.rs:48,108); lifecycle event `event_type` as `"type"` (incus.rs:1928).

**IncusState / IncusInstance (incus.rs:30-99) & RawIncusInstance (104-125)**

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|instances|incus.rs:31|Vec\<IncusInstance\>|—|instances|array\<record\>|private|State record|
|name|incus.rs:44,105|String|name everywhere|instance_name|opaque id string|private|Host-local instance identity|
|status|incus.rs:46,106|String|status vs service active_state vs output `state`|instance_status|enum string|private|—|
|type → instance_type|incus.rs:48,108|String `"container"\|"virtual-machine"`|type / type_ / instance_type|instance_type|enum string|public|Rust name ≠ wire name|
|status_code|incus.rs:52,109|Option\<i64\> / i64|—|instance_status_code|i64\|null|private|Optionality differs state vs raw|
|image|incus.rs:56|Option\<String\>|image vs CreateInstanceInput.image:String|image_reference|string\|null|public|e.g. `images:debian/12`|
|storage_pool|incus.rs:59|Option\<String\>|pool (disk device)|storage_pool_name|string\|null|private|—|
|profiles|incus.rs:62,110|Vec\<String\>|—|profile_names|array\<string\>|private|—|
|description|incus.rs:65,117|Option\<String\>/String|—|description|string\|null|private|—|
|architecture|incus.rs:68,118|Option\<String\>/String|—|architecture|enum string|public|—|
|ephemeral / stateful|incus.rs:71,74|Option\<bool\>/bool|—|is_ephemeral / is_stateful|bool|private|Optionality differs|
|created_at / last_used_at|incus.rs:77,80|Option\<String\>/String|created_at vs observed_at|created_at / last_used_at|RFC3339 UTC\|null|private|—|
|location|incus.rs:83,123|Option\<String\>/String|location (also lifecycle)|cluster_location|string\|null|private|Host/cluster identifying|
|project|incus.rs:86,125|Option\<String\>/String|—|project_name|string\|null|private|Tenancy identifier|
|config / expanded_config|incus.rs:89,96|Option\<HashMap\<String,String\>\>|config (flat dotted keys)|instance_config / expanded_instance_config|map\<string,string\>|private|May contain limits/security keys; never publish raw|
|devices / expanded_devices|incus.rs:93,99|Vec\<NamedDevice\> (state) vs BTreeMap (raw:112,116)|—|devices / expanded_devices|array\<NamedDevice\>|private|Representation differs typed vs raw|
|etag (IncusHttpResponse)|incus.rs:1597|Option\<String\>|—|etag|opaque string\|null|hash-only|Concurrency token, not inventory field|

**Inputs (incus.rs:1384-1592)** — all `name:String` instance key unless noted:

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|operation_id (Create/Reconcile)|incus.rs:1389,1414|String|operation_id vs freedesktop event_id|operation_id|opaque id string|hash-only|Provenance/replay key — see (c)|
|source_mode|incus.rs:1392|String|—|source_mode|enum string|private|CreateInstanceInput|
|instance_type (input)|incus.rs:1395|String|input uses instance_type (test JSON incus.rs:3716)|instance_type|enum string|public|Input wire name ≠ instance record wire name `type`|
|image (input)|incus.rs:1398|String|—|image_reference|string|public|Required in input, optional in record|
|config (input)|incus.rs:1404|Option\<HashMap\>|—|instance_config|map\|null|private|—|
|devices (input)|incus.rs:1407|Vec\<NamedDevice\>|devices Modify: HashMap form (1511)|devices|array\<NamedDevice\>|private|Modify uses `Option<HashMap<String,HashMap<String,String>>>` — same field, two shapes|
|force (Stop/Restart)|incus.rs:1538,1544|Option\<bool\>|force bool in service|force|bool\|null|public|Optionality differs|
|snapshot_name|incus.rs:1560|String|snapshot_name vs btrfs destination|snapshot_name|string|private|SnapshotInstanceInput|
|command (Exec)|incus.rs:1566|Vec\<String\>|—|command|array\<string\>|private|Exec input — sensitive|
|instance_name / device_name|incus.rs:1571-1572,1578-1579,1590-1591|String|instance_name here vs name elsewhere|instance_name / device_name|opaque id|private|Add/Remove/UpdateDeviceInput — better naming than core inputs|
|device|incus.rs:1573,1592|Device (incus_device)|—|device|tagged union|private|—|

**Outputs (incus.rs:2363-2421)**

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|operation_id / native_uuid / phase / replayed / created|incus.rs:2364-2369|String/String/String/bool/bool|native_uuid vs uuid elsewhere|operation_id / native_instance_uuid / phase / replayed / created|opaque id / uuid / enum / bool|hash-only (uuid), private (rest)|CreateInstanceOutput — provenance receipt, see (c)|
|updated / deleted / changed / created (snap)|incus.rs:2375,2381,2388,2395|bool|changed vs updated vs deleted|changed (uniform)|bool|private|Verb-form inconsistency|
|state (InstanceStateOutput)|incus.rs:2387|String|state vs status|instance_status|enum string|private|Third name for instance status|
|response|incus.rs:2382,2389,2396,2405,2412|serde_json::Value|response everywhere|native_response|untyped JSON|private|Raw upstream payload embedded in 5 outputs|
|exit_code / stdout / stderr / completed|incus.rs:2401-2404|i64/String/String/bool|exit_code u32 in packagekit|exit_code / stdout / stderr|i64 / string|private|Exec output — stdout/stderr sensitive|
|IncusLifecycleEvent.type→event_type, timestamp, location, project|incus.rs:1928-1934|String/DateTime\<Utc\>/String/String|timestamp (typed!) vs observed_at String|event_type / occurred_at|lifecycle enum / RFC3339 UTC|private|Only properly-typed timestamp in slice; `location`+`project` repeat instance fields|
|LifecycleMetadata.action/source/context/requestor{name,project}|incus.rs:1940-1948|String/String/Map/Option|context untyped|action / source / context|enum / string / JSON|private|—|
|LifecycleRequestor.username/protocol/address|incus.rs:1954-1957|String|username vs user|requestor_username / protocol / address|string|private|Network address is host/client-identifying|

**Device union — `incus_device.rs`** `NamedDevice{name:String, device:Device}` (96-100); `Device` serde `tag="type", rename_all="kebab-case"` variants Nic/Disk/UnixChar/UnixBlock/Usb/Gpu/Infiniband/Proxy/UnixHotplug/Tpm/Pci/None (109-135). All sub-structs: every field `Option<…>` default-skip. `StrBool`/`StrInt` serialize as strings (incus_device.rs:24-85).

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|Nic: nictype,network,parent,name,hwaddr,host_name,mtu,mode,vlan,vlan_tagged,acceleration,vrf,pci,vendorid,productid,security_acls; dotted wire: `boot.priority, vlan.tagged→(field vlan_tagged), ipv4.address, ipv4.gateway, ipv4.routes, ipv4.host_address, ipv4.host_tables, ipv6.*, limits.egress, limits.ingress, limits.max, limits.priority, queue.tx.length, io.bus, security.mac_filtering, security.ipv4_filtering, security.ipv6_filtering, security.port_isolation`; gvrp,connected|incus_device.rs:138-306|Option\<String\>/StrInt/StrBool|snake field vs dotted wire (e.g. limits_priority→`limits.priority`)|keep dotted canonical: `ipv4_address` etc. as structured sub-record|StrInt→i64, StrBool→bool, rest string; hwaddr=MAC|private; hwaddr/ipv4_address/ipv6_address/gateway: hash-only|MAC + IP addresses are host/network-identifying. Limits are unbounded strings (should be quantity types)|
|Disk: path,source,source_identity,pool,readonly,required,recursive,shift,size,size_state,boot_priority→`boot.priority`,limits_read→`limits.read`,limits_write→`limits.write`,limits_max→`limits.max`,io_bus→`io.bus`,io_cache,propagation,raw_mount_options,ceph_cluster_name,ceph_user_name,wwn|incus_device.rs:309-395|Option\<String\>/StrBool/StrInt|pool vs storage_pool; size as String|disk_path/source_path/pool_name/size_bytes…|path string / bool / i64; size unparsed string|private; wwn/ceph_*: hash-only|Disk identity + host paths|
|UnixChar/UnixBlock (UnixDevice): source,path,uid,gid,mode,major,minor,required|incus_device.rs:398-420|Option\<String\>/StrInt/StrBool|uid StrInt here vs u64/u32 elsewhere|uid/gid|u32|private|Device node identity|
|Usb: vendorid,productid,busnum,devnum,serial,uid,gid,mode,required|incus_device.rs:423-447|Option\<String\>/StrInt/StrBool|vendorid vs vendor_id|vendor_id/product_id/serial_number|string|hash-only (serial, vendor/product)|USB serial is device-identifying|
|Gpu: gputype,id,pci,vendorid,productid,uid,gid,mode,mdev,mig_ci,mig_gi,mig_uuid|incus_device.rs:450-480|Option\<String\>/StrInt|id vs pci address|gpu_id/pci_address/mig_uuid|uuid/string|hash-only|PCI/MIG UUID identify host hardware|
|Infiniband: nictype,parent,name,hwaddr,mtu|incus_device.rs:483-496|Option|hwaddr duplicate of Nic|—|as Nic|hash-only|—|
|Proxy: connect,listen,bind,nat,proxy_protocol,uid,gid,mode,security_uid→`security.uid`,security_gid→`security.gid`|incus_device.rs:499-535|String/Option\<StrBool/StrInt\>|connect/listen are socket specs|connect_address/listen_address|socket string|private|Forwarding endpoints sensitive|
|UnixHotplug: vendorid,productid,pci,uid,gid,mode,required|incus_device.rs:538-556|Option|—|vendor_id…|string|hash-only|—|
|Tpm: path,pathrm|incus_device.rs:559-565|Option\<String\>|pathrm opaque abbrev.|tpm_path / tpm_resource_manager_path|path string|private|—|
|Pci: address,firmware|incus_device.rs:569-575|String/Option\<StrBool\>|address vs requestor address|pci_address|PCI BDF string|hash-only|Required field (only non-Option besides Proxy connect/listen)|

### btrfs — `btrfs_plugin.rs`
Methods: `create_subvolume,delete_subvolume,snapshot,list_subvolumes,scrub_start,scrub_status,balance_start,balance_status,filesystem_df,filesystem_usage,device_add,device_remove`. Inputs via `path_input!` macro = `{path:String}` for Create/Delete/List/ScrubStart/ScrubStatus/BalanceStart/BalanceStatus/FilesystemDf/FilesystemUsage (btrfs_plugin.rs:15-33). Enums `ScrubState`, `BalanceState` snake_case (112-139).

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|path (all path_input!)|btrfs_plugin.rs:20 (macro), used 25-33|String|path vs source/destination/device|filesystem_path|absolute path string|private|Host path|
|source / destination (SnapshotInput)|btrfs_plugin.rs:38-39|String|source/destination vs incus snapshot_name|source_subvolume_path / destination_snapshot_path|path string|private|—|
|readonly (SnapshotInput)|btrfs_plugin.rs:41|bool (default)|readonly vs DiskDevice.readonly StrBool|is_readonly|bool|public|—|
|device (DeviceAdd/RemoveInput)|btrfs_plugin.rs:47,54|String|device vs DeviceRecord.path|device_path|device path string|private|Input identifies device by path|
|SubvolumeRecord.id|btrfs_plugin.rs:61|u64|id vs removed_subvolume_id|subvolume_id|u64|hash-only|—|
|SubvolumeRecord.name|btrfs_plugin.rs:62|String|name basename|subvolume_name|string|private|—|
|parent_id|btrfs_plugin.rs:63|u64|—|parent_subvolume_id|u64|hash-only|—|
|generation|btrfs_plugin.rs:64,185,226|u64|generation (also FsIdentity, FilesystemUsage)|generation|u64|hash-only|Btrfs transaction id|
|uuid / parent_uuid / received_uuid|btrfs_plugin.rs:65-67|String / Option\<String\>|uuid vs native_uuid vs filesystem_uuid|subvolume_uuid / parent_uuid / received_uuid|uuid string|hash-only|—|
|readonly (record)|btrfs_plugin.rs:68|bool|—|is_readonly|bool|public|—|
|operation / path / subvolume / total_subvolumes|btrfs_plugin.rs:74-77|String/String/SubvolumeRecord/u64|total_subvolumes vs total vs remaining_subvolumes|operation / subvolume_path / subvolume / total_subvolume_count|enum/path/record/u64|private|SubvolumeMutationOutput|
|removed_subvolume_id / remaining_subvolumes|btrfs_plugin.rs:84-85|u64|—|removed_subvolume_id / remaining_subvolume_count|u64|hash-only/public|DeleteSubvolumeOutput|
|filesystem_uuid|btrfs_plugin.rs:91,122,149,165,184,199|String|filesystem_uuid vs FsIdentity.uuid|filesystem_uuid|uuid string|hash-only|Appears in 6 outputs; internal FsIdentity names it just `uuid` (225)|
|total / maximum_supported|btrfs_plugin.rs:93,95|u64 / Option\<u64\>|total vs total_subvolumes|total_count / maximum_supported|u64|public|ListSubvolumesOutput; also BtrfsState.maximum_subvolumes:210|
|ScrubProgressRecord: device_id,data_bytes_scrubbed,tree_bytes_scrubbed,read_errors,checksum_errors,verify_errors,uncorrectable_errors,corrected_errors|btrfs_plugin.rs:101-108|u64 ×8|device_id vs DeviceRecord.id|device_id + *_count suffixes|u64 bytes/counts|hash-only (device_id), public (counts)|Well unit-named bytes fields|
|state (Scrub/BalanceOutput)|btrfs_plugin.rs:123,150|ScrubState/BalanceState|state vs status|scrub_state / balance_state|enum|public|—|
|BalanceProgressRecord.expected/considered/completed_chunks|btrfs_plugin.rs:141-143|u64|—|expected_chunk_count etc.|u64|public|—|
|SpaceRecord.flags/total_bytes/used_bytes|btrfs_plugin.rs:157-159|u64|flags raw u64|space_flags (bitmask)/total_bytes/used_bytes|u64|public|flags not decoded to enum|
|FilesystemDfOutput.spaces/allocated_bytes/used_bytes|btrfs_plugin.rs:166-168|Vec\<SpaceRecord\>/u64|used_bytes repeated|—|u64 bytes|public|—|
|DeviceRecord.id/uuid/path/total_bytes/bytes_used|btrfs_plugin.rs:174-178|u64/String/String/u64/u64|bytes_used vs used_bytes|device_id/device_uuid/device_path/total_bytes/used_bytes|u64/uuid/path|hash-only (uuid), private (path)|Field named `bytes_used` here, `used_bytes` in SpaceRecord — same file|
|FilesystemUsageOutput.node_size/sector_size/devices/spaces/device_total_bytes|btrfs_plugin.rs:186-192|u32/u32/…|device_total_bytes vs total_bytes|node_size_bytes/sector_size_bytes|u32 bytes|hash-only|Sizes without _bytes suffix|
|DeviceMutationOutput.operation/device/devices|btrfs_plugin.rs:198-201|String/String/Vec\<DeviceRecord\>|device = path string here|device_path|path|private|—|
|BtrfsState.deployment_transport|btrfs_plugin.rs:211|String|—|deployment_transport|enum string|public|State record alongside maximum_subvolumes|

### login1 — `login1.rs`, `login1/methods.rs`
Methods: `list_sessions,list_users,list_seats,get_session,get_user,get_seat,create_session,release/terminate/lock/unlock_session,kill_session,kill_user,terminate_user,terminate_seat,set_user_linger,attach_device,flush_devices,power_off,reboot,suspend,hibernate,sleep`. Enum `LoginOperationDisposition{Accepted}` snake_case (methods.rs:51); `SessionPropertyValue` serde `tag="type",content="value"` (methods.rs:69).

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|Login1State.sessions|login1.rs:22|Vec\<SessionInfo\>|—|sessions|array\<record\>|private|State record|
|SessionInfo.id|login1.rs:30|String|id vs session_id (inputs)|session_id|opaque id string|private|Record uses `id`, every input uses `session_id`|
|SessionInfo.uid|login1.rs:32|u32|uid u32 vs users u64|user_id|u64|hash-only|—|
|SessionInfo.user|login1.rs:34|String|user vs username|username|string|private|—|
|SessionInfo.seat|login1.rs:36|String|seat vs seat_id|seat_id|string|private|Record `seat`, inputs `seat_id`|
|SessionInfo.path|login1.rs:38|String|path = D-Bus object path|session_object_path|object path string|private|Ambiguous `path` (cf. btrfs fs path)|
|session_id (Get/SessionId/Kill inputs)|login1.rs:139,178,183|String|—|session_id|opaque id|private|—|
|uid (GetUser/Uid/KillUser/SetUserLinger)|login1.rs:144,159,190,195,201|u32|—|user_id|u64|hash-only|—|
|seat_id (GetSeat/SeatId/Attach/Create)|login1.rs:149,165,208,213|String|—|seat_id|string|private|—|
|interactive|login1.rs:154,203,215,220|bool|—|interactive|bool|public|PowerOff/SetUserLinger/Attach/Interactive inputs|
|CreateSessionInput: pid,service,type_,class,desktop,vtnr,tty,display,remote,remote_user,remote_host|login1.rs:160-171|u32/String×…/bool|type_ (wire likely `type_` — no serde rename seen) vs incus `"type"`|pid→process_id; service→service_name; type_→session_type; class→session_class; desktop→desktop_name; vtnr→virtual_terminal_number; tty→tty_path; display→display_name; remote→is_remote; remote_user→remote_username; remote_host→remote_host|mixed|private; remote_host/remote_user hash-only|Most host-identifying input in slice; `properties:Vec<SessionProperty>` at :173|
|SessionProperty.name / value{type,value}|login1.rs via methods.rs:84-85,69-81|String / tagged enum|type tag duplicated pattern|property_name / property_value|enum-typed value|private|Methods.rs definition|
|who / signal_number (KillSession/KillUser)|login1.rs:184-185,196|String / i32|who ambiguous|kill_scope / signal_number|enum string / i32|private|—|
|enable (SetUserLinger)|login1.rs:202|bool|enable vs enabled|enable_linger|bool|private|—|
|sysfs_path (AttachDevice)|login1.rs:214|String|sysfs_path (good)|sysfs_path|path string|private|Host device path|
|flags (SleepInput)|login1.rs:226|u64|flags vs btrfs flags|sleep_flags|u64 bitmask|public|—|
|UserInfo.uid/user/path (methods)|login1/methods.rs:14-16|u32/String/String|path again object path|user_id/username/user_object_path|—|private/hash-only(uid)|ListUsersOutput record — third user representation (cf. users::UserRecord, SessionInfo)|
|SeatInfo.id/path|login1/methods.rs:21-22|String|id vs seat_id|seat_id/seat_object_path|string|private|—|
|object_path (GetObjectOutput)|login1/methods.rs:39|String|object_path vs path|object_path|object path string|private|Output uses long form, records use `path`|
|operation / disposition|login1/methods.rs:46-47|String / LoginOperationDisposition|disposition typed here, String in service|operation/disposition|enum|public|LoginOperationOutput|
|CreateSessionOutput: session_id,object_path,runtime_path,uid,seat_id,vtnr,existing|login1/methods.rs:57-63|String/String/String/u32/String/u32/bool|runtime_path new|session_id/object_path/runtime_path/user_id/seat_id/virtual_terminal_number/is_existing|—|private|Receipt-like creation output|

### endpoint — `endpoint.rs`
Methods: `list_endpoints,get_endpoint,add_endpoint,remove_endpoint`.

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|endpoints (EndpointState)|endpoint.rs:22|Vec\<String\>|endpoints plural vs endpoint singular|endpoints|array\<endpoint string\>|private|State; example `192.168.1.100:8080` (line 19) — IP:port is network-identifying|
|endpoint (EndpointInput)|endpoint.rs:44|String, ≤2048 chars validated :70-80|endpoint|endpoint|host:port string|hash-only|Free-form string where structured host+port belongs|
|endpoints (ListEndpointsOutput)|endpoint.rs:49|Vec\<String\>|—|endpoints|array\<string\>|hash-only|—|
|endpoint (GetEndpointOutput)|endpoint.rs:53|Option\<String\>|—|endpoint|string\|null|hash-only|—|
|endpoint / present / revision (EndpointRegistrationOutput)|endpoint.rs:57-59|String/bool/u64|revision vs version|endpoint/is_present/revision|u64 revision|hash-only (endpoint)|revision is provenance-like monotonic counter, see (c)|

### freedesktop — `freedesktop.rs`
Generic projection over sealed plugin catalog. Internal `DbusInterface/DbusMethod/DbusSignal/DbusProperty/FreeDesktopConfig` (freedesktop.rs:161-198,560-564) are catalog descriptors, not method outputs.

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|plugin_id (all inputs/outputs)|freedesktop.rs:53,62,70,81,87,93,98,114,128,139,149,156|String|plugin_id vs plugin (schema index)|plugin_id|opaque id string|public|Consistent in this file|
|interface_name|freedesktop.rs:55,64,72,100|String, default canonical interface :54|interface_name vs procfs `interface`|interface_name|string|public|—|
|property_name|freedesktop.rs:56,73,115,129|String|—|property_name|string|public|—|
|value (SetPropertyInput/GetPropertyOutput)|freedesktop.rs:75,116|serde_json::Value|value vs SessionPropertyValue|property_value|JSON (schema-checked per comment :74)|private|Untyped at this layer, checked against sealed field downstream|
|ManagedPluginObject.object_path|freedesktop.rs:99|String|object_path vs login1 `path`|object_path|object path string|private|—|
|schema_hash|freedesktop.rs:101,118,141,151|String|schema_hash (hash as field)|schema_hash|lowercase hex sha256|hash-only|Value is itself a hash — publishable commitment, see (c)|
|properties|freedesktop.rs:103|BTreeMap\<String,Value\>|properties (login1 SessionProperty list)|properties|map\<string,JSON\>|private|Only sealed public-SHM fields per comment :102 — raw values still sensitive by field|
|projection_available|freedesktop.rs:104|bool|—|projection_available|bool|public|—|
|objects|freedesktop.rs:109|Vec\<ManagedPluginObject\>|—|objects|array\<record\>|private|GetManagedObjectsOutput|
|read_only|freedesktop.rs:117|bool|read_only vs btrfs readonly|is_read_only|bool|public|—|
|method (SetPropertyOutput)|freedesktop.rs:130|String|method (mapped mutation name)|mapped_method|string|public|—|
|result|freedesktop.rs:132|serde_json::Value|result vs incus `response`|result|JSON|private|No requested-value echo per comment :131|
|event_id|freedesktop.rs:133|u64|event_id vs operation_id vs revision|event_id|u64|hash-only|Receipt field — see (c)|
|event_hash|freedesktop.rs:134|String|event_hash|event_hash|lowercase hex sha256|hash-only|Receipt/provenance anchor field|
|xml (IntrospectOutput)|freedesktop.rs:144|String|—|introspection_xml|XML string|public|Canonical PluginV1 ABI|
|machine_id (GetMachineIdOutput)|freedesktop.rs:157|String|machine_id|machine_id|opaque id string|hash-only|Quintessential host identifier — never publish raw|
|DbusMethod/DbusSignal.name, signature, description; DbusProperty.name, property_type, access(Read\|Write\|ReadWrite)|freedesktop.rs:179-198|String/String/String, enum|property_type vs type|name/signature/description; property_name/property_type/access_mode|—|public|Catalog descriptor records|
|FreeDesktopConfig.enable_object_manager/enable_properties/enable_introspection/enable_peer|freedesktop.rs:561-564|bool|enable_* vs is_*|enable_*|bool|public|Config record|

---

### (a) Duplicate / synonym fields across this slice
- **User principal, 3 representations:** `users::UserRecord{uid:u64,username,real_name,account_type,shell,system_account}`, `login1::UserInfo{uid:u32,user,path}`, `login1::SessionInfo{uid:u32,user,seat,…}` (login1/methods.rs:13, login1.rs:28). `username` vs `user`; `uid` u64 vs u32.
- **Instance/service/session status:** service `active_state`+`sub_state`, incus record `status`, incus output `state`, procfs process `status` — four names/shapes for lifecycle state.
- **Service type:** input `service_type` (service.rs:33) vs output `definition_type` (service/methods.rs:15).
- **Type discriminator, 3 conventions:** incus `instance_type`→wire `"type"` (incus.rs:48), login1 `type_` (no rename, login1.rs:162), freedesktop/session `type` tag (login1/methods.rs:69, incus_device.rs:109).
- **Device identity:** hardware `DiskInfo.name`, btrfs `DeviceRecord{path,uuid,id}` vs input `device:String`, procfs `MountEntry.device`, incus `NamedDevice.name` — device referenced by name, path, and id in different plugins.
- **Memory size:** hardware/procfs MemInfo `*_kb` vs ProcfsSnapshot/ProcessSnapshot `*_bytes` (procfs.rs:126 vs 263,333); btrfs consistently `*_bytes` except `node_size/sector_size` (btrfs_plugin.rs:186-187).
- **Counts:** `total` (users, btrfs list), `total_subvolumes`, `remaining_subvolumes`, `disk_count`, `*_count` absent elsewhere.
- **Change booleans:** `changed` (users, incus device), `updated`, `deleted`, `created`, `replayed`, `existing`, `present`, `provider_completed`, `deleted` — no uniform mutation receipt shape.
- **Scope:** users state `list_scope` vs output `scope`.
- **CPU model:** hardware `cpu.model` vs procfs `CpuSnapshot.brand` (+ `name` in same struct).
- **Raw upstream payload:** incus `response:Value` ×5 (incus.rs:2382-2412) vs freedesktop `result:Value` (freedesktop.rs:132).
- **Object path:** login1 records `path` vs outputs `object_path`; freedesktop `object_path` — plus btrfs/fs `path`, incus disk `path`, TPM `path/pathrm` all unrelated senses.
- **Pool:** incus instance `storage_pool` vs DiskDevice `pool`.

### (b) Mis-normalized fields
- **Stringly enums:** service `active_state,sub_state,disposition,service_type`; incus `status,instance_type,source_mode,phase,state`; packagekit `ensure,provider,info:u32`; procfs process `status`; btrfs `SpaceRecord.flags:u64`, `deployment_transport`. Only AccountType/ScrubState/BalanceState/LoginOperationDisposition/SessionPropertyValue/Device are typed enums.
- **Unbounded strings where structured/id/hash/enum belongs:** `endpoint` (free host:port string, endpoint.rs:44), packagekit `package_id` composite string + `filter/transaction_flags` raw u64 bitmasks, incus `config/expanded_config` dotted-key maps, `location`, Nic `limits_*` quantity strings, Disk `size:String`, procfs `MountEntry.options` raw string, `up_time:String` (service), freedesktop `value/result:Value`, incus `response:Value`, procfs state `cpuinfo/stat/kernel/vmstat/diskstats:Value`, service state `services:Value`.
- **Mixed case/format conventions:** wire dotted keys vs Rust snake in incus_device (`limits_priority`→`limits.priority`, ~20 fields); `type_` vs `"type"`; `fstype`, `net_dev`, `pathrm`, `wwn`, `hwaddr`, `vendorid/productid` (not `vendor_id`); abbreviations `allow_deps`, `uid/gid` as StrInt strings in devices vs integers elsewhere.
- **Unit inconsistency:** KB (MemInfo) vs bytes (snapshots/process) for same memory quantities; `runtime_ms`, `uptime_secs`, `frequency_mhz` are the good unit-suffixed examples; `node_size/sector_size` lack suffix.
- **Signedness/width drift:** `pid:i32` (procfs, login1) vs `main_pid:u32`; `exit_code:u32` (packagekit) vs `i64` (incus); `uid:u64` vs `u32`; `status_code Option<i64>` vs `i64` (incus state vs raw); many device fields Option in state, required in Raw.
- **Timestamps all `String`** (observed_at, created_at, last_used_at, up_time) except IncusLifecycleEvent `timestamp:DateTime<Utc>` (incus.rs:1929) — the sole typed timestamp.
- **Same field, two shapes:** incus `devices` = Vec\<NamedDevice\> in create/state, `HashMap<String,HashMap<String,String>>` in ModifyInstanceInput (incus.rs:1511); `packages` = names in resolve input, ids in result.

### (c) Tagged routing / provenance relevance
- **Hash/receipt fields already present:** freedesktop `schema_hash` (4 outputs), `event_id:u64`+`event_hash:String` on SetPropertyOutput (freedesktop.rs:133-134) — closest existing receipt; incus `operation_id`+`native_uuid`+`replayed` on create (incus.rs:2364-2367) functions as an idempotency/provenance receipt; endpoint `revision:u64` (endpoint.rs:59) is a monotonic state counter usable as provenance sequence.
- **No tag fields anywhere in this slice:** no `tags`, control tags, capability strings, decision, or grant fields appear in any inventoried record — routing tags must come from the schema-tag registry/catalog layer, not these payloads. Btrfs method declarations do carry capabilities outside the records (`cap.storage.btrfs.*@v1`, `mut.storage.btrfs.*@v2`, btrfs_plugin.rs:914-931), and OSCAL subids annotate hardware/endpoint/users schemas (e.g. hardware.rs:12, endpoint.rs:12) — those are the routing hooks.
- **Hash-only publication candidates:** `machine_id`, `schema_hash`, `event_hash`, filesystem/subvolume/device UUIDs, MAC (`hwaddr`), USB `serial`, GPU `mig_uuid`/PCI addresses, `uid`s, endpoint strings — per boundary in spec, publish hashes/commitments only.
- **Always-private:** usernames/real names, session ids, remote_user/remote_host, process lists, exec command/stdout/stderr, incus `config` maps and `response` payloads, host paths (mountpoints, sysfs, tty, device paths), IP addresses in endpoints/NICs.
- **Incus lifecycle events** (`IncusLifecycleEvent/Metadata/Requestor`, incus.rs:1926-1957) are a provenance stream in embryo: typed timestamp + actor (requestor) + action + project, but no hash linkage, operation_id, or decision field to chain them.

