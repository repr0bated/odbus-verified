//! Nested catalogs: compliance, UI target, models.
//!
//! Each module holds a routing view only. The sources stay authoritative:
//! `oscal/` JSON for controls, the sealed `json_render` blob for components,
//! the live `chat_router` catalog for providers/models.

pub mod compliance;
pub mod models;
pub mod projection;
pub mod specgen;
pub mod ui;
