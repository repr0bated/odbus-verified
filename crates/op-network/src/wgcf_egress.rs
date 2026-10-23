//! Restore the decoy's existing marked WARP route without changing WireGuard
//! configuration, packet classification, or the management routing table.

use anyhow::{Context, Result, ensure};
use futures::TryStreamExt;
use netlink_packet_route::{
    AddressFamily,
    route::{RouteAttribute, RouteType},
    rule::{RuleAction, RuleAttribute, RuleMessage},
};
use rtnetlink::{Handle, IpVersion};
use serde::Serialize;

const MARK: u32 = 0x51820;
const TABLE: u32 = 51820;
const PRIORITY: u32 = 41;
const FAILURE_METRIC: u32 = 32766;

#[derive(Debug, Serialize)]
pub struct PolicyReceipt {
    pub interface_index: u32,
    pub packet_mark: u32,
    pub table: u32,
    pub priority: u32,
    pub failure_metric: u32,
}

fn owned_selector(rule: &RuleMessage) -> bool {
    let table = rule.attributes.iter().find_map(|attribute| match attribute {
        RuleAttribute::Table(value) => Some(*value),
        _ => None,
    }).unwrap_or(u32::from(rule.header.table));
    rule.header.family == AddressFamily::Inet
        && rule.header.src_len == 0
        && rule.header.dst_len == 0
        && rule.header.tos == 0
        && rule.header.flags.is_empty()
        && rule.header.action == RuleAction::ToTable
        && table == TABLE
        && rule.attributes.contains(&RuleAttribute::FwMark(MARK))
        && rule.attributes.iter().all(|attribute| match attribute {
            RuleAttribute::Priority(value) => *value == PRIORITY,
            RuleAttribute::Table(value) => *value == TABLE,
            RuleAttribute::FwMark(value) => *value == MARK,
            RuleAttribute::FwMask(value) => *value == u32::MAX,
            RuleAttribute::SuppressPrefixLen(value)
            | RuleAttribute::SuppressIfGroup(value) => *value == u32::MAX,
            RuleAttribute::Protocol(_) | RuleAttribute::L3MDev(false) => true,
            _ => false,
        })
}

async fn inspect(handle: &Handle, interface_index: u32) -> Result<(bool, bool)> {
    let mut selectors = 0;
    let mut rules = handle.rule().get(IpVersion::V4).execute();
    while let Some(rule) = rules.try_next().await? {
        if rule.attributes.contains(&RuleAttribute::Priority(PRIORITY)) {
            ensure!(owned_selector(&rule), "priority 41 is not the owned WARP selector");
            selectors += 1;
            ensure!(selectors == 1, "priority 41 has duplicate selectors");
        }
    }

    let mut warp_default = false;
    let mut terminal_default = false;
    let mut routes = handle.route().get(IpVersion::V4).execute();
    while let Some(route) = routes.try_next().await? {
        let table = route.attributes.iter().find_map(|attribute| match attribute {
            RouteAttribute::Table(value) => Some(*value),
            _ => None,
        }).unwrap_or(u32::from(route.header.table));
        if table != TABLE {
            continue;
        }
        let metric = route.attributes.iter().find_map(|attribute| match attribute {
            RouteAttribute::Priority(value) => Some(*value),
            _ => None,
        }).unwrap_or(0);
        let default = route.header.destination_prefix_length == 0
            && route.header.source_prefix_length == 0
            && route.header.tos == 0;
        let terminal = default
            && route.header.kind == RouteType::Unreachable
            && metric == FAILURE_METRIC
            && route.attributes.iter().all(|attribute| matches!(attribute,
                RouteAttribute::Table(_) | RouteAttribute::Priority(_)
                    | RouteAttribute::CacheInfo(_)
            ));
        let warp = route.header.kind == RouteType::Unicast
            && route.attributes.contains(&RouteAttribute::Oif(interface_index))
            && !route.attributes.iter().any(|attribute| matches!(attribute,
                RouteAttribute::MultiPath(_)
            ));
        ensure!(warp || terminal, "table 51820 contains a foreign route");
        warp_default |= warp && default && metric < FAILURE_METRIC;
        terminal_default |= terminal;
    }
    ensure!(warp_default, "table 51820 has no retained WARP default before metric 32766");
    Ok((selectors == 1, terminal_default))
}

/// Restore only the retained IPv4 selector and its fail-closed terminal route.
/// The caller must already own the interface, table and packet classifier.
/// Refuse foreign state; never flush, replace routes, or reapply peer profiles.
pub async fn restore_selected_policy(interface: &str) -> Result<PolicyReceipt> {
    let (connection, handle, _) = rtnetlink::new_connection()?;
    tokio::spawn(connection);
    let mut links = handle.link().get().match_name(interface.to_owned()).execute();
    let interface_index = links.try_next().await?
        .with_context(|| format!("retained WARP interface {interface} does not exist"))?
        .header.index;
    let (selector, terminal) = inspect(&handle, interface_index).await?;

    // The terminal route must exist before marked traffic can select the table.
    if !terminal {
        handle.route().add().v4().table_id(TABLE)
            .kind(RouteType::Unreachable).priority(FAILURE_METRIC)
            .execute().await.context("installing WARP fail-closed route")?;
    }
    if !selector {
        let mut request = handle.rule().add().v4().table_id(TABLE)
            .priority(PRIORITY).fw_mark(MARK).action(RuleAction::ToTable);
        request.message_mut().attributes.push(RuleAttribute::FwMask(u32::MAX));
        request.execute().await.context("installing marked WARP selector")?;
    }
    let (selector, terminal) = inspect(&handle, interface_index).await?;
    ensure!(selector && terminal, "WARP policy readback is incomplete");
    Ok(PolicyReceipt {
        interface_index,
        packet_mark: MARK,
        table: TABLE,
        priority: PRIORITY,
        failure_metric: FAILURE_METRIC,
    })
}
