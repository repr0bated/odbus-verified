//! Unit tests for OpenFlow actions.

use super::*;
use super::types::NxActionSubtype;
use crate::match_fields::MacAddr;
use crate::oxm::OxmField;

#[test]
fn action_type_wire_values() {
    assert_eq!(ActionType::Output as u16, 0);
    assert_eq!(ActionType::PushVlan as u16, 17);
    assert_eq!(ActionType::PopVlan as u16, 18);
    assert_eq!(ActionType::Group as u16, 22);
    assert_eq!(ActionType::DecNwTtl as u16, 24);
    assert_eq!(ActionType::SetField as u16, 25);
    assert_eq!(ActionType::Experimenter as u16, 0xffff);
}

#[test]
fn nx_action_subtype_values() {
    assert_eq!(NxActionSubtype::Resubmit as u16, 1);
    assert_eq!(NxActionSubtype::Move as u16, 6);
    assert_eq!(NxActionSubtype::RegLoad as u16, 7);
    assert_eq!(NxActionSubtype::ResubmitTable as u16, 14);
    assert_eq!(NxActionSubtype::Ct as u16, 35);
}

#[test]
fn encode_output_port_1() {
    let bytes = encode_output(1, 0xffff);
    assert_eq!(bytes.len(), 16);
    // type = 0
    assert_eq!(&bytes[0..2], &[0x00, 0x00]);
    // length = 16
    assert_eq!(&bytes[2..4], &[0x00, 0x10]);
    // port = 1
    assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x01]);
    // max_len = 0xffff
    assert_eq!(&bytes[8..10], &[0xff, 0xff]);
    // padding
    assert_eq!(&bytes[10..16], &[0, 0, 0, 0, 0, 0]);
}

#[test]
fn encode_output_controller() {
    let bytes = encode_output(port::CONTROLLER, 128);
    assert_eq!(bytes.len(), 16);
    // port = CONTROLLER (0xfffffffd)
    assert_eq!(&bytes[4..8], &[0xff, 0xff, 0xff, 0xfd]);
    // max_len = 128
    assert_eq!(&bytes[8..10], &[0x00, 0x80]);
}

#[test]
fn encode_pop_vlan_action() {
    let bytes = encode_pop_vlan();
    assert_eq!(bytes.len(), 8);
    // type = 18
    assert_eq!(&bytes[0..2], &[0x00, 0x12]);
    // length = 8
    assert_eq!(&bytes[2..4], &[0x00, 0x08]);
    // padding
    assert_eq!(&bytes[4..8], &[0, 0, 0, 0]);
}

#[test]
fn encode_push_vlan_8021q() {
    let bytes = encode_push_vlan(0x8100);
    assert_eq!(bytes.len(), 8);
    // type = 17
    assert_eq!(&bytes[0..2], &[0x00, 0x11]);
    // length = 8
    assert_eq!(&bytes[2..4], &[0x00, 0x08]);
    // ethertype = 0x8100
    assert_eq!(&bytes[4..6], &[0x81, 0x00]);
    // padding
    assert_eq!(&bytes[6..8], &[0, 0]);
}

#[test]
fn encode_dec_ttl_action() {
    let bytes = encode_dec_ttl();
    assert_eq!(bytes.len(), 8);
    // type = 24
    assert_eq!(&bytes[0..2], &[0x00, 0x18]);
    // length = 8
    assert_eq!(&bytes[2..4], &[0x00, 0x08]);
}

#[test]
fn encode_set_nw_ttl_action() {
    let bytes = encode_set_nw_ttl(64);
    assert_eq!(bytes.len(), 8);
    // type = 23
    assert_eq!(&bytes[0..2], &[0x00, 0x17]);
    // length = 8
    assert_eq!(&bytes[2..4], &[0x00, 0x08]);
    // ttl = 64
    assert_eq!(bytes[4], 64);
}

#[test]
fn encode_group_action() {
    let bytes = encode_group(100);
    assert_eq!(bytes.len(), 8);
    // type = 22
    assert_eq!(&bytes[0..2], &[0x00, 0x16]);
    // length = 8
    assert_eq!(&bytes[2..4], &[0x00, 0x08]);
    // group_id = 100
    assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x64]);
}

#[test]
fn encode_set_field_mac_eth_dst() {
    let mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
    let bytes = encode_set_field_mac(OxmField::EthDst, mac);
    assert_eq!(bytes.len(), 16);
    // type = 25 (SetField)
    assert_eq!(&bytes[0..2], &[0x00, 0x19]);
    // length = 16
    assert_eq!(&bytes[2..4], &[0x00, 0x10]);
    // OXM header: class=0x8000, field=3 (EthDst), has_mask=0, length=6
    // = 0x8000_0606 = (0x8000 << 16) | (3 << 9) | 6
    let expected_oxm: u32 = (0x8000 << 16) | (3 << 9) | 6;
    assert_eq!(
        &bytes[4..8],
        &expected_oxm.to_be_bytes(),
        "OXM header mismatch"
    );
    // MAC address
    assert_eq!(&bytes[8..14], &mac);
    // padding
    assert_eq!(&bytes[14..16], &[0, 0]);
}

#[test]
fn encode_set_field_u32_ipv4_dst() {
    let addr: u32 = 0x0a000001; // 10.0.0.1
    let bytes = encode_set_field_u32(OxmField::Ipv4Dst, addr);
    assert_eq!(bytes.len(), 16);
    // type = 25 (SetField)
    assert_eq!(&bytes[0..2], &[0x00, 0x19]);
    // OXM header: class=0x8000, field=12 (Ipv4Dst), has_mask=0, length=4
    let expected_oxm: u32 = (0x8000 << 16) | (12 << 9) | 4;
    assert_eq!(&bytes[4..8], &expected_oxm.to_be_bytes());
    // IPv4 address
    assert_eq!(&bytes[8..12], &[0x0a, 0x00, 0x00, 0x01]);
}

#[test]
fn encode_set_field_u16_vlan_vid() {
    // VLAN VID has CFI bit set (0x1000)
    let vid = 100 | 0x1000;
    let bytes = encode_set_field_u16(OxmField::VlanVid, vid);
    assert_eq!(bytes.len(), 16);
    // OXM header: class=0x8000, field=6 (VlanVid), has_mask=0, length=2
    let expected_oxm: u32 = (0x8000 << 16) | (6 << 9) | 2;
    assert_eq!(&bytes[4..8], &expected_oxm.to_be_bytes());
    // VLAN VID with CFI
    assert_eq!(&bytes[8..10], &[0x10, 0x64]);
}

#[test]
fn encode_nx_resubmit_table() {
    let bytes = nicira::encode_nx_resubmit(None, Some(10));
    assert_eq!(bytes.len(), 16);
    // type = 0xffff (Experimenter)
    assert_eq!(&bytes[0..2], &[0xff, 0xff]);
    // length = 16
    assert_eq!(&bytes[2..4], &[0x00, 0x10]);
    // vendor = NICIRA (0x00002320)
    assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x23, 0x20]);
    // subtype = 14 (ResubmitTable)
    assert_eq!(&bytes[8..10], &[0x00, 0x0e]);
    // in_port = 0xfff8 (IN_PORT)
    assert_eq!(&bytes[10..12], &[0xff, 0xf8]);
    // table = 10
    assert_eq!(bytes[12], 10);
}

#[test]
fn encode_nx_ct_action() {
    let bytes = nicira::encode_nx_ct(0x01, 100, Some(5));
    assert_eq!(bytes.len(), 24);
    // type = 0xffff (Experimenter)
    assert_eq!(&bytes[0..2], &[0xff, 0xff]);
    // length = 24
    assert_eq!(&bytes[2..4], &[0x00, 0x18]);
    // vendor = NICIRA
    assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x23, 0x20]);
    // subtype = 35 (Ct)
    assert_eq!(&bytes[8..10], &[0x00, 0x23]);
    // flags = 0x01
    assert_eq!(&bytes[10..12], &[0x00, 0x01]);
    // zone_src = 0 (4 bytes)
    assert_eq!(&bytes[12..16], &[0x00, 0x00, 0x00, 0x00]);
    // zone = 100
    assert_eq!(&bytes[16..18], &[0x00, 0x64]);
    // recirc_table = 5
    assert_eq!(bytes[18], 5);
    // alg = 0
    assert_eq!(&bytes[22..24], &[0x00, 0x00]);
}

#[test]
fn encode_set_tunnel_id_action() {
    let bytes = nicira::encode_set_tunnel_id(0x1234);
    assert_eq!(bytes.len(), 24);
    // type = 0xffff (Experimenter)
    assert_eq!(&bytes[0..2], &[0xff, 0xff]);
    // vendor = NICIRA
    assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x23, 0x20]);
    // subtype = 33 (RegLoad2)
    assert_eq!(&bytes[8..10], &[0x00, 0x21]);
}

#[test]
fn encode_nx_reg_load_reg0() {
    let bytes = nicira::encode_nx_reg_load(0, 0x12345678, 0, 32);
    assert_eq!(bytes.len(), 24);
    // type = 0xffff
    assert_eq!(&bytes[0..2], &[0xff, 0xff]);
    // length = 24
    assert_eq!(&bytes[2..4], &[0x00, 0x18]);
    // vendor = NICIRA
    assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x23, 0x20]);
    // subtype = 7 (RegLoad)
    assert_eq!(&bytes[8..10], &[0x00, 0x07]);
    // ofs_nbits = (0 << 6) | 31 = 31
    assert_eq!(&bytes[10..12], &[0x00, 0x1f]);
}

#[test]
fn encode_nx_move_eth_src_to_reg() {
    // NXM headers: EthSrc = 0x80000406, Reg0 = 0x00010004
    let src = (0x8000 << 16) | (2 << 9) | 6; // EthSrc
    let dst = (1 << 16) | (0 << 9) | 4; // NXM_NX_REG0
    let bytes = nicira::encode_nx_move(src, dst, 32, 0, 0);
    assert_eq!(bytes.len(), 24);
    // subtype = 6 (Move)
    assert_eq!(&bytes[8..10], &[0x00, 0x06]);
    // n_bits = 32
    assert_eq!(&bytes[10..12], &[0x00, 0x20]);
}

#[test]
fn action_output_encode() {
    let action = Action::Output(OutputPort::Port(1));
    let bytes = action.encode();
    assert_eq!(bytes.len(), 16);
    assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x01]);
}

#[test]
fn action_controller_encode() {
    let action = Action::Controller { max_len: 65535 };
    let bytes = action.encode();
    assert_eq!(bytes.len(), 16);
    // port = CONTROLLER
    assert_eq!(&bytes[4..8], &[0xff, 0xff, 0xff, 0xfd]);
    // max_len = 65535
    assert_eq!(&bytes[8..10], &[0xff, 0xff]);
}

#[test]
fn action_drop_encode_empty() {
    let action = Action::Drop;
    let bytes = action.encode();
    assert!(bytes.is_empty()); // Drop produces no bytes
}

#[test]
fn action_pop_vlan_encode() {
    let action = Action::PopVlan;
    let bytes = action.encode();
    assert_eq!(bytes.len(), 8);
    assert_eq!(&bytes[0..2], &[0x00, 0x12]); // type=18
}

#[test]
fn action_push_vlan_encode() {
    let action = Action::PushVlan(0x8100);
    let bytes = action.encode();
    assert_eq!(bytes.len(), 8);
    assert_eq!(&bytes[0..2], &[0x00, 0x11]); // type=17
    assert_eq!(&bytes[4..6], &[0x81, 0x00]); // ethertype
}

#[test]
fn action_dec_ttl_encode() {
    let action = Action::DecTtl;
    let bytes = action.encode();
    assert_eq!(bytes.len(), 8);
    assert_eq!(&bytes[0..2], &[0x00, 0x18]); // type=24
}

#[test]
fn action_group_encode() {
    let action = Action::Group(42);
    let bytes = action.encode();
    assert_eq!(bytes.len(), 8);
    assert_eq!(&bytes[0..2], &[0x00, 0x16]); // type=22
    assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x2a]); // group_id=42
}

#[test]
fn action_set_eth_dst_encode() {
    let mac = MacAddr::from([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
    let action = Action::SetEthDst(mac);
    let bytes = action.encode();
    assert_eq!(bytes.len(), 16);
    assert_eq!(&bytes[0..2], &[0x00, 0x19]); // SetField
    assert_eq!(&bytes[8..14], &[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
}

#[test]
fn action_set_ipv4_dst_encode() {
    let addr: Ipv4Addr = "192.168.1.1".parse().unwrap();
    let action = Action::SetIpv4Dst(addr);
    let bytes = action.encode();
    assert_eq!(bytes.len(), 16);
    assert_eq!(&bytes[0..2], &[0x00, 0x19]); // SetField
    assert_eq!(&bytes[8..12], &[192, 168, 1, 1]);
}

#[test]
fn action_list_encode_multiple() {
    let list = ActionList::new()
        .pop_vlan()
        .output(OutputPort::Port(2));
    let bytes = list.encode();
    // PopVlan (8) + Output (16) = 24 bytes (already 8-byte aligned)
    assert_eq!(bytes.len(), 24);
    // First action: PopVlan
    assert_eq!(&bytes[0..2], &[0x00, 0x12]);
    // Second action: Output
    assert_eq!(&bytes[8..10], &[0x00, 0x00]);
    assert_eq!(&bytes[12..16], &[0x00, 0x00, 0x00, 0x02]);
}

#[test]
fn action_list_encode_empty() {
    let list = ActionList::new();
    let bytes = list.encode();
    assert!(bytes.is_empty());
}

#[test]
fn action_list_encode_padding() {
    // Just dec_ttl (8 bytes) should be 8-byte aligned already
    let list = ActionList::new().dec_ttl();
    let bytes = list.encode();
    assert_eq!(bytes.len(), 8);
    assert_eq!(bytes.len() % 8, 0);
}

#[test]
fn output_port_to_wire() {
    assert_eq!(OutputPort::Port(1).to_wire_port(), 1);
    assert_eq!(OutputPort::Controller.to_wire_port(), port::CONTROLLER);
    assert_eq!(OutputPort::Flood.to_wire_port(), port::FLOOD);
    assert_eq!(OutputPort::All.to_wire_port(), port::ALL);
    assert_eq!(OutputPort::InPort.to_wire_port(), port::IN_PORT);
    assert_eq!(OutputPort::Local.to_wire_port(), port::LOCAL);
    assert_eq!(OutputPort::Normal.to_wire_port(), port::NORMAL);
    assert_eq!(OutputPort::None.to_wire_port(), port::NONE);
}

// ========================================================================
// Decode tests
// ========================================================================

#[test]
fn decode_output_action() {
    let action = Action::Output(OutputPort::Port(5));
    let encoded = action.encode();
    let (decoded, len) = Action::decode(&encoded).unwrap();
    assert_eq!(len, 16);
    match decoded {
        Action::Output(port) => assert_eq!(port.to_wire_port(), 5),
        _ => panic!("expected Output action"),
    }
}

#[test]
fn decode_controller_action() {
    let action = Action::Controller { max_len: 128 };
    let encoded = action.encode();
    let (decoded, len) = Action::decode(&encoded).unwrap();
    assert_eq!(len, 16);
    match decoded {
        Action::Controller { max_len } => assert_eq!(max_len, 128),
        _ => panic!("expected Controller action"),
    }
}

#[test]
fn decode_pop_vlan_action() {
    let action = Action::PopVlan;
    let encoded = action.encode();
    let (decoded, len) = Action::decode(&encoded).unwrap();
    assert_eq!(len, 8);
    assert!(matches!(decoded, Action::PopVlan));
}

#[test]
fn decode_push_vlan_action() {
    let action = Action::PushVlan(0x8100);
    let encoded = action.encode();
    let (decoded, len) = Action::decode(&encoded).unwrap();
    assert_eq!(len, 8);
    match decoded {
        Action::PushVlan(ethertype) => assert_eq!(ethertype, 0x8100),
        _ => panic!("expected PushVlan action"),
    }
}

#[test]
fn decode_dec_ttl_action() {
    let action = Action::DecTtl;
    let encoded = action.encode();
    let (decoded, len) = Action::decode(&encoded).unwrap();
    assert_eq!(len, 8);
    assert!(matches!(decoded, Action::DecTtl));
}

#[test]
fn decode_set_ttl_action() {
    let action = Action::SetTtl(64);
    let encoded = action.encode();
    let (decoded, len) = Action::decode(&encoded).unwrap();
    assert_eq!(len, 8);
    match decoded {
        Action::SetTtl(ttl) => assert_eq!(ttl, 64),
        _ => panic!("expected SetTtl action"),
    }
}

#[test]
fn decode_group_action() {
    let action = Action::Group(42);
    let encoded = action.encode();
    let (decoded, len) = Action::decode(&encoded).unwrap();
    assert_eq!(len, 8);
    match decoded {
        Action::Group(group_id) => assert_eq!(group_id, 42),
        _ => panic!("expected Group action"),
    }
}

#[test]
fn decode_set_eth_dst_action() {
    let mac = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
    let action = Action::SetEthDst(mac);
    let encoded = action.encode();
    let (decoded, _) = Action::decode(&encoded).unwrap();
    match decoded {
        Action::SetEthDst(m) => assert_eq!(m, mac),
        _ => panic!("expected SetEthDst action"),
    }
}

#[test]
fn decode_set_ipv4_dst_action() {
    let addr: Ipv4Addr = "10.0.0.1".parse().unwrap();
    let action = Action::SetIpv4Dst(addr);
    let encoded = action.encode();
    let (decoded, _) = Action::decode(&encoded).unwrap();
    match decoded {
        Action::SetIpv4Dst(a) => assert_eq!(a, addr),
        _ => panic!("expected SetIpv4Dst action"),
    }
}

#[test]
fn decode_set_vlan_vid_action() {
    let action = Action::SetVlanVid(100);
    let encoded = action.encode();
    let (decoded, _) = Action::decode(&encoded).unwrap();
    match decoded {
        Action::SetVlanVid(vid) => assert_eq!(vid, 100),
        _ => panic!("expected SetVlanVid action"),
    }
}

#[test]
fn decode_nx_resubmit_action() {
    let action = Action::NxResubmit { port: Some(1), table: Some(10) };
    let encoded = action.encode();
    let (decoded, _) = Action::decode(&encoded).unwrap();
    match decoded {
        Action::NxResubmit { port, table } => {
            assert_eq!(port, Some(1));
            assert_eq!(table, Some(10));
        }
        _ => panic!("expected NxResubmit action"),
    }
}

#[test]
fn decode_nx_ct_action() {
    let action = Action::NxCt { flags: 0x01, zone: 100, table: Some(5) };
    let encoded = action.encode();
    let (decoded, _) = Action::decode(&encoded).unwrap();
    match decoded {
        Action::NxCt { flags, zone, table } => {
            assert_eq!(flags, 0x01);
            assert_eq!(zone, 100);
            assert_eq!(table, Some(5));
        }
        _ => panic!("expected NxCt action"),
    }
}

#[test]
fn decode_set_tunnel_id_action() {
    let action = Action::SetTunnelId(0x1234567890);
    let encoded = action.encode();
    let (decoded, _) = Action::decode(&encoded).unwrap();
    match decoded {
        Action::SetTunnelId(tun_id) => assert_eq!(tun_id, 0x1234567890),
        _ => panic!("expected SetTunnelId action"),
    }
}

#[test]
fn decode_action_list_multiple() {
    let list = ActionList::new()
        .pop_vlan()
        .output(OutputPort::Port(2))
        .dec_ttl();
    let encoded = list.encode();
    let decoded = ActionList::decode(&encoded).unwrap();
    assert_eq!(decoded.len(), 3);
    assert!(matches!(decoded.actions()[0], Action::PopVlan));
    assert!(matches!(decoded.actions()[1], Action::Output(_)));
    assert!(matches!(decoded.actions()[2], Action::DecTtl));
}

#[test]
fn decode_action_list_empty() {
    let list = ActionList::new();
    let encoded = list.encode();
    let decoded = ActionList::decode(&encoded).unwrap();
    assert!(decoded.is_empty());
}

#[test]
fn roundtrip_action_list() {
    let original = ActionList::new()
        .push_vlan(0x8100)
        .set_vlan_vid(100)
        .output(OutputPort::Port(3));
    let encoded = original.encode();
    let decoded = ActionList::decode(&encoded).unwrap();

    assert_eq!(decoded.len(), 3);
    match &decoded.actions()[0] {
        Action::PushVlan(ethertype) => assert_eq!(*ethertype, 0x8100),
        _ => panic!("expected PushVlan"),
    }
    match &decoded.actions()[1] {
        Action::SetVlanVid(vid) => assert_eq!(*vid, 100),
        _ => panic!("expected SetVlanVid"),
    }
    match &decoded.actions()[2] {
        Action::Output(port) => assert_eq!(port.to_wire_port(), 3),
        _ => panic!("expected Output"),
    }
}

#[test]
fn output_port_from_wire() {
    assert_eq!(OutputPort::from_wire(1).to_wire_port(), 1);
    assert_eq!(OutputPort::from_wire(port::CONTROLLER).to_wire_port(), port::CONTROLLER);
    assert!(matches!(OutputPort::from_wire(port::FLOOD), OutputPort::Flood));
    assert!(matches!(OutputPort::from_wire(port::ALL), OutputPort::All));
    assert!(matches!(OutputPort::from_wire(port::IN_PORT), OutputPort::InPort));
    assert!(matches!(OutputPort::from_wire(port::LOCAL), OutputPort::Local));
    assert!(matches!(OutputPort::from_wire(port::NORMAL), OutputPort::Normal));
    assert!(matches!(OutputPort::from_wire(port::NONE), OutputPort::None));
}

#[test]
fn action_type_try_from() {
    assert_eq!(ActionType::try_from(0).unwrap(), ActionType::Output);
    assert_eq!(ActionType::try_from(17).unwrap(), ActionType::PushVlan);
    assert_eq!(ActionType::try_from(18).unwrap(), ActionType::PopVlan);
    assert_eq!(ActionType::try_from(22).unwrap(), ActionType::Group);
    assert_eq!(ActionType::try_from(24).unwrap(), ActionType::DecNwTtl);
    assert_eq!(ActionType::try_from(25).unwrap(), ActionType::SetField);
    assert_eq!(ActionType::try_from(0xffff).unwrap(), ActionType::Experimenter);
    assert!(ActionType::try_from(99).is_err());
}

// Nicira extension tests

#[test]
fn resubmit_table_action_roundtrip() {
    let action = Action::NxResubmit {
        port: None,
        table: Some(5),
    };
    let encoded = action.encode();
    let (decoded, len) = Action::decode(&encoded).unwrap();
    assert_eq!(len, encoded.len());
    match decoded {
        Action::NxResubmit { port, table } => {
            assert_eq!(port, None);
            assert_eq!(table, Some(5));
        }
        _ => panic!("expected NxResubmit action"),
    }
}

#[test]
fn ct_action_roundtrip_with_table() {
    let action = Action::NxCt {
        flags: CT_COMMIT,
        zone: 100,
        table: Some(10),
    };
    let encoded = action.encode();
    let (decoded, len) = Action::decode(&encoded).unwrap();
    assert_eq!(len, encoded.len());
    match decoded {
        Action::NxCt { flags, zone, table } => {
            assert_eq!(flags, CT_COMMIT);
            assert_eq!(zone, 100);
            assert_eq!(table, Some(10));
        }
        _ => panic!("expected NxCt action"),
    }
}

#[test]
fn action_list_resubmit_table() {
    let list = ActionList::new().resubmit_table(10);
    assert_eq!(list.len(), 1);
    match &list.actions()[0] {
        Action::NxResubmit { port, table } => {
            assert_eq!(*port, None);
            assert_eq!(*table, Some(10));
        }
        _ => panic!("expected NxResubmit"),
    }
}

#[test]
fn action_list_ct_commit() {
    let list = ActionList::new().ct_commit(50);
    assert_eq!(list.len(), 1);
    match &list.actions()[0] {
        Action::NxCt { flags, zone, table } => {
            assert_eq!(*flags, CT_COMMIT);
            assert_eq!(*zone, 50);
            assert_eq!(*table, None);
        }
        _ => panic!("expected NxCt"),
    }
}

#[test]
fn action_list_ct_with_recirc() {
    let list = ActionList::new().ct(CT_COMMIT, 100, Some(5));
    assert_eq!(list.len(), 1);
    match &list.actions()[0] {
        Action::NxCt { flags, zone, table } => {
            assert_eq!(*flags, CT_COMMIT);
            assert_eq!(*zone, 100);
            assert_eq!(*table, Some(5));
        }
        _ => panic!("expected NxCt"),
    }
}

#[test]
fn nx_learn_builder() {
    let learn = NxLearn::new()
        .idle_timeout(300)
        .hard_timeout(600)
        .priority(100)
        .table(5)
        .cookie(0x1234);

    assert_eq!(learn.idle_timeout, 300);
    assert_eq!(learn.hard_timeout, 600);
    assert_eq!(learn.priority, 100);
    assert_eq!(learn.table_id, 5);
    assert_eq!(learn.cookie, 0x1234);
}

#[test]
fn nx_learn_with_specs() {
    let learn = NxLearn::new()
        .table(10)
        .match_field(0x00010006, 0x00010006, 48) // eth_src -> eth_src
        .load_immediate(0x00000404, vec![0, 0, 0, 1], 32); // output port 1

    assert_eq!(learn.table_id, 10);
    assert_eq!(learn.specs.len(), 2);
    match &learn.specs[0] {
        LearnSpec::MatchField {
            src_field,
            dst_field,
            n_bits,
        } => {
            assert_eq!(src_field, &0x00010006);
            assert_eq!(dst_field, &0x00010006);
            assert_eq!(n_bits, &48);
        }
        _ => panic!("expected MatchField"),
    }
    match &learn.specs[1] {
        LearnSpec::LoadImmediate {
            dst_field,
            value,
            n_bits,
        } => {
            assert_eq!(dst_field, &0x00000404);
            assert_eq!(value, &[0, 0, 0, 1]);
            assert_eq!(n_bits, &32);
        }
        _ => panic!("expected LoadImmediate"),
    }
}

#[test]
fn nx_learn_action_roundtrip() {
    let learn = NxLearn::new()
        .idle_timeout(300)
        .hard_timeout(600)
        .priority(50)
        .table(5)
        .cookie(0xabcd);

    let action = Action::NxLearn(learn);
    let encoded = action.encode();
    let (decoded, len) = Action::decode(&encoded).unwrap();
    assert_eq!(len, encoded.len());
    match decoded {
        Action::NxLearn(l) => {
            assert_eq!(l.idle_timeout, 300);
            assert_eq!(l.hard_timeout, 600);
            assert_eq!(l.priority, 50);
            assert_eq!(l.table_id, 5);
            assert_eq!(l.cookie, 0xabcd);
        }
        _ => panic!("expected NxLearn action"),
    }
}

#[test]
fn action_list_learn_builder() {
    let learn = NxLearn::new().table(10).priority(100);
    let list = ActionList::new().learn(learn);
    assert_eq!(list.len(), 1);
    match &list.actions()[0] {
        Action::NxLearn(l) => {
            assert_eq!(l.table_id, 10);
            assert_eq!(l.priority, 100);
        }
        _ => panic!("expected NxLearn"),
    }
}

#[test]
fn action_list_set_tunnel_id() {
    let list = ActionList::new().set_tunnel_id(0x123456789abc);
    assert_eq!(list.len(), 1);
    match &list.actions()[0] {
        Action::SetTunnelId(tun_id) => {
            assert_eq!(*tun_id, 0x123456789abc);
        }
        _ => panic!("expected SetTunnelId"),
    }
}

#[test]
fn action_list_group() {
    let list = ActionList::new().group(42);
    assert_eq!(list.len(), 1);
    match &list.actions()[0] {
        Action::Group(group_id) => {
            assert_eq!(*group_id, 42);
        }
        _ => panic!("expected Group"),
    }
}
