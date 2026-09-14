//! op-deployment: Container and image deployment
//!
//! Features:
//! - Container image management
//! - Deployment orchestration

pub mod image_manager;

pub use image_manager::ImageManager;

/// Prelude for convenient imports
pub mod prelude {
    pub use super::image_manager::ImageManager;
}
