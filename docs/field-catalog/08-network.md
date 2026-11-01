# Slice 8 — Network

Source: FldNetwork field inventory (read-only, cited). Integrated verbatim; canonical proposals are proposals, not renames applied to code.

Sealed/live verification is pending (door unreachable). Read-only inventory; no files edited. Base path for all sources: `/srv/git/odbus-verified/crates/op-plugins/src/state_plugins/`. Files found: `net.rs`, `wireguard.rs`, `wgcf.rs`, `wg_opdbus.rs`, `openflow.rs`, `openflow_obfuscation.rs`, `ovsdb_bridge.rs`, `ovsdb_topology.rs` (+`ovsdb_topology_tests.rs`, test-only, excluded), `xray.rs`, `xray_config_types.rs`, `proxy_server.rs`, `dnsresolver.rs`, `privacy_routes.rs`.

Sensitivity: `public`=operational/non-secret, `hash-only`=publish/commit only as lowercase-hex SHA-256 or opaque commitment, `private`=never publish raw.

## net — `NetworkConfig`/`InterfaceConfig`/`TunableConfig`/`Ipv4Config`/`Ipv6Config`/`AddressConfig`

|field_as_found|source (file:line)|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity (public\|hash-only\|private)|notes|
|---|---|---|---|---|---|---|---|
|interfaces|net.rs:30|Vec\<InterfaceConfig\>|interfaces|interface|array\<InterfaceConfig\>|public|root list|
|name|net.rs:44|String|name, interface_name, bridge|interface_name|string (bounded)|public|also method key|
|type (wire) / if_type|net.rs:47-54|InterfaceType{kebab: ethernet,ovs-bridge,ovs-port,bridge}|type, if_type, interface_type|interface_type|enum|public|serde rename `type`|
|driver|net.rs:62|Option\<String\>|driver|interface_driver|string enum-like|public|L2 driver, unbounded|
|l3_driver|net.rs:86|Option\<String\>|l3_driver|l3_driver|string enum-like|public|flattened via TunableConfig|
|ports|net.rs:78|Option\<Vec\<String\>\>|ports, attached_ports|port_name|array\<string\>|public|interface port names|
|ipv4 / ipv6|net.rs:94,102|Option\<Ipv4Config/Ipv6Config\>|ipv4, ipv6|ipv4_config / ipv6_config|object|public||
|controller|net.rs:110|Option\<String\>|controller, controller_endpoint, endpoint|controller_endpoint|string endpoint|public|SDN controller for OpenFlow|
|properties|net.rs:123|Option\<HashMap\<String,Value\>\>|properties, options, other_config|interface_property|map\<string,json\>|private|dynamic introspection bag, unbounded; may capture MACs|
|property_schema|net.rs:132|Option\<Vec\<String\>\>|property_schema|property_schema|array\<string\>|public|append-only field-name set|
|enabled (v4/v6)|net.rs:152,193|bool|enabled|ipv4_enabled/ipv6_enabled|bool|public||
|dhcp (v4/v6)|net.rs:159,200|Option\<bool\>|dhcp|dhcp_enabled|bool|public||
|address|net.rs:166|Option\<Vec\<AddressConfig\>\>|address, addresses, ipv4_addresses|ipv4_address|array\<AddressConfig\>|private|IP addresses = provenance-sensitive|
|gateway|net.rs:174|Option\<String\>|gateway, default_gateway|default_gateway_ipv4|ip address string|private||
|dns|net.rs:182|Option\<Vec\<String\>\>|dns, servers|dns_server_ipv4|array\<ip string\>|private||
|ip|net.rs:211|String|ip, address, assigned_ip|ip_address|IPv4/IPv6 string|private||
|prefix|net.rs:218|u8|prefix|prefix_length|u8 (0-32/128)|public|CIDR prefix|

## net — method records

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|interface (ApplyInterfaceInput)|net.rs:950|ApplyInterfaceConfig|interface|interface|object|public||
|name / type→interface_type / ports / ipv4 (ApplyInterfaceConfig)|net.rs:956-962|String/ApplyInterfaceType{ethernet,ovs-bridge}/Vec\<String\>/Option\<StaticIpv4ApplyConfig\>|name,type,ports|interface_name/interface_type/port_name/ipv4_config|as above|public/private IPs|narrower than InterfaceConfig: no ipv6/driver/controller|
|addresses / gateway (StaticIpv4ApplyConfig)|net.rs:976-978|Vec\<StaticIpv4Address\>/Option\<String\>|addresses|ipv4_address/default_gateway_ipv4|array/ip|private||
|ip / prefix (StaticIpv4Address)|net.rs:984-985|String/u8|ip|ip_address/prefix_length|ip/u8|private/public|duplicates AddressConfig|
|name (DeleteOvsBridgeInput)|net.rs:992|String|name, bridge|bridge_name|string|public||
|name/interface_type/link_state/attached_ports/ipv4_addresses/default_gateway/operations_completed (InterfaceApplyOutput)|net.rs:997-1003|String/ApplyInterfaceType/String/Vec\<String\>/Vec\<AddressConfig\>/Option\<String\>/Vec\<String\>|name,type,ports,address,gateway|interface_name/interface_type/link_state/port_name/ipv4_address/default_gateway_ipv4/operation|string/enum/array|public; IPs private|`link_state`,`operations_completed` unbounded strings, should be enums|
|name/absent (BridgeDeleteOutput)|net.rs:1008-1009|String/bool|name|bridge_name/absent|string/bool|public|receipt-like|

## wireguard — core

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|interfaces (WireGuardState)|wireguard.rs:25|Vec\<WireGuardInterface\>|interfaces|interface|array|public|sanitized kernel readback|
|name (WireGuardInterface)|wireguard.rs:32|String|name, interface, interface_name|interface_name|string|public||
|public_key (Interface)|wireguard.rs:33|Option\<String\>|public_key, wireguard_public_key|wireguard_public_key|base64 X25519 key|hash-only|linkable identity key|
|listen_port|wireguard.rs:34|Option\<u16\>|listen_port, port|listen_port|u16|public||
|fwmark|wireguard.rs:35|Option\<u32\>|fwmark|fwmark|u32|public|routing mark|
|peers|wireguard.rs:38|Vec\<WireGuardPeer\>|peers|peer|array|public||
|public_key (Peer/PeerConfig)|wireguard.rs:45,59|String|public_key|peer_public_key|base64 key|hash-only||
|allowed_ips|wireguard.rs:48,62|Vec\<String\>|allowed_ips, addresses, assigned_ip|allowed_ip_prefix|array\<CIDR string\>|private|routes per peer|
|endpoint (Peer/PeerConfig)|wireguard.rs:49,64|Option\<String\>|endpoint|peer_endpoint|host:port string|private||
|persistent_keepalive|wireguard.rs:50,66|Option\<u16\>|persistent_keepalive|persistent_keepalive_seconds|u16 seconds|public|unit implicit|
|latest_handshake_unix|wireguard.rs:51|Option\<i64\>|latest_handshake_unix|latest_handshake_at_unix_seconds|i64 unix s|private|timestamp not RFC3339|
|rx_bytes / tx_bytes|wireguard.rs:52-53|u64|rx_bytes, tx_bytes|rx_bytes/tx_bytes|u64 bytes|public|units explicit, good|
|preshared_key (PeerConfig)|wireguard.rs:69|Option\<String\>|preshared_key|preshared_key|secret string|private|`x-secret=true`; secret|
|private_key (DeviceConfig/SetDeviceInput)|wireguard.rs:77,93|Option\<String\>|private_key|private_key|secret string|private|`x-secret=true`; secret|
|listen_port/fwmark/peers (DeviceConfig)|wireguard.rs:79-84|Option\<u16\>/Option\<u32\>/Vec\<WireGuardPeerConfig\>|listen_port|listen_port/fwmark/peer|u16/u32/array|public; peers contain private PSK||
|interface (all method inputs)|wireguard.rs:90,103,118,126,133,152|String|interface, interface_name, name|interface_name|string|public|synonym of `name`|
|interface/changed/peer (outputs)|wireguard.rs:108,113,139,144-146,159|Option\<WireGuardInterface\>/WireGuardInterface/Vec\<WireGuardPeer\>/bool/Option\<WireGuardPeer\>|interface|interface/peer/changed|object/array/bool|public; keys hash-only||
|config (SetConfigInput)|wireguard.rs:154|WireGuardDeviceConfig|config|device_config|object|private|embeds private_key/preshared_key|

## wgcf (Cloudflare WARP façade over wireguard)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|interface (WgcfConfig/GenerateConfigInput)|wgcf.rs:17,105|String (default fn)|interface|interface_name|string|public||
|id (Account/Device)|wgcf.rs:33,47|String|id, profile_id, device id|account_id / device_id|opaque string id|hash-only|generic `id` ambiguous|
|account_type/role/warp_plus/quota/premium_data/referral_count/referral_renewal_countdown|wgcf.rs:34-40|String/String/bool/f64/f64/f64/f64|type,role|account_type/role/warp_plus/quota/premium_data/referral_count/referral_renewal_countdown|enum/string/bool/f64|private|account metadata; f64 counts mis-typed (counts should be u64)|
|created/updated (Account/Device)|wgcf.rs:41-42,53-54|String|created, updated, created_at|created_at/updated_at|RFC3339 UTC string|private|name variant vs `created_at` elsewhere|
|enabled/name/model/type→device_type/warp_enabled (Device)|wgcf.rs:48-55|bool/Option\<String\>/Option\<String\>/String/bool|name, device_name|device_enabled/device_name/device_model/device_type/warp_enabled|bool/string/enum|private||
|public_key (Device)|wgcf.rs:56|String|public_key|wireguard_public_key|base64 key|hash-only||
|profile_id/interface/addresses/dns/peer_public_key/endpoint/mtu (Profile)|wgcf.rs:61-69|String/String/Vec\<String\>/Vec\<String\>/String/String/Option\<u32\>|profile_id, id|profile_id/interface_name/ip_address/dns_server/peer_public_key/peer_endpoint/mtu|id/string/array\<CIDR\>/array\<ip\>/key/endpoint/u32|hash-only/private IPs,endpoint/private,public mtu|profile is portable config|
|revision (Status/Generate/Apply outputs)|wgcf.rs:74,113,124|u64|revision, expected_revision|revision|u64|public|optimistic concurrency|
|account/device/active_profile/kernel (Status)|wgcf.rs:75-80|WgcfAccountView/WgcfDeviceView/Option\<WgcfProfileView\>/Option\<WireGuardState\>|status|account/device/active_profile/kernel_interface|objects|private/hash-only keys|embeds wireguard state|
|accept_tos/device_model (RegisterInput)|wgcf.rs:86-87|bool/String|device_model, model|accept_tos/device_model|bool/string|public||
|license (UpdateInput)|wgcf.rs:93|Option\<String\>|license|license_key|secret string|private|`x-secret=true`; secret|
|device_name (UpdateInput)|wgcf.rs:94|Option\<String\>|device_name, name|device_name|string|private||
|endpoint (SetEndpointInput)|wgcf.rs:99|String|endpoint|peer_endpoint|string|private||
|dns/mtu (GenerateConfigInput)|wgcf.rs:107-108|Vec\<String\>/Option\<u32\>|dns|dns_server/mtu|array\<ip\>/u32|private/public||
|profile (Generate/Apply outputs)|wgcf.rs:114,127|WgcfProfileView|profile, active_profile|profile|object|private||
|kernel (ApplyConfigOutput)|wgcf.rs:130|WireGuardState|kernel|kernel_interface|WireGuardState|hash-only keys|apply receipt readback|
|kind (RefreshInput)|wgcf.rs:142|RefreshKind{account,device,all}|kind, type|refresh_kind|enum|public||
|config/status/profiles (WgcfState)|wgcf.rs:150-154|WgcfConfig/Option\<WgcfStatusOutput\>/Vec\<WgcfProfileView\>|config|config/status/profile|objects|private|note: schema test asserts `private_key`/`access_token` never projected (wgcf.rs:325-326) — no such fields declared|

## wg_opdbus (A.N.N.A. Scribe façade)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|principal_id/persona_id/email/plugin_ref (AnnaScribeServicePersona, private Rust fields, serialized)|wg_opdbus.rs:27-30|String|user_id, principal_id|principal_id/persona_id/email/plugin_ref|did string/string/email/string|private|immutable constants `did:op:service:anna-scribe`, `anna-scribe`, `anna.scribe@3tched.com` (wg_opdbus.rs:20-22); PII/identity|
|service_persona (State/Aggregate)|wg_opdbus.rs:77,100|AnnaScribeServicePersona|service_persona|service_persona|object|private|readOnly, immutable path|
|interface (WgOpdbusState)|wg_opdbus.rs:78|Option\<WireGuardInterface\>|interface|interface|WireGuardInterface|hash-only keys||
|addresses|wg_opdbus.rs:81|Vec\<String\>|addresses, allowed_ips|ip_address|array\<CIDR string\>|private||
|mtu|wg_opdbus.rs:82|Option\<u32\>|mtu|mtu|u32|public||
|admin_up|wg_opdbus.rs:83|bool|admin_up, enabled, running|admin_up|bool|public||
|interfaces (Aggregate)|wg_opdbus.rs:103|BTreeMap\<String,WgOpdbusState\>|interfaces|interface|map\<interface_name,state\>|hash-only|key = interface name|
|interface_name (Create/GetStatus inputs)|wg_opdbus.rs:109,123|String|interface_name, interface, name|interface_name|string|public|canonical form used here, unlike wireguard.rs `interface`|
|addresses/mtu/device (CreateInterfaceInput)|wg_opdbus.rs:112-117|Vec\<String\>/Option\<u32\>/WireGuardDeviceConfig|device, config|ip_address/mtu/device_config|array/u32/object|private|device embeds `private_key`+`preshared_key` (private)|
|status (CreateInterfaceOutput)|wg_opdbus.rs:128|WgOpdbusState|status|status|object|private||

## openflow — config/flows

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|bridges (OpenFlowConfig)|openflow.rs:49|Vec\<BridgeFlowConfig\>|bridges|bridge|array|public||
|observed_flows|openflow.rs:56|BTreeMap\<String,BTreeMap\<String,FlowObservation\>\>|observed_flows|observed_flow|map\<bridge,map\<flow_id,obs\>\>|public|keyed by canonical flow identity (comment)|
|controller_endpoint|openflow.rs:60|Option\<String\>|controller_endpoint, controller, endpoint|controller_endpoint|endpoint string (tcp:IP:PORT)|private||
|auto_discover_containers|openflow.rs:64|bool|auto_discover_containers|auto_discover_containers|bool|public|legacy toggle|
|enable_security_flows|openflow.rs:68|bool|enable_security_flows|enable_security_flows|bool|public|also in obfuscation plugin|
|obfuscation_level|openflow.rs:75|u8 (0-3)|obfuscation_level|obfuscation_level|u8 enum 0-3|public|duplicated in openflow_obfuscation|
|flow/installed (FlowObservation)|openflow.rs:82-83|FlowEntry/bool|flow|flow/installed|object/bool|public|observation incl. confirmed absence|
|name/flows/socket_ports (BridgeFlowConfig)|openflow.rs:98-105|String/Vec\<FlowEntry\>/Option\<Vec\<SocketPort\>\>|name, bridge|bridge_name/flow/socket_port|string/array|public||
|table (FlowEntry)|openflow.rs:113|u8 (0-254)|table|flow_table|u8|public||
|priority|openflow.rs:116|u16|priority|flow_priority|u16|public||
|match_fields|openflow.rs:127|HashMap\<String,String\>|match_fields, match_spec, match|match_field|map\<string,string\>|private| **tag-routing**: reserved keys `oscal_subid`, `identity_tag`, `service_tag` documented openflow.rs:120-125; unbounded, untyped — mis-normalized for tags|
|actions|openflow.rs:131|Vec\<FlowAction\>|actions|flow_action|array\<enum\>|public||
|cookie|openflow.rs:135|Option\<u64\>|cookie, fallback_cookie|flow_cookie|u64|public|flow identity; string form in DatapathHealth|
|idle_timeout/hard_timeout|openflow.rs:139,143|u16|idle_timeout|idle_timeout_seconds/hard_timeout_seconds|u16 s|public|0=permanent|
|FlowAction::Output{port}|openflow.rs:152|String|port, in_port, ofport|output_port|port name/number string|public||
|FlowAction::LoadRegister{register,value}|openflow.rs:155|u8/u64|register|register_index/register_value|u8/u64|public|**registers**: tagged-routing datapath register|
|FlowAction::Resubmit{table}|openflow.rs:158|u8|table|resubmit_table|u8|public||
|FlowAction::SetField{field,value}|openflow.rs:161|String/String|field|set_field_name/set_field_value|string|public|unbounded|
|FlowAction::Controller{max_len}|openflow.rs:170|Option\<u16\>|max_len|controller_max_len_bytes|u16|public||
|FlowAction::ArpResponder{mac,ip}|openflow.rs:173|String/String|mac, ip|arp_mac/arp_ip|MAC/IP string|private||
|name/port_type/ofport (SocketPort)|openflow.rs:186-193|String/SocketPortType{privacy,shared_ingress}/Option\<u16\>|name, port|socket_port_name/socket_port_type/ofport|enum/u16|public|privacy sockets `gbr_wg/gbr_xray/gbr_warp`; SharedIngress routes via tag/identity (openflow.rs:201)|

## openflow — method records

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|bridge (all inputs/outputs)|openflow.rs:1501,1508,1517,1524,1533,1539,1553,1561,1570,1579,1599|String|bridge, bridge_name, name|bridge_name|string|public||
|mode (SetFailModeInput)|openflow.rs:1510|String (`standalone`\|`secure`)|mode, fail_mode|fail_mode|enum string|public|should be enum; synonym `fail_mode` in output|
|endpoint (SetController/AttachControllerSafe)|openflow.rs:1526,1555|String|endpoint, controller_endpoint|controller_endpoint|string|private||
|fail_mode/controllers/fallback_normal/fallback_priority/fallback_cookie/connection_mode/disable_in_band (DatapathHealthOutput)|openflow.rs:1540-1546|String/Vec\<String\>/bool/u16/String/String/bool|mode, cookie|fail_mode/controller_endpoint/fallback_normal/fallback_priority/fallback_cookie/connection_mode/disable_in_band|enum/array/bool/u16/string/enum/bool|private endpoints; cookie hash-only|`fallback_cookie` String vs FlowEntry.cookie u64 — type mismatch|
|flow (AddFlowInput)|openflow.rs:1564|FlowEntry|flow|flow|FlowEntry|private (match may carry tags)|exact-member CAS|
|expected (Delete/ModifyFlowInput)|openflow.rs:1573,1582|FlowEntry|expected|expected_flow|FlowEntry|private|no wildcard delete|
|replacement (ModifyFlowInput)|openflow.rs:1585|FlowEntry|replacement|replacement_flow|FlowEntry|private|same table/priority/match identity|
|operation/before/after/verified/observed_at_unix (FlowMutationOutput)|openflow.rs:1600-1606|FlowMutationKind{added,deleted,modified}/Option\<FlowEntry\>/Option\<FlowEntry\>/bool/i64|operation, kind|operation/before_flow/after_flow/verified/observed_at_unix_seconds|enum/object/bool/i64|public; flows private|**receipt-like** provenance record; timestamp unix not RFC3339|
|success (dispatch ad-hoc JSON)|openflow.rs:1621,1627,1633,1639|bool literal `{success:true}`|success, verified, installed|success|bool|public|not a typed output struct — mis-normalized|

**OpenFlow ↔ `schema_tag_registry` binding (explicitly requested):** no `network_tag_id` field exists inside `openflow.rs`. The binding lives in `schema_tag_registry_methods.rs`: `OpenFlowBindingRef{binding_id:String, revision:u64, plugin_id:String (must =="openflow"), method_key:String ∈ {add_flow,modify_flow,delete_flow}, schema_hash:String, network_tag_id:u32 (>0)}` (schema_tag_registry_methods.rs:26-33,41-52), requested via `OpenFlowBindingRequest{binding_id, network_tag_id}` (:61-63). Invariants: `TagDescriptor.tag_id == binding.network_tag_id` (:165-167), `SchemaTagAssociation.tag_id` likewise (:80,151), and `routing_decision` enforces `tag_id == binding.network_tag_id` (routing_decision.rs:167). Canonical: `network_tag_id:u32` → keep, `schema_hash` → `schema_hash` lowercase-hex SHA-256 (**hash-only**), `binding_id` → opaque id (**hash-only**), `method_key`→`method_key` enum. So OpenFlow flows are bound to registry tags only by numeric `network_tag_id`; `FlowEntry.match_fields[identity_tag/service_tag/oscal_subid]` (openflow.rs:123-125) are reserved-but-unconsumed strings, a second, unbound tag channel.

## openflow_obfuscation

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|bridge_name (Config)|openflow_obfuscation.rs:32|String|bridge_name, bridge|bridge_name|string|public|variant vs openflow `bridge`|
|obfuscation_level/enable_security_flows|openflow_obfuscation.rs:37,42|u8/bool|obfuscation_level|obfuscation_level/enable_security_flows|u8/bool|public|duplicates OpenFlowConfig|
|privacy_ports|openflow_obfuscation.rs:47|Vec\<String\>|privacy_ports, socket_ports|privacy_port_name|array\<string\>|public||
|custom_flows|openflow_obfuscation.rs:52|Vec\<OpenFlowRule\>|custom_flows, flows|custom_flow|array|public||
|table/priority/match_spec/actions/description (OpenFlowRule)|openflow_obfuscation.rs:60-76|u8/u16/String/String/String|match_spec vs match_fields; actions String vs Vec\<FlowAction\>|flow_table/flow_priority/match_spec/action_spec/description|u8/u16/string/string/string|public/private match|**mis-normalized duplicate of FlowEntry**: match/actions as raw ovs-ofctl strings, not typed|
|bridge/flow/ttl/expected_revision (ObfuscateFlowInput)|openflow_obfuscation.rs:636-643|String/FlowEntry/u8(1-255)/u64|bridge, revision|bridge_name/flow/ttl/expected_revision|string/FlowEntry/u8/u64|public|ttl unitless (TTL value)|
|bridge/flow_id/expected_revision (DeobfuscateFlowInput)|openflow_obfuscation.rs:651-654|String/String/u64|flow_id|bridge_name/flow_id/expected_revision|string/opaque id/u64|hash-only|**receipt id** from obfuscate_flow|
|revision/flows (ObfuscationState)|openflow_obfuscation.rs:659-661|u64/BTreeMap\<String,ObfuscationRecord\>|flows|revision/flow|u64/map\<flow_id,record\>|hash-only key||
|bridge/flow/ttl (ObfuscationRecord)|openflow_obfuscation.rs:666-668|String/FlowEntry/u8|flow|bridge_name/flow/ttl|…|public||
|flow_id/revision/installed/record (ObfuscationOutput)|openflow_obfuscation.rs:673-676|String/u64/bool/ObfuscationRecord|flow_id|flow_id/revision/installed/record|id/u64/bool/object|hash-only|**receipt**|
|current_hash/desired_hash (calculate_diff, internal)|openflow_obfuscation.rs:476-477|String (md5 hex)|current_hash|current_hash/desired_hash|md5 hex string|hash-only|MD5, not SHA-256 — provenance weakness|

## ovsdb_bridge (+ ovsdb_topology projection)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|db_name (GetSchema/Transact/Monitor/MonitorCond)|ovsdb_bridge.rs:27,32,38,46|String|db_name|database_name|string|public||
|operations (TransactInput)|ovsdb_bridge.rs:33|Vec\<Value\>|operations|operation|array\<json\>|private|raw OVSDB ops, unbounded|
|monitor_id|ovsdb_bridge.rs:39,46,118,146,233|String|monitor_id, id|cancel uses `id`|monitor_id|opaque string|hash-only|`CancelInput.id` (ovsdb_bridge.rs:98) is this id — synonym|
|monitor_requests/monitor_cond_requests|ovsdb_bridge.rs:40,47|BTreeMap\<String,Vec\<OvsdbMonitorRequest\>\>|monitor_requests|monitor_request|map\<table,array\>|public||
|columns/where→where_/select (OvsdbMonitorRequest)|ovsdb_bridge.rs:56-60|Vec\<String\>/Option\<Vec\<Value\>\>/Option\<OvsdbMonitorSelect\>|where_|column_name/where/select|array/json/object|public|wire name `where`|
|initial/insert/delete/modify (OvsdbMonitorSelect)|ovsdb_bridge.rs:67-73|Option\<bool\>|initial|monitor_initial/monitor_insert/monitor_delete/monitor_modify|bool|public|OVSDB monitor select flags; `delete` shadows mutation verb|
|lock_id (Lock/Steal/Unlock/LockOutput/UnlockOutput)|ovsdb_bridge.rs:78,83,88,126,133|String|lock_id|lock_id|opaque string|hash-only||
|params (Echo)|ovsdb_bridge.rs:93,141|Vec\<Value\>|params|echo_param|array\<json\>|public||
|databases/schema/results (outputs)|ovsdb_bridge.rs:103,108,113|Vec\<String\>/Value/Vec\<Value\>|databases|database_name/schema/result|array/json|public; schema public||
|method/sequence/tables (MonitorOutput)|ovsdb_bridge.rs:119-121|String/u64/Vec\<OvsdbMonitorTable\>|method, method_key|monitor_method/sequence/table|string/u64/array|public||
|locked/queued (LockOutput); unlocked/acquired_while_queued/stolen_before_unlock (UnlockOutput)|ovsdb_bridge.rs:127-136|bool|locked|locked/queued/unlocked/acquired_while_queued/stolen_before_unlock|bool|public|lock provenance flags|
|canceled (CancelOutput)|ovsdb_bridge.rs:147|bool|canceled|cancelled|bool|public|US spelling vs `cancelled` elsewhere|
|bridge/changed (BridgeMutationOutput); bridge/port/changed (PortMutationOutput)|ovsdb_bridge.rs:152-160|String/String/bool|bridge|bridge_name/port_name/changed|string/bool|public|receipt-like|
|name/datapath_type/fail_mode/stp_enable (CreateBridgeInput)|ovsdb_bridge.rs:168-174|String/Option\<String\>/Option\<String\>/Option\<bool\>|name|bridge_name/datapath_type/fail_mode/stp_enabled|string/enum/enum/bool|public|defaults differ from BridgeConfig (non-Option there)|
|name (DeleteBridgeInput)|ovsdb_bridge.rs:182|String|name|bridge_name|string|public||
|bridge/port/interface/tag (AddPortInput)|ovsdb_bridge.rs:190-196|String/String/Option\<String\>/Option\<i32\>|tag (VLAN)|bridge_name/port_name/interface_name/vlan_tag|string/u16|public|**tag here = VLAN id**, not schema tag — collision with tag routing; i32 vs u16 in PortConfig|
|bridge/port (RemovePortInput)|ovsdb_bridge.rs:204-206|String|bridge|bridge_name/port_name|string|public||
|bridges/monitors (OvsBridgeState)|ovsdb_bridge.rs:220,226|Vec\<BridgeConfig\>/Vec\<OvsdbMonitorState\>|bridges|bridge/monitor|array|public; monitors protected (comment :221-223)||
|projection_key/monitor_id/method/sequence/active/error/tables (OvsdbMonitorState)|ovsdb_bridge.rs:232-239|String/String/String/u64/bool/Option\<String\>/Vec\<…\>|projection_key|projection_key/monitor_id/monitor_method/sequence/active/error/table|opaque/u64/bool/string|hash-only projection_key; rest public|caller-scoped|
|name/rows (MonitorTable); uuid/columns (Row); name/value (Column)|ovsdb_bridge.rs:244-257|String/Vec\<…\>/String/Vec\<…\>/String/Value|uuid|table_name/row/row_uuid/column_name/column_value|string/UUID string/json|hash-only uuid; value private|dynamic native rows|
|name/ports/datapath_type/fail_mode/stp_enable/mcast_snooping_enable/other_config (BridgeConfig)|ovsdb_bridge.rs:290-304|String/Vec\<PortConfig\>/String/Option\<String\>/bool/bool/HashMap\<String,String\>|name|bridge_name/port/datapath_type/fail_mode/stp_enabled/mcast_snooping_enabled/other_config|string/array/enum/bool/map|public; other_config private|topology reads these columns at ovsdb_topology.rs:95-103; `other_config`/`options` deliberately unobserved by topology (ovsdb_topology.rs:2-4)|
|name/interfaces/tag/trunks/vlan_mode/bond_mode (PortConfig)|ovsdb_bridge.rs:310-320|String/Vec\<InterfaceConfig\>/Option\<u16\>/Vec\<u16\>/Option\<String\>/Option\<String\>|tag|port_name/interface/vlan_tag/trunk_vlan/vlan_mode/bond_mode|string/array/u16/enum|public|topology: ovsdb_topology.rs:84-87|
|name/type→iface_type/mac_in_use/mac/admin_state/link_state/options (InterfaceConfig, ovsdb)|ovsdb_bridge.rs:326-339|String/String/Option\<String\>×4/HashMap\<String,String\>|type, iface_type vs net if_type|interface_name/interface_type/mac_in_use/mac_address/admin_state/link_state/option|string/MAC/enum/map|private MACs; options private|topology: ovsdb_topology.rs:53-59; note same struct name `InterfaceConfig` as net.rs with different fields|

`ovsdb_topology.rs` declares no records: it projects native rows (Open_vSwitch.bridges :30, Bridge.ports :35, Port.interfaces :44, name/type/mac_in_use/mac/admin_state/link_state :53-58, trunks/tag/vlan_mode/bond_mode :72-87, datapath_type/fail_mode/stp_enable/mcast_snooping_enable :95-102) into the ovsdb_bridge records above, keyed by UUID (:11,139-157).

## xray — state/methods (config types below)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|enabled/socket_port/config_path (XrayConfig)|xray.rs:21-29|bool/String/String|socket_port, tag|enabled/socket_port_name/config_path|bool/string/path string|public; config_path private|socket_port default `gbr_xray` — links to OpenFlow privacy socket|
|api/dns/policy (XrayConfig)|xray.rs:33-41|Option\<APIConfig/DNSConfig/PolicyConfig\>|api|api_config/dns_config/policy_config|objects|private (dns may leak queries)||
|inbound_protocols/outbound_protocols|xray.rs:45,49|XrayInbound/OutboundProtocolCatalog|inbound_protocols|inbound_protocol_catalog/outbound_protocol_catalog|objects|private|reference catalog, see below|
|software/version/dependencies/oscal_source/config/running/tools (XrayState)|xray.rs:75-99|String/String/Vec\<String\>/Option\<String\>/XrayConfig/bool/Value|version|software/version/dependency/oscal_source/config/running/tool|string/array/path/object/bool/json|public; tools private|`oscal_source` is a D-Bus path string, provenance pointer|
|name/value (GetStatsOutput); name (StatsInput)|xray.rs:486-487,638|String/i64/String|name|stat_name/stat_value|string/i64|public||
|inbound_tag (Add/RemoveUser input+output)|xray.rs:548,557,567,576|String (1-256)|inbound_tag, tag|inbound_tag|string tag|hash-only|**Xray routing tag**, distinct from schema/OpenFlow tags|
|email (HandlerUser/Add/Remove)|xray.rs:538,558,569,577|String (1-320)|email, user_id|user_email|email string|private|user key, PII|
|level (HandlerUser/AddUserOutput)|xray.rs:540,560|u32|level, userLevel, user_level|user_level|u32|public||
|account (HandlerUser)|xray.rs:541|XrayHandlerAccount|account, users, clients|account|tagged enum|private|see next row|
|XrayHandlerAccount::Vless{id,flow} / Vmess{id} / Trojan{password}|xray.rs:519-529|String(≤64)/XrayVlessFlow{``,xtls-rprx-vision}/String/String(≤4096)|id|account_id/flow/password|UUID string/enum/secret|private|UUID is credential-equivalent; Trojan password **secret (not marked x-secret)**|
|tag/listen_ip/port/protocol/users/transport (AddInboundInput)|xray.rs:611-620|String/String(≤45 IP)/u16/XrayHandlerProtocol/Vec\<HandlerUser\>/XrayInboundTransport|tag|inbound_tag/listen_ip/port/protocol/user/transport|string/ip/u16/enum/array/object|hash-only tag; users private|`tag` synonym of inbound_tag|
|certificate_pem/private_key_pem/alpn (Transport::Tls)|xray.rs:590-596|String(≤262144)/String(≤65536)/Vec\<String\>|certificate_pem|certificate_pem/private_key_pem/alpn|PEM string|private|**secrets: private_key_pem private; cert hash-only at most**; large unbounded PEM strings|
|tag/listen_ip/port/protocol/user_count/security (AddInboundOutput)|xray.rs:628-633|String/String/u16/XrayHandlerProtocol/u32/XrayInboundSecurity{loopback_plaintext,tls}|inbound_tag/…/user_count/security|…|public; tag hash-only|receipt-like|

## xray_config_types — per-record field lists (all `String=u` unbounded unless noted; `password/pass/id/email` = **private** in every record)

Records (source = struct line; fields as declared, serde wire-name in «» where renamed):
`APIConfig` xray_config_types.rs:20 `tag,listen,services:Vec<String>` → canonical `tag/listen/services`, public/private listen. `NameServerConfig` :31 `address,client_ip«clientIp»,port:u16,skip_fallback«skipFallback»,domains,expected_ips«expectedIPs»,expect_ips«expectIPs» (duplicate synonym pair!),query_strategy«queryStrategy»,tag,timeout_ms«timeoutMs»:u64,disable_cache«disableCache»,serve_stale«serveStale»,serve_expired_ttl«serveExpiredTTL»:u32,final_query«finalQuery»,unexpected_ips«unexpectedIPs»` → canonical snake_case, IPs/domains private. `DNSConfig` :77 `servers:Vec<Option<NameServerConfig>>,hosts:Value,client_ip«clientIp»,tag,query_strategy,disable_cache,serve_stale,serve_expired_ttl,disable_fallback,disable_fallback_if_match,enable_parallel_query,use_system_hosts` (bools «camelCase»). `PolicyConfig` :115 `levels:HashMap<u32,Option<Value>>,system:Option<Value>` — unbounded JSON. `DokodemoConfig` :126 `allowed_network«allowedNetwork»:Value,rewrite_address«rewriteAddress»,rewrite_port«rewritePort»:u16,network:Value,address,port:u16,port_map«portMap»:HashMap<String,String>,follow_redirect«followRedirect»,user_level«userLevel»:u32`. `HTTPAccount` :155 `username«user»,password«pass»` **private**. `HTTPServerConfig` :166 `users,accounts (synonym pair) :Vec<Option<HTTPAccount>>,transparent«allowTransparent»,user_level`. `HTTPRemoteConfig` :181 `address,port,users:Vec<Value>`. `HTTPClientConfig` :192 `address,port,level:u32,email,username«user»,password«pass»,servers,headers:HashMap<String,String>` **private creds**. `SocksAccount` :215 = HTTPAccount shape. `SocksServerConfig` :226 `auth_method«auth»,users,accounts,udp,host«ip»,user_level`. `SocksRemoteConfig` :246, `SocksClientConfig` :257 (like HTTPClient). `VMessAccount` :278 `id,security,experiments` **private id**. `VMessDefaultConfig` :289 `level:u8`. `VMessInboundConfig` :296 `users,clients (synonyms):Vec<Value>,defaults«default»`. `VMessOutboundTarget` :308 / `VMessOutboundConfig` :319 `…,receivers«vnext»`. `VLessInboundFallback` :341 `name,alpn,path,kind«type»,dest:Value,xver:u64`; `VLessInboundConfig` :359 `users,clients,decryption,fallbacks,flow,testseed:Vec<u32>`; `VLessReverseConfig` :376 `tag,sniffing:Value`; `VLessOutboundVnext` :385; `VLessOutboundConfig` :396 `address,port,level,email,id,flow,seed,encryption,reverse,testpre:u32,testseed` (**seed/id private**). `TrojanServerTarget` :425 / `TrojanClientConfig` :442 / `TrojanUserConfig` :479 `password,flow,email,level` **private**; `TrojanInboundFallback` :461 = VLess fallback shape; `TrojanServerConfig` :492 `users,clients,fallbacks`. `ShadowsocksUserConfig` :503 `cipher«method»,password,level,email,address,port`; `ShadowsocksServerConfig` :521 (+`users,clients,network_list«network»:Value`); `ShadowsocksServerTarget` :542; `ShadowsocksClientConfig` :560 (+`servers`) — all passwords **private**, cipher should be enum. `FreedomConfig` :580 `target_strategy«targetStrategy»,domain_strategy«domainStrategy»,redirect,user_level,fragment:Fragment,noise:Noise,noises,proxy_protocol«proxyProtocol»:u32,ips_blocked«ipsBlocked»,final_rules«finalRules»`; `Fragment` :611 `packets,length:Value,interval:Value,max_split«maxSplit»:Value`; `Noise` :625 `kind«type»,packet,delay:Value,apply_to«applyTo»`; `FreedomFinalRuleConfig` :640 `action,network:Value,port:Value,ip,block_delay«blockDelay»:Value` (action/port should be enums); `BlackholeConfig` :656 `response:Value`. `Policy` :663 `handshake,connection_idle«connIdle»,uplink_only,downlink_only,stats_user_uplink/downlink/online,buffer_size:i32` (u32 timeouts, units implicit → `*_seconds`); `SystemPolicy` :691 four `stats_*` bools. Catalogs: `XrayInboundProtocolCatalog` :710-775 and `XrayOutboundProtocolCatalog` :781-846 each have one `Option<…Config/Account/…>` field per struct above (field names = snake struct names, e.g. `dokodemo_config`, `http_account`, … `shadowsocks_client_config`) — canonical `*_config` object, sensitivity inherits embedded secrets (**private** wherever a password/id-bearing struct is present). Mis-normalization in this file is systematic: camelCase serde wire names vs snake_case Rust, `user/pass/ip/auth/method/default/vnext` terse wire names, `users`/`accounts` and `users`/`clients` and `expectedIPs`/`expectIPs` duplicate synonym fields, many `serde_json::Value` escape hatches.

## proxy_server

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|revision (State/MethodOutput)|proxy_server.rs:14,44|u64|revision, expected_revision|revision|u64|public||
|proxies (State)|proxy_server.rs:16|BTreeMap\<String,ProxyStatus\>|proxies|proxy|map\<proxy_name,status\>|public|key = name|
|enabled/bind_address/port/upstream (ProxyConfig)|proxy_server.rs:23-29|bool/String/String-IP/u16/String|bind_address, bound_address|enabled/bind_address/port/upstream_endpoint|bool/ip/u16/ip:port string|private upstream/bind|upstream fixed IP:port, no creds (comment :80)|
|config/running/bound_address (ProxyStatus)|proxy_server.rs:35-37|ProxyConfig/bool/Option\<String\>|bound_address|config/running/bound_address|object/bool/string|private||
|name/status (ProxyMethodOutput)|proxy_server.rs:43-45|String/ProxyStatus|name|proxy_name/status|string/object|public|receipt-like|
|name/port/expected_revision (StartProxyInput)|proxy_server.rs:53-56|String/Option\<u16\>/u64|name|proxy_name/port/expected_revision|string/u16/u64|public||
|name/expected_revision (StopProxyInput)|proxy_server.rs:63-65|String/u64|name|proxy_name/expected_revision|…|public||
|name/enabled/bind_address/port/upstream/expected_revision (ConfigureProxyInput)|proxy_server.rs:73-81|String/Option\<bool\>/Option\<String\>/Option\<u16\>/Option\<String\>/u64|name|proxy_name/enabled/bind_address/port/upstream_endpoint/expected_revision|…|private upstream||

## dnsresolver

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|version/items (DnsState)|dnsresolver.rs:17-19|u32/Vec\<DnsItem\>|version|schema_version/item|u32/array|public||
|id/mode/servers/search/options (DnsItem)|dnsresolver.rs:35-49|String/Mode{enforce,observe-only kebab}/Vec\<String\>/Option\<Vec\<String\>\>/Option\<Vec\<String\>\>|id|item_id/mode/dns_server/search_domain/dns_option|opaque/enum/array\<ip\>/array|hash-only id; servers private|`mode` here vs openflow fail-mode|
|ifindex (all link/resolve records)|dnsresolver.rs:278,284,289,571,579,587,596,607,612,…|i32|ifindex|interface_index|i32|public|numeric interface id — synonym of interface_name family|
|family|dnsresolver.rs:279,580,601|i32|family|address_family|i32 enum (AF_INET/6)|public|raw int, should be enum|
|address (DnsAddress/ResolveAddressInput)|dnsresolver.rs:280,581|Vec\<u8\>|address|ip_address_bytes|byte array|private|binary IP, inconsistent with string IPs elsewhere|
|name (DnsName/inputs)|dnsresolver.rs:285,572,588,597|String|name|hostname|string|private|queried names = provenance|
|class/record_type/data (DnsRecord)|dnsresolver.rs:290-292|u16/u16/Vec\<u8\>|class, type|dns_class/record_type/record_data|u16/enum/bytes|private||
|priority/weight/port/hostname/addresses/canonical (DnsServiceRecord)|dnsresolver.rs:296-301|u16/u16/u16/String/Vec\<DnsAddress\>/String|canonical, canonical_name|srv_priority/srv_weight/port/hostname/address/canonical_name|u16/string|private||
|addresses/canonical/flags (ResolveHostnameOutput)|dnsresolver.rs:306-308|Vec\<DnsAddress\>/String/u64|canonical|address/canonical_name/flags|array/string/u64 bitmask|private|flags undocumented bitmask everywhere (:308,313,318,327,574,582,591,602)|
|names/flags; records/flags; srv_data/txt_data/canonical_name/canonical_type/canonical_domain/flags|dnsresolver.rs:312-327|…|canonical vs canonical_name/type/domain|names/record/srv_record/txt_record/canonical_name/canonical_type/canonical_domain/flags|…|private|`canonical*` three-way synonym split|
|path (LinkOutput); disposition (DnsMutationOutput); service_path (ServiceRegistrationOutput)|dnsresolver.rs:331,335,339|String|path, service_path|link_path/disposition/service_path|D-Bus path/enum string/opaque|hash-only service_path; disposition unbounded string should be enum|receipt-like|
|name/family/flags etc. (Resolve*Inputs)|dnsresolver.rs:570-603|as outputs|type→service_type (:598-599)|hostname/address_family/flags/dns_class/record_type/service_type/domain|…|private||
|addresses (SetLinkDNSInput)|dnsresolver.rs:613|Vec\<(i32,Vec\<u8\>)\>|addresses|dns_server_address|(family,bytes) tuples|private|tuple-typed, no named fields — mis-normalized|
|addresses (SetLinkDNSExInput)|dnsresolver.rs:619|Vec\<(i32,Vec\<u8\>,u16,String)\>|addresses|dns_server_address_ex|(family,bytes,port,server_name) tuples|private|4-tuple, last String likely SNI/server name — unnamed|
|domains (SetLinkDomainsInput)|dnsresolver.rs:625|Vec\<(String,bool)\>|domains|search_domain|(domain,routing_only) tuples|private|bool meaning unnamed|
|enable (SetLinkDefaultRouteInput)|dnsresolver.rs:631|bool|enable, enabled|default_route_enabled|bool|public||
|mode (SetLinkLLMNR/MulticastDNS/DNSOverTLS/DNSSEC)|dnsresolver.rs:637,643,649,655|String|mode|llmnr_mode/mdns_mode/dot_mode/dnssec_mode|enum string|public|four same-named `mode:String`, should be enums|
|names (SetLinkDNSSECNegativeTrustAnchorsInput)|dnsresolver.rs:661|Vec\<String\>|names|negative_trust_anchor_name|array\<string\>|public||
|id/name_template/type_/port/priority/weight/txt (RegisterServiceInput)|dnsresolver.rs:671-677|String/String/String/u16/u16/u16/Vec\<BTreeMap\<String,Vec\<u8\>\>\>|type_|service_id/name_template/service_type/port/priority/weight/txt_record|opaque/string/enum/u16/map|hash-only id; txt private|`type_` Rust-escaped, wire name differs|
|service_path (UnregisterServiceInput)|dnsresolver.rs:682|String|service_path, path|service_path|opaque path|hash-only||
|(ResetStatistics/FlushCaches/ResetServerFeatures inputs)|dnsresolver.rs:686,689,692|empty structs|—|—|unit|public||

## privacy_routes

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|routes (State/ListRoutesOutput)|privacy_routes.rs:19,436|Vec\<PrivacyRoute\>|routes|route|array|private||
|name|privacy_routes.rs:24|String|name|route_name|string (in example = hex hash, :484)|hash-only|example shows name is actually a hash — mis-normalized name|
|route_id|privacy_routes.rs:25,440|String|route_id, id|route_id|opaque/hash string|hash-only|schema desc: “Derived route ID from WireGuard public key and shared secret” (:274) — hash-only, never raw secret (no secret field declared)|
|user_id|privacy_routes.rs:26|String|user_id, principal_id|user_id|UUID string|private|example UUID :486|
|email|privacy_routes.rs:27|String|email|user_email|email|private|PII|
|wireguard_public_key|privacy_routes.rs:28|String|wireguard_public_key, public_key|wireguard_public_key|base64 X25519|hash-only|route keyed by WG identity (schema :471)|
|assigned_ip|privacy_routes.rs:29|String CIDR (`10.100.0.2/32`)|assigned_ip, addresses, allowed_ips|assigned_ip_prefix|CIDR string|private||
|selector_ip|privacy_routes.rs:30|String (bare IP)|selector_ip|selector_ip|IP string|private|same host as assigned_ip without /32 — duplicate representation|
|container_name|privacy_routes.rs:32|Option\<String\>|container_name|container_name|string|private||
|ingress_port|privacy_routes.rs:33|String (port *name* e.g. `ovsbr0-sock`)|ingress_port, socket_port, port|ingress_socket_port_name|string|public|not a number — links to OpenFlow SocketPort SharedIngress|
|next_hop|privacy_routes.rs:34|String (default `gbr_wg`)|next_hop|next_hop_socket_port_name|string|public|first hop in GhostBridge chain|
|enabled|privacy_routes.rs:35|bool|enabled|enabled|bool|public||
|created_at/updated_at|privacy_routes.rs:36-37|String RFC3339 example|created_at, created/updated (wgcf)|created_at/updated_at|RFC3339 UTC string|private|correct format here; wgcf diverges|
|route_id (DeleteRouteInput)|privacy_routes.rs:440|String|route_id|route_id|opaque|hash-only|only mutation besides state diff; methods list_routes/delete_route (:445-464)|

## (a) Duplicate / synonym fields across this slice

- Interface identity: `name` (net.rs:44, ovsdb_bridge.rs:290/310/326, openflow bridge `name` :98), `interface` (wireguard.rs:90…), `interface_name` (wg_opdbus.rs:109), `ifindex` (dnsresolver.rs:278…), socket/port names (openflow.rs:186).
- Interface type: wire `type`→`if_type` (net.rs:47), `interface_type` (net.rs:958), `iface_type` wire `type` (ovsdb_bridge.rs:328-329), `device_type` wire `type` (wgcf.rs:51-52), `kind«type»` (xray_config_types.rs:350).
- Bridge identity: `name` (net.rs:992, ovsdb_bridge.rs:168/182/290), `bridge` (openflow.rs:1501…, ovsdb_bridge.rs:190), `bridge_name` (openflow_obfuscation.rs:32).
- IP address: `ip`+`prefix` (net.rs:211), `address`/`addresses` (net.rs:166, wgcf.rs:63, wg_opdbus.rs:81), `ipv4_addresses` (net.rs:1001), `allowed_ips` (wireguard.rs:48), `assigned_ip`/`selector_ip` (privacy_routes.rs:29-30), binary `Vec<u8>` (dnsresolver.rs:280).
- Gateway: `gateway` (net.rs:174,978) vs `default_gateway` (net.rs:1002).
- DNS servers: `dns` (net.rs:182, wgcf.rs:66) vs `servers` (dnsresolver.rs:41) vs xray `DNSConfig.servers` (xray_config_types.rs:79).
- Public key: `public_key` (wireguard.rs:33/45, wgcf.rs:56), `peer_public_key` (wgcf.rs:67), `wireguard_public_key` (privacy_routes.rs:28).
- Endpoint: `endpoint` (wireguard.rs:49, wgcf.rs:68/99, openflow.rs:1526), `controller_endpoint` (openflow.rs:60), `upstream` (proxy_server.rs:29), `listen_ip`+`port` (xray.rs:614-616).
- Tag: VLAN `tag:Option<i32/u16>` (ovsdb_bridge.rs:196,314), Xray `tag`/`inbound_tag` (xray.rs:548,611), OpenFlow reserved `identity_tag`/`service_tag`/`oscal_subid` match keys (openflow.rs:123-125), registry `network_tag_id`/`tag_id` (schema_tag_registry_methods.rs:33,80) — four unrelated “tag” namespaces.
- Revision: `revision`/`expected_revision` (wgcf, proxy_server, openflow_obfuscation), flow `cookie` (openflow.rs:135) as de-facto version.
- Timestamps: `created_at/updated_at` (privacy_routes.rs:36), `created/updated` (wgcf.rs:41), `latest_handshake_unix`/`observed_at_unix` (wireguard.rs:51, openflow.rs:1606).
- `users`/`accounts`, `users`/`clients`, `expected_ips`/`expect_ips` inside xray_config_types (see above); `mode`: fail-mode (openflow.rs:1510), DnsItem mode (dnsresolver.rs:38), four SetLink* `mode:String` (dnsresolver.rs:637-655), `connection_mode` (openflow.rs:1545).

## (b) Mis-normalized fields

- Mixed case conventions: snake_case Rust vs camelCase serde wire names throughout xray_config_types (`clientIp`, `skipFallback`, `userLevel`, «pass», «ip», «vnext», «default»); kebab-case enums in net/dnsresolver (`ovs-bridge`, `observe-only`) vs snake_case elsewhere.
- Ambiguous names: bare `id` (wgcf.rs:33,47; dnsresolver RegisterService :671; ovsdb CancelInput.id :98), `name` used for bridges/interfaces/proxies/stats/hostnames, `type`, `mode`, `status`, `config`, `kernel` (wgcf.rs:80), `flow` vs `expected`/`replacement`.
- Unbounded strings where id/hash/enum belongs: `match_fields:HashMap<String,String>` incl. tag keys (openflow.rs:127); `link_state`, `operations_completed`, `disposition`, `connection_mode`, all `mode:String`/`fail_mode:String` (should be enums); `schema_hash`, `route_id`, `flow_id`, `fallback_cookie:String` vs `cookie:u64`; PEM blobs (xray.rs:590-592); `properties/options/other_config` open maps (net.rs:123, ovsdb_bridge.rs:304,339); `operations:Vec<Value>` (ovsdb_bridge.rs:33); tuple-typed DNS inputs (dnsresolver.rs:613,619,625); `OpenFlowRule.match_spec/actions:String` duplicating typed FlowEntry (openflow_obfuscation.rs:68-72); f64 referral/quota counts (wgcf.rs:37-40); ad-hoc `{success:true}` outputs (openflow.rs:1621).
- Units implicit: `persistent_keepalive`, `idle/hard_timeout`, `timeout_ms` (explicit, good), `latest_handshake_unix`, `ttl` (is a TTL *value*, not seconds).

## (c) Tagged routing / provenance relevance

- Registry↔OpenFlow: `network_tag_id:u32` binding detailed under openflow above; OpenFlow itself carries no registry id — routing must join via `schema_tag_registry` (`OpenFlowBindingRef.schema_hash` hash-only, `binding_id` hash-only).
- Reserved-but-inert FlowEntry match keys `oscal_subid`/`identity_tag`/`service_tag` (openflow.rs:120-127, test :1688-1718) are the future tagged-routing channel; live path is fallback NORMAL. `LoadRegister{register,value}` (openflow.rs:155) and `SocketPortType::SharedIngress` “routing many privacy routes via tag/identity” (openflow.rs:201) are the datapath tag/register mechanisms.
- Privacy routing chain: `privacy_routes.wireguard_public_key + selector_ip → ingress_port (ovsbr0-sock) → next_hop (gbr_wg)` (privacy_routes.rs:28-34) joins wireguard peers, OpenFlow socket ports, and xray `socket_port=gbr_xray` (xray.rs:25,56).
- Receipts/provenance: `FlowMutationOutput{before,after,verified,observed_at_unix}` (openflow.rs:1598-1606), `ObfuscationOutput.flow_id` (openflow_obfuscation.rs:672), `ProxyMethodOutput{revision,status}`, `InterfaceApplyOutput.operations_completed`, monitor `sequence` (ovsdb_bridge.rs:120) — none carry `operation_id`/`event_hash`, so they cannot yet point into the event-chain receipt machinery without an added hash-only receipt ref. MD5 `current/desired_hash` (openflow_obfuscation.rs:476) should be SHA-256.
- **All secret fields flagged private above**: wireguard `private_key`/`preshared_key`, wgcf `license`, wg_opdbus-embedded device keys, xray Trojan `password`, TLS `private_key_pem` (+ account UUIDs), and every `password` in xray_config_types HTTP/Socks/VMess/VLess/Trojan/Shadowsocks records.