# op-deployment - Specification

## Overview
**Crate**: `op-deployment`  
**Location**: `crates/op-deployment`  
**Description**: Container and image deployment management

## Purpose

The `op-deployment` crate provides a sophisticated BTRFS-based deployment image management system with intelligent deduplication. It enables efficient creation, storage, and streaming of deployment images by leveraging filesystem snapshots and symlink-based deduplication.

This crate is critical for:
- **Deployment Orchestration**: Managing deployment artifacts and versions
- **Storage Efficiency**: Deduplicating common files across deployment images
- **Snapshot Management**: Creating BTRFS snapshots for atomic deployments
- **Streaming Deployments**: Preparing images for efficient network transfer

## Architecture

### BTRFS Integration
The system leverages BTRFS filesystem features:
- **Snapshots**: Atomic, copy-on-write snapshots for each deployment image
- **Deduplication**: Symlink-based deduplication across image versions
- **Streaming**: Efficient snapshot streaming for remote deployments

### Directory Structure
```
base_path/
├── images/          # Deployment image directories
│   ├── image-v1/    # First deployment image
│   ├── image-v2/    # Second deployment image (deduplicated)
│   └── ...
└── snapshots/       # BTRFS snapshots for streaming
    ├── image-v1-snap/
    ├── image-v2-snap/
    └── ...
```

## Key Components

### ImageManager
Core component for managing deployment images.

```rust
pub struct ImageManager {
    base_path: PathBuf,
    images_dir: PathBuf,
    snapshots_dir: PathBuf,
}
```

**Key Methods**:
- `new(base_path)`: Create new image manager instance
- `init()`: Initialize directory structure and verify BTRFS
- `create_image(name, files)`: Create new deployment image with deduplication
- `is_btrfs(path)`: Check if path is on BTRFS filesystem

### ImageMetadata
Metadata for a deployment image.

```rust
pub struct ImageMetadata {
    pub name: String,              // Image identifier
    pub path: PathBuf,             // Image directory path
    pub created: i64,              // Creation timestamp
    pub files: Vec<FileEntry>,     // File inventory
    pub total_size: u64,           // Total size of all files
    pub unique_size: u64,          // Size of unique files
    pub symlinked_size: u64,       // Size of deduplicated files
}
```

**Metrics**:
- `total_size`: Sum of all file sizes in the image
- `unique_size`: Storage actually consumed by new files
- `symlinked_size`: Storage saved through deduplication

### FileEntry
Represents a file within a deployment image.

```rust
pub struct FileEntry {
    pub path: PathBuf,                  // Relative path in image
    pub is_symlink: bool,               // Whether file is symlinked
    pub symlink_target: Option<PathBuf>, // Target if symlinked
    pub size: u64,                      // File size in bytes
    pub hash: Option<String>,           // SHA256 hash for deduplication
}
```

## Deduplication Strategy

### Hash-Based Deduplication
1. **Hash Calculation**: SHA256 hash computed for each file
2. **Previous Image Scan**: Check if hash exists in prior images
3. **Symlink Creation**: If match found, create symlink instead of copying
4. **New File Copy**: If no match, copy file normally

### Benefits
- **Storage Efficiency**: Dramatically reduces disk usage for similar images
- **Fast Creation**: Symlinking is faster than copying
- **Version Tracking**: Maintains clear lineage between image versions

### Example
```
Image v1:
  /bin/app (100MB) - copied

Image v2:
  /bin/app (100MB) - symlinked to v1 (saves 100MB)
  /bin/new-tool (50MB) - copied
  
Total storage: 150MB instead of 200MB
```

## Dependencies

### Core Dependencies
- **tokio**: Async runtime for non-blocking I/O
- **serde**: Serialization for metadata
- **simd-json**: High-performance JSON handling
- **anyhow**: Error handling with context
- **thiserror**: Custom error types

### Filesystem Operations
- **tar**: Archive creation for image packaging
- **flate2**: Compression for efficient transfer
- **sha2**: SHA256 hashing for deduplication

### Utilities
- **reqwest**: HTTP client for remote image operations
- **chrono**: Timestamp management
- **uuid**: Unique identifiers for images
- **tracing/log**: Structured logging

### Development
- **tempfile**: Temporary directories for testing

## Usage

### Initialization

```rust
use op_deployment::ImageManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create image manager
    let manager = ImageManager::new("/var/lib/op-deployment");
    
    // Initialize directory structure
    manager.init().await?;
    
    Ok(())
}
```

### Creating a Deployment Image

```rust
use std::path::PathBuf;

// Prepare files for deployment
let files = vec![
    PathBuf::from("/path/to/binary"),
    PathBuf::from("/path/to/config.toml"),
    PathBuf::from("/path/to/lib.so"),
];

// Create deployment image
let metadata = manager.create_image("my-service-v1", files).await?;

println!("Image created: {}", metadata.name);
println!("Total size: {} bytes", metadata.total_size);
println!("Unique size: {} bytes", metadata.unique_size);
println!("Saved via deduplication: {} bytes", metadata.symlinked_size);
```

### Creating Incremental Updates

```rust
// Create v2 with mostly same files
let files_v2 = vec![
    PathBuf::from("/path/to/binary"),        // Same - will be symlinked
    PathBuf::from("/path/to/config.toml"),   // Same - will be symlinked
    PathBuf::from("/path/to/new-feature.so"), // New - will be copied
];

let metadata_v2 = manager.create_image("my-service-v2", files_v2).await?;

// Most files symlinked, only new files consume storage
assert!(metadata_v2.symlinked_size > 0);
```

## BTRFS Snapshot Workflow

### Snapshot Creation
1. Image directory created with files
2. BTRFS snapshot taken of image directory
3. Snapshot stored in `snapshots/` for streaming

### Snapshot Streaming
1. BTRFS send command generates snapshot stream
2. Stream can be piped over network
3. Remote system receives with BTRFS receive
4. Atomic deployment on target system

### Fallback Behavior
If not on BTRFS:
- Images still created with deduplication
- Snapshots disabled (warning logged)
- Standard tar/gzip used for transfer

## Performance Considerations

### Filesystem Requirements
- **BTRFS Recommended**: Full feature set with snapshots
- **Other Filesystems**: Deduplication works, snapshots disabled

### Deduplication Overhead
- **Hash Calculation**: SHA256 computed once per file
- **Lookup Cost**: O(n) scan of previous images
- **Optimization**: Consider hash index for large deployments

### Storage Savings
- **Similar Images**: 70-90% deduplication typical
- **Incremental Updates**: 95%+ deduplication common
- **Complete Rewrites**: Minimal deduplication

## Integration Points

### Deployment Pipeline
```
Build → Package → Create Image → Snapshot → Stream → Deploy
         ↓
    op-deployment
```

### Service Integration
- **op-plugins**: Deploy plugin binaries
- **op-services**: Deploy service configurations
- **op-tools**: Deployment automation scripts

## Error Handling

### Filesystem Errors
- Directory creation failures
- Permission issues
- BTRFS command failures

### Deduplication Errors
- Hash calculation failures
- Symlink creation errors
- File copy failures

### Recovery
- Partial images cleaned up on failure
- Atomic operations where possible
- Detailed error context via anyhow

## Testing

### Test Coverage
- Unit tests for deduplication logic
- Integration tests with tempfile
- BTRFS detection tests
- Snapshot creation tests

### Test Utilities
```rust
#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_image_creation() {
        let temp = TempDir::new().unwrap();
        let manager = ImageManager::new(temp.path());
        manager.init().await.unwrap();
        // ...
    }
}
```

## Future Enhancements

- **Parallel Hashing**: Multi-threaded hash calculation
- **Hash Index**: Database for O(1) deduplication lookups
- **Compression**: Per-file compression for unique files
- **Remote Streaming**: Direct network streaming support
- **Garbage Collection**: Cleanup of unreferenced images
- **Incremental Snapshots**: BTRFS incremental send/receive
- **Multi-Backend**: Support for ZFS, LVM snapshots

## Security Considerations

- **Hash Verification**: SHA256 ensures file integrity
- **Symlink Safety**: Validate symlink targets within image
- **Permission Preservation**: Maintain file permissions in images
- **Atomic Operations**: Prevent partial deployments

---
*BTRFS-based deployment image management with intelligent deduplication*
