//! Mirrors subsidiary's `link.test.ts`, on the shared record vector.

use super::*;
use crate::public_record::build_record;
use serde_json::json;
use std::collections::HashMap;

/// The link link.ts builds for the same inputs, byte for byte.
const TS_LINK: &str = r#"{"bubble":{"closesAt":1795860300,"opensAt":1795859995},"entryHash":"0xf7c0fb89e1ae43539f5a16ceb090a040d647c79b310896878acbdf4c4c867890","hashchain":{"eventHash":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","eventId":1024,"time":1795860042},"public":{"blockHash":"0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","blockNumber":7,"blockTime":1795860000,"chainId":31337,"contract":"0x5fbdb2315678afecb367f032d93f642f64180aa3","recordId":1,"root":"0x99e2f8cc8681e2b207d3b8862993558ddbbb031bbd4f8cde53ddc5875911591d","txHash":"0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"vector":{"ledgerPointId":"99dfeafe-d6fa-5d7d-b60d-570f1f7eb43f"},"version":"subsidiary.link/1"}"#;

// 2026-11-28T10:00:00Z
const T: i64 = 1_795_860_000;

fn salt(b: &str) -> String {
    format!("0x{}", b.repeat(32))
}

fn entry() -> RecordEntry {
    build_record(
        &json!({ "status": "delivered", "client": "Ada Lovelace", "design": { "patent": "pending" } }),
        &serde_json::from_value(json!({ "status": "none", "client": "private" })).unwrap(),
        &HashMap::from([
            ("client".to_owned(), salt("11")),
            ("design".to_owned(), salt("22")),
            ("status".to_owned(), salt("33")),
        ]),
    )
    .unwrap()
    .entry
}

fn public() -> PublicRef {
    PublicRef {
        chain_id: 31337,
        contract: "0x5fbdb2315678afecb367f032d93f642f64180aa3".to_owned(),
        record_id: 1,
        root: entry().root,
        tx_hash: salt("aa"),
        block_number: 7,
        block_hash: salt("bb"),
        block_time: T,
    }
}

fn event() -> LinkedEventInput {
    LinkedEventInput {
        event_id: 1024,
        event_hash: "c".repeat(64),
        timestamp: "2026-11-28T10:00:42.123Z".to_owned(),
    }
}

fn at(timestamp: &str) -> LinkedEventInput {
    LinkedEventInput { timestamp: timestamp.to_owned(), ..event() }
}

#[test]
fn ledger_point_id_matches_python_uuid5() {
    // python3 -c "import uuid;print(uuid.uuid5(uuid.UUID('3c0b91e4-5a27-4d6f-8b12-e6f409d3a751'),'ledger:abc'))"
    assert_eq!(ledger_point_id("abc").to_string(), "0415f5fc-1672-5a0d-b073-3c7e8e46a6a8");
}

#[test]
fn ties_every_id_inside_the_bubble() {
    let link = build_link(&public(), &event(), &entry(), None, None).unwrap();
    assert_eq!(link.bubble, Bubble { opens_at: T - 5, closes_at: T + 300 });
    assert_eq!(
        link.hashchain,
        LinkedEvent { event_id: 1024, event_hash: "c".repeat(64), time: T + 42 }
    );
    assert_eq!(link.vector.ledger_point_id, ledger_point_id(&"c".repeat(64)).to_string());
    assert_eq!(link.entry_hash, crate::public_record::entry_hash(&entry()).unwrap());
    assert!(check_link(&link, Some(&public()), Some(&event()), Some(&entry())).is_empty());
}

#[test]
fn writes_and_reads_the_link_as_link_ts_does() {
    let link = build_link(&public(), &event(), &entry(), None, None).unwrap();
    let text = link_text(&link).unwrap();
    assert_eq!(text, TS_LINK);
    assert!(text.starts_with(r#"{"bubble":{"closesAt":1795860300,"opensAt":1795859995},"entryHash":"0x"#));
    assert!(text.contains(r#""hashchain":{"eventHash":"cccc"#));
    let back: Link = serde_json::from_str(&text).unwrap();
    assert_eq!(back, link);
    // snowball.query_events returns more than these three fields.
    let ev: LinkedEventInput = serde_json::from_value(json!({
        "event_id": 1024, "event_hash": "c".repeat(64), "timestamp": "2026-11-28T10:00:42Z",
        "prev_hash": "0".repeat(64), "kind": "x"
    }))
    .unwrap();
    assert_eq!(event_seconds(&ev).unwrap(), T + 42);
}

#[test]
fn refuses_an_event_outside_the_bubble() {
    for late_or_early in ["2026-11-28T10:05:01Z", "2026-11-28T09:59:54Z"] {
        let err = build_link(&public(), &at(late_or_early), &entry(), None, None).unwrap_err();
        assert!(err.to_string().contains("outside the bubble"), "{err}");
    }
    let wide = build_link(&public(), &at("2026-11-28T10:05:01Z"), &entry(), Some(600), None).unwrap();
    assert_eq!(wide.bubble.closes_at, T + 600);
    assert!(matches!(build_link(&public(), &event(), &entry(), Some(0), None), Err(LinkError::BadSeconds)));
    assert!(matches!(build_link(&public(), &event(), &entry(), None, Some(-1)), Err(LinkError::BadSkew)));
    assert!(matches!(
        build_link(&public(), &at("yesterday"), &entry(), None, None),
        Err(LinkError::BadTimestamp(1024))
    ));
}

#[test]
fn refuses_an_entry_whose_root_is_not_the_public_root() {
    let p = PublicRef { root: salt("dd"), ..public() };
    let err = build_link(&p, &event(), &entry(), None, None).unwrap_err();
    assert!(err.to_string().contains("entry root differs"), "{err}");
}

#[test]
fn catches_each_side_changing_after_the_link_was_made() {
    let link = build_link(&public(), &event(), &entry(), None, None).unwrap();
    let p = PublicRef { block_hash: salt("ee"), ..public() };
    assert_eq!(check_link(&link, Some(&p), None, None), ["public chain says something else"]);
    let e = LinkedEventInput { event_hash: "d".repeat(64), ..event() };
    assert_eq!(check_link(&link, None, Some(&e), None), ["hash-chain event id or hash differs"]);
    assert_eq!(
        check_link(&link, None, Some(&at("2026-11-28T10:00:43Z")), None),
        ["hash-chain event time differs"]
    );
    let mut changed = entry();
    changed.fields[1].value = json!({ "patent": "granted" });
    assert_eq!(check_link(&link, None, None, Some(&changed)), ["entry differs from the one linked"]);
    let mut wrong = link.clone();
    wrong.vector.ledger_point_id = "00000000-0000-5000-8000-000000000000".to_owned();
    assert_eq!(check_link(&wrong, None, None, None), ["vector id is not the ledger point id of the event hash"]);
    let mut moved = link.clone();
    moved.hashchain.time = T + 301;
    assert_eq!(check_link(&moved, None, None, None), ["hash-chain event time is outside the bubble"]);
}
