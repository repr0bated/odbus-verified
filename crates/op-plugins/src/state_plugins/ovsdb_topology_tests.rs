use super::*;
use serde_json::json;

const ROOT: &str = "00000000-0000-0000-0000-000000000001";
const BRIDGE: &str = "00000000-0000-0000-0000-000000000002";
const PORT: &str = "00000000-0000-0000-0000-000000000003";
const INTERFACE: &str = "00000000-0000-0000-0000-000000000004";
const OTHER_INTERFACE: &str = "00000000-0000-0000-0000-000000000005";

fn snapshot() -> OvsdbSnapshot {
    let rows = json!({
        "Open_vSwitch":{(ROOT):{"bridges":["uuid",BRIDGE]}},
        "Bridge":{(BRIDGE):{"name":"br-fixture","ports":["uuid",PORT],"datapath_type":"system","fail_mode":["set",["secure"]],"stp_enable":false,"mcast_snooping_enable":true}},
        "Port":{(PORT):{"name":"bond-fixture","interfaces":["set",[["uuid",OTHER_INTERFACE],["uuid",INTERFACE]]],"tag":0,"trunks":["set",[100,0]],"vlan_mode":"trunk","bond_mode":"active-backup"}},
        "Interface":{
            (INTERFACE):{"name":"eth-a","type":"system","mac_in_use":["set",["00:11:22:33:44:55"]],"mac":["set",[]],"admin_state":"up","link_state":"down"},
            (OTHER_INTERFACE):{"name":"eth-b","type":"system","mac_in_use":["set",[]],"mac":["set",[]],"admin_state":"down","link_state":"up"}
        }
    });
    OvsdbSnapshot {
        sequence: 1,
        tables: serde_json::from_value(rows).unwrap(),
    }
}
fn row<'a>(snapshot: &'a mut OvsdbSnapshot, table: &str, id: &str) -> &'a mut OvsdbRow {
    snapshot
        .tables
        .get_mut(table)
        .unwrap()
        .get_mut(&id.parse::<Uuid>().unwrap())
        .unwrap()
}

#[test]
fn actual_uuid_hierarchy_preserves_bonds_optional_scalars_zero_and_false() {
    let state = project_topology(&snapshot()).unwrap();
    let bridge = &state.bridges[0];
    assert_eq!(bridge.name, "br-fixture");
    assert!(!bridge.stp_enable);
    assert_eq!(bridge.fail_mode.as_deref(), Some("secure"));
    let port = &bridge.ports[0];
    assert_eq!(port.name, "bond-fixture");
    assert_eq!(port.tag, Some(0));
    assert_eq!(port.trunks, [0, 100]);
    assert_eq!(
        port.interfaces
            .iter()
            .map(|interface| interface.name.as_str())
            .collect::<Vec<_>>(),
        ["eth-a", "eth-b"]
    );
    assert_eq!(port.interfaces[0].link_state.as_deref(), Some("down"));
    assert_eq!(port.interfaces[0].mac, None);
    let serialized = serde_json::to_value(&state).unwrap();
    assert!(serialized["bridges"][0].get("other_config").is_none());
    assert!(
        serialized["bridges"][0]["ports"][0]["interfaces"][0]
            .get("options")
            .is_none()
    );
    let schema = super::super::ovsdb_bridge_schema();
    assert!(
        schema
            .validate(&simd_json::serde::to_owned_value(&state).unwrap())
            .valid
    );
    assert!(schema.validate_public_state_policy().is_ok());
}

#[test]
fn whole_snapshot_replacement_removes_actual_children_including_last_bridge() {
    let mut source = snapshot();
    let old = project_topology(&source).unwrap();
    row(&mut source, "Port", PORT).insert("interfaces".into(), json!(["uuid", INTERFACE]));
    source
        .tables
        .get_mut("Interface")
        .unwrap()
        .remove(&OTHER_INTERFACE.parse::<Uuid>().unwrap());
    let current = project_topology(&source).unwrap();
    assert_eq!(old.bridges[0].ports[0].interfaces.len(), 2);
    assert_eq!(current.bridges[0].ports[0].interfaces.len(), 1);
    row(&mut source, "Bridge", BRIDGE).insert("ports".into(), json!(["set", []]));
    source.tables.get_mut("Port").unwrap().clear();
    source.tables.get_mut("Interface").unwrap().clear();
    assert!(
        project_topology(&source).unwrap().bridges[0]
            .ports
            .is_empty()
    );
    row(&mut source, "Open_vSwitch", ROOT).insert("bridges".into(), json!(["set", []]));
    source.tables.get_mut("Bridge").unwrap().clear();
    assert!(project_topology(&source).unwrap().bridges.is_empty());
}

#[test]
fn unresolved_or_duplicate_refs_and_invalid_native_types_never_become_empty_topology() {
    let mut source = snapshot();
    source.tables.get_mut("Interface").unwrap().clear();
    assert!(project_topology(&source).is_err());
    let mut source = snapshot();
    row(&mut source, "Bridge", BRIDGE).insert(
        "ports".into(),
        json!(["set", [["uuid", PORT], ["uuid", PORT]]]),
    );
    assert!(project_topology(&source).is_err());
    let mut source = snapshot();
    row(&mut source, "Bridge", BRIDGE).insert("stp_enable".into(), json!("false"));
    assert!(project_topology(&source).is_err());
    for invalid in [json!(-1), json!(4096), json!(["set", [1, 2]]), json!("1")] {
        let mut source = snapshot();
        row(&mut source, "Port", PORT).insert("tag".into(), invalid);
        assert!(project_topology(&source).is_err());
    }
    let mut source = snapshot();
    source.tables.get_mut("Open_vSwitch").unwrap().clear();
    assert!(project_topology(&source).is_err());
}

#[test]
fn public_policy_keeps_nested_topology_and_scrubs_opaque_configuration() {
    let state = project_topology(&snapshot()).unwrap();
    let mut value = serde_json::to_value(&state).unwrap();
    value["bridges"][0]["other_config"] = json!({"opaque":"private"});
    value["bridges"][0]["ports"][0]["interfaces"][0]["options"] = json!({"password":"hidden"});
    let value = simd_json::serde::to_owned_value(value).unwrap();
    let projected = super::super::ovsdb_bridge_schema()
        .project_public_state(&value)
        .unwrap()
        .unwrap();
    let encoded = simd_json::to_string(&projected).unwrap();
    assert!(
        !encoded.contains("private")
            && !encoded.contains("hidden")
            && !encoded.contains("options")
            && !encoded.contains("other_config")
    );
    assert_eq!(
        projected["bridges"][0]["ports"][0]["interfaces"][0]["name"],
        simd_json::json!("eth-a")
    );
    assert_eq!(
        projected["bridges"][0]["stp_enable"],
        simd_json::json!(false)
    );
}
