# op-dbus-model - Specification

## Overview
**Crate**: `op-dbus-model`  
**Location**: `crates/op-dbus-model`  
**Version**: 0.1.0  
**Edition**: 2021

## Purpose

The `op-dbus-model` crate provides the core data models and database schema management for the operation-dbus system. It defines the persistence layer for plugin metadata and D-Bus interface schemas discovered through introspection.

This crate serves as the foundational data layer that enables:
- Plugin registration and lifecycle tracking
- Schema discovery and versioning
- Metadata persistence for D-Bus services
- Historical tracking of interface definitions

## Architecture

### Database Layer
- **Backend**: SQLite via sqlx
- **Runtime**: Tokio async runtime
- **Schema Management**: Automated table creation and migration

### Data Models

#### Plugin Model
Represents a registered D-Bus plugin/service in the system.

```rust
pub struct Plugin {
    pub name: String,              // Unique plugin identifier
    pub service_name: String,      // D-Bus service name
    pub base_object: OwnedValue,   // Base object path (JSON)
    pub created_at: DateTime<Utc>, // Registration timestamp
}
```

**Database Schema**:
```sql
CREATE TABLE plugins (
    name TEXT PRIMARY KEY,
    service_name TEXT NOT NULL,
    base_object TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
)
```

#### Schema Model
Represents a discovered D-Bus interface schema.

```rust
pub struct Schema {
    pub id: String,                      // Unique schema identifier
    pub plugin_name: String,             // Foreign key to plugin
    pub definition: OwnedValue,          // Interface definition (JSON)
    pub discovered_from: Option<String>, // Discovery source
    pub discovered_at: Option<DateTime<Utc>>, // Discovery timestamp
    pub created_at: DateTime<Utc>,       // Record creation time
}
```

**Database Schema**:
```sql
CREATE TABLE schemas (
    id TEXT PRIMARY KEY,
    plugin_name TEXT NOT NULL,
    definition TEXT NOT NULL,
    discovered_from TEXT,
    discovered_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (plugin_name) REFERENCES plugins(name)
)
```

## Key Components

### Schema Creation
```rust
pub async fn create_schema(pool: &SqlitePool) -> Result<()>
```

Initializes the database schema with all required tables. This function is idempotent and safe to call multiple times.

**Features**:
- Creates `plugins` table for service registration
- Creates `schemas` table for interface definitions
- Establishes foreign key relationships
- Automatic timestamp management

## Dependencies

### Core Dependencies
- **sqlx** (0.8): Async SQL toolkit with SQLite support
  - Features: `runtime-tokio`, `sqlite`, `json`
- **serde** (1.0): Serialization framework
  - Features: `derive`
- **simd-json**: High-performance JSON handling
- **chrono** (0.4): Date and time handling
  - Features: `serde`
- **uuid** (1.6): Unique identifier generation
  - Features: `v4`, `serde`

### Error Handling
- **thiserror** (1.0): Derive macros for error types
- **anyhow** (1.0): Flexible error handling

### Internal Dependencies
- **op-core**: Core types and utilities

## Usage

### Initialization

```rust
use op_dbus_model::create_schema;
use sqlx::SqlitePool;

// Connect to database
let pool = SqlitePool::connect("sqlite:operation-dbus.db").await?;

// Initialize schema
create_schema(&pool).await?;
```

### Working with Models

```rust
use op_dbus_model::models::{Plugin, Schema};
use chrono::Utc;

// Create a plugin record
let plugin = Plugin {
    name: "my-service".to_string(),
    service_name: "org.example.MyService".to_string(),
    base_object: simd_json::json!("/org/example"),
    created_at: Utc::now(),
};

// Create a schema record
let schema = Schema {
    id: uuid::Uuid::new_v4().to_string(),
    plugin_name: "my-service".to_string(),
    definition: simd_json::json!({
        "interface": "org.example.MyInterface",
        "methods": [...]
    }),
    discovered_from: Some("introspection".to_string()),
    discovered_at: Some(Utc::now()),
    created_at: Utc::now(),
};
```

## Integration Points

### Plugin Registration Flow
1. Plugin discovered via D-Bus introspection
2. Plugin metadata stored in `plugins` table
3. Interface schemas extracted and stored in `schemas` table
4. Foreign key maintains relationship between plugin and schemas

### Schema Discovery Flow
1. D-Bus interface introspected
2. Schema definition serialized to JSON
3. Schema record created with discovery metadata
4. Linked to parent plugin via `plugin_name`

## Data Integrity

### Referential Integrity
- Foreign key constraint ensures schemas reference valid plugins
- Cascade behavior can be configured for plugin deletion

### Timestamp Tracking
- `created_at`: Automatic timestamp on record creation
- `discovered_at`: Manual timestamp for schema discovery events

### JSON Storage
- `base_object`: Flexible storage for object path configurations
- `definition`: Complete interface definition with methods, signals, properties

## Performance Considerations

- **SQLite**: Suitable for single-node deployments
- **Connection Pooling**: Managed by sqlx for concurrent access
- **JSON Indexing**: Consider adding indexes on JSON fields for large datasets
- **Async Operations**: Non-blocking database access via tokio

## Future Enhancements

- Schema versioning and migration tracking
- Plugin dependency management
- Schema validation and compatibility checking
- Query builders for common access patterns
- Migration to PostgreSQL for distributed deployments

---
*Core data models and persistence layer for operation-dbus*
