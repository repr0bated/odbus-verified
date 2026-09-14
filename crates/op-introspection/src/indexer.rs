//! DBus Hierarchical Indexer with FTS5 Full-Text Search
//!
//! Builds a persistent, searchable index of all DBus services, objects, methods, and properties.
//! Enables semantic queries like "find all network-related methods" without real-time DBus calls.

use anyhow::{Context, Result};
use chrono::Utc;
use op_core::types::{BusType, ObjectInfo};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

use crate::scanner::ServiceScanner;

/// Statistics about the indexed DBus system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStatistics {
    pub total_services: usize,
    pub total_objects: usize,
    pub total_interfaces: usize,
    pub total_methods: usize,
    pub total_properties: usize,
    pub total_signals: usize,
    pub scan_duration_seconds: f64,
    pub indexed_at: i64,
}

/// Search result for FTS queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub service: String,
    pub object_path: String,
    pub interface: String,
    pub item_type: String, // "method", "property", "signal", "interface"
    pub item_name: String,
    pub description: String,
    pub relevance_score: f64,
}

/// DBus FTS Indexer - builds searchable index of entire DBus system
pub struct DbusIndexer {
    #[allow(clippy::arc_with_non_send_sync)]
    conn: Arc<RwLock<Connection>>,
    scanner: Arc<ServiceScanner>,
}

impl DbusIndexer {
    /// Create new indexer with SQLite database
    #[allow(clippy::arc_with_non_send_sync)]
    pub async fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path.as_ref()).context("Failed to open indexer database")?;

        // Create schema with FTS5 virtual tables
        conn.execute_batch(
            r#"
            -- Core index tables
            CREATE TABLE IF NOT EXISTS services (
                service_name TEXT PRIMARY KEY,
                indexed_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS objects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                service_name TEXT NOT NULL,
                object_path TEXT NOT NULL,
                indexed_at INTEGER NOT NULL,
                UNIQUE(service_name, object_path)
            );

            CREATE TABLE IF NOT EXISTS interfaces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                object_id INTEGER NOT NULL,
                interface_name TEXT NOT NULL,
                indexed_at INTEGER NOT NULL,
                FOREIGN KEY(object_id) REFERENCES objects(id),
                UNIQUE(object_id, interface_name)
            );

            CREATE TABLE IF NOT EXISTS methods (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                interface_id INTEGER NOT NULL,
                method_name TEXT NOT NULL,
                input_signature TEXT,
                output_signature TEXT,
                description TEXT,
                FOREIGN KEY(interface_id) REFERENCES interfaces(id),
                UNIQUE(interface_id, method_name)
            );

            CREATE TABLE IF NOT EXISTS properties (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                interface_id INTEGER NOT NULL,
                property_name TEXT NOT NULL,
                type_signature TEXT NOT NULL,
                access TEXT NOT NULL,
                description TEXT,
                FOREIGN KEY(interface_id) REFERENCES interfaces(id),
                UNIQUE(interface_id, property_name)
            );

            CREATE TABLE IF NOT EXISTS signals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                interface_id INTEGER NOT NULL,
                signal_name TEXT NOT NULL,
                signature TEXT,
                description TEXT,
                FOREIGN KEY(interface_id) REFERENCES interfaces(id),
                UNIQUE(interface_id, signal_name)
            );

            -- FTS5 virtual tables for full-text search
            CREATE VIRTUAL TABLE IF NOT EXISTS methods_fts USING fts5(
                service_name,
                object_path,
                interface_name,
                method_name,
                description,
                input_signature,
                output_signature,
                content=methods,
                content_rowid=id
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS properties_fts USING fts5(
                service_name,
                object_path,
                interface_name,
                property_name,
                description,
                type_signature,
                access,
                content=properties,
                content_rowid=id
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS signals_fts USING fts5(
                service_name,
                object_path,
                interface_name,
                signal_name,
                description,
                signature,
                content=signals,
                content_rowid=id
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS interfaces_fts USING fts5(
                service_name,
                object_path,
                interface_name,
                content=interfaces,
                content_rowid=id
            );

            -- Triggers to keep FTS tables in sync
            CREATE TRIGGER IF NOT EXISTS methods_ai AFTER INSERT ON methods BEGIN
                INSERT INTO methods_fts(rowid, service_name, object_path, interface_name,
                                       method_name, description, input_signature, output_signature)
                SELECT m.id,
                       o.service_name,
                       o.object_path,
                       i.interface_name,
                       m.method_name,
                       m.description,
                       m.input_signature,
                       m.output_signature
                FROM methods m
                JOIN interfaces i ON m.interface_id = i.id
                JOIN objects o ON i.object_id = o.id
                WHERE m.id = NEW.id;
            END;

            CREATE TRIGGER IF NOT EXISTS methods_au AFTER UPDATE ON methods BEGIN
                INSERT INTO methods_fts(methods_fts, rowid) VALUES('delete', OLD.id);
                INSERT INTO methods_fts(rowid, service_name, object_path, interface_name,
                                       method_name, description, input_signature, output_signature)
                SELECT m.id,
                       o.service_name,
                       o.object_path,
                       i.interface_name,
                       m.method_name,
                       m.description,
                       m.input_signature,
                       m.output_signature
                FROM methods m
                JOIN interfaces i ON m.interface_id = i.id
                JOIN objects o ON i.object_id = o.id
                WHERE m.id = NEW.id;
            END;

            CREATE TRIGGER IF NOT EXISTS properties_ai AFTER INSERT ON properties BEGIN
                INSERT INTO properties_fts(rowid, service_name, object_path, interface_name,
                                          property_name, description, type_signature, access)
                SELECT p.id,
                       o.service_name,
                       o.object_path,
                       i.interface_name,
                       p.property_name,
                       p.description,
                       p.type_signature,
                       p.access
                FROM properties p
                JOIN interfaces i ON p.interface_id = i.id
                JOIN objects o ON i.object_id = o.id
                WHERE p.id = NEW.id;
            END;

            CREATE TRIGGER IF NOT EXISTS properties_au AFTER UPDATE ON properties BEGIN
                INSERT INTO properties_fts(properties_fts, rowid) VALUES('delete', OLD.id);
                INSERT INTO properties_fts(rowid, service_name, object_path, interface_name,
                                          property_name, description, type_signature, access)
                SELECT p.id,
                       o.service_name,
                       o.object_path,
                       i.interface_name,
                       p.property_name,
                       p.description,
                       p.type_signature,
                       p.access
                FROM properties p
                JOIN interfaces i ON p.interface_id = i.id
                JOIN objects o ON i.object_id = o.id
                WHERE p.id = NEW.id;
            END;

            CREATE TRIGGER IF NOT EXISTS signals_ai AFTER INSERT ON signals BEGIN
                INSERT INTO signals_fts(rowid, service_name, object_path, interface_name,
                                       signal_name, description, signature)
                SELECT s.id,
                       o.service_name,
                       o.object_path,
                       i.interface_name,
                       s.signal_name,
                       s.description,
                       s.signature
                FROM signals s
                JOIN interfaces i ON s.interface_id = i.id
                JOIN objects o ON i.object_id = o.id
                WHERE s.id = NEW.id;
            END;

            CREATE TRIGGER IF NOT EXISTS signals_au AFTER UPDATE ON signals BEGIN
                INSERT INTO signals_fts(signals_fts, rowid) VALUES('delete', OLD.id);
                INSERT INTO signals_fts(rowid, service_name, object_path, interface_name,
                                       signal_name, description, signature)
                SELECT s.id,
                       o.service_name,
                       o.object_path,
                       i.interface_name,
                       s.signal_name,
                       s.description,
                       s.signature
                FROM signals s
                JOIN interfaces i ON s.interface_id = i.id
                JOIN objects o ON i.object_id = o.id
                WHERE s.id = NEW.id;
            END;

            CREATE TRIGGER IF NOT EXISTS interfaces_ai AFTER INSERT ON interfaces BEGIN
                INSERT INTO interfaces_fts(rowid, service_name, object_path, interface_name)
                SELECT i.id,
                       o.service_name,
                       o.object_path,
                       i.interface_name
                FROM interfaces i
                JOIN objects o ON i.object_id = o.id
                WHERE i.id = NEW.id;
            END;

            CREATE TRIGGER IF NOT EXISTS interfaces_au AFTER UPDATE ON interfaces BEGIN
                INSERT INTO interfaces_fts(interfaces_fts, rowid) VALUES('delete', OLD.id);
                INSERT INTO interfaces_fts(rowid, service_name, object_path, interface_name)
                SELECT i.id,
                       o.service_name,
                       o.object_path,
                       i.interface_name
                FROM interfaces i
                JOIN objects o ON i.object_id = o.id
                WHERE i.id = NEW.id;
            END;

            -- Index for performance
            CREATE INDEX IF NOT EXISTS idx_objects_service ON objects(service_name);
            CREATE INDEX IF NOT EXISTS idx_interfaces_object ON interfaces(object_id);
            CREATE INDEX IF NOT EXISTS idx_methods_interface ON methods(interface_id);
            CREATE INDEX IF NOT EXISTS idx_properties_interface ON properties(interface_id);
            CREATE INDEX IF NOT EXISTS idx_signals_interface ON signals(interface_id);

            -- Statistics table
            CREATE TABLE IF NOT EXISTS index_stats (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                total_services INTEGER NOT NULL,
                total_objects INTEGER NOT NULL,
                total_interfaces INTEGER NOT NULL,
                total_methods INTEGER NOT NULL,
                total_properties INTEGER NOT NULL,
                total_signals INTEGER NOT NULL,
                scan_duration_seconds REAL NOT NULL,
                indexed_at INTEGER NOT NULL
            );
            "#,
        )?;

        let scanner = Arc::new(ServiceScanner::new());

        Ok(Self {
            conn: Arc::new(RwLock::new(conn)),
            scanner,
        })
    }

    /// Build complete index of DBus system
    pub async fn build_index(&self, bus_type: BusType) -> Result<IndexStatistics> {
        info!("🔍 Starting DBus index build for {:?} bus", bus_type);
        let start = std::time::Instant::now();

        // Discover all services
        let services = self.scanner.list_services(bus_type).await?;
        info!("   Found {} services to index", services.len());

        let mut total_objects = 0;
        let mut total_interfaces = 0;
        let mut total_methods = 0;
        let mut total_properties = 0;
        let mut total_signals = 0;

        let timestamp = Utc::now().timestamp();

        for (idx, service_info) in services.iter().enumerate() {
            if (idx + 1) % 10 == 0 {
                info!("   Progress: {}/{} services", idx + 1, services.len());
            }

            match self
                .index_service(bus_type, &service_info.name, timestamp)
                .await
            {
                Ok(stats) => {
                    total_objects += stats.0;
                    total_interfaces += stats.1;
                    total_methods += stats.2;
                    total_properties += stats.3;
                    total_signals += stats.4;
                }
                Err(e) => {
                    warn!("Failed to index service {}: {}", service_info.name, e);
                }
            }
        }

        let duration = start.elapsed().as_secs_f64();

        let stats = IndexStatistics {
            total_services: services.len(),
            total_objects,
            total_interfaces,
            total_methods,
            total_properties,
            total_signals,
            scan_duration_seconds: duration,
            indexed_at: timestamp,
        };

        // Store statistics
        self.store_statistics(&stats)?;

        info!("✅ Index build complete in {:.2}s", duration);
        info!("   Services: {}", stats.total_services);
        info!("   Objects: {}", stats.total_objects);
        info!("   Methods: {}", stats.total_methods);
        info!("   Properties: {}", stats.total_properties);

        Ok(stats)
    }

    /// Index a single service
    async fn index_service(
        &self,
        bus_type: BusType,
        service_name: &str,
        timestamp: i64,
    ) -> Result<(usize, usize, usize, usize, usize)> {
        let mut total_objects = 0;
        let mut total_interfaces = 0;
        let mut total_methods = 0;
        let mut total_properties = 0;
        let mut total_signals = 0;

        // Store service
        {
            let conn = self.conn.write().map_err(|e| anyhow::anyhow!("{}", e))?;
            conn.execute(
                "INSERT OR REPLACE INTO services (service_name, indexed_at) VALUES (?1, ?2)",
                params![service_name, timestamp],
            )?;
        }

        // Try common root paths (most services expose objects at "/" or service-specific paths)
        let service_path = format!("/{}", service_name.replace('.', "/"));
        let common_paths = vec!["/".to_string(), service_path];

        for object_path in &common_paths {
            match self
                .scanner
                .introspect(bus_type, service_name, object_path)
                .await
            {
                Ok(object_info) => {
                    self.store_object(service_name, &object_info, timestamp)?;
                    total_objects += 1;
                    total_interfaces += object_info.interfaces.len();

                    for interface in &object_info.interfaces {
                        total_methods += interface.methods.len();
                        total_properties += interface.properties.len();
                        total_signals += interface.signals.len();
                    }
                }
                Err(e) => {
                    debug!(
                        "Failed to introspect {}:{}: {}",
                        service_name, object_path, e
                    );
                }
            }
        }

        Ok((
            total_objects,
            total_interfaces,
            total_methods,
            total_properties,
            total_signals,
        ))
    }

    /// Store object and its interfaces in the database
    fn store_object(
        &self,
        service_name: &str,
        object_info: &ObjectInfo,
        timestamp: i64,
    ) -> Result<()> {
        let conn = self.conn.write().map_err(|e| anyhow::anyhow!("{}", e))?;

        // Insert object
        conn.execute(
            "INSERT INTO objects (service_name, object_path, indexed_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(service_name, object_path)
             DO UPDATE SET indexed_at = excluded.indexed_at",
            params![service_name, &object_info.path, timestamp],
        )?;

        let object_id: i64 = conn.query_row(
            "SELECT id FROM objects WHERE service_name = ?1 AND object_path = ?2",
            params![service_name, &object_info.path],
            |row| row.get(0),
        )?;

        // Insert interfaces and their members
        for interface in &object_info.interfaces {
            conn.execute(
                "INSERT INTO interfaces (object_id, interface_name, indexed_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(object_id, interface_name)
                 DO UPDATE SET indexed_at = excluded.indexed_at",
                params![object_id, &interface.name, timestamp],
            )?;

            let interface_id: i64 = conn.query_row(
                "SELECT id FROM interfaces WHERE object_id = ?1 AND interface_name = ?2",
                params![object_id, &interface.name],
                |row| row.get(0),
            )?;

            // Insert methods
            for method in &interface.methods {
                let input_sig = method
                    .in_args
                    .iter()
                    .map(|a| format!("{}:{}", a.name.as_deref().unwrap_or("arg"), a.signature))
                    .collect::<Vec<_>>()
                    .join(", ");
                let output_sig = method
                    .out_args
                    .iter()
                    .map(|a| format!("{}:{}", a.name.as_deref().unwrap_or("arg"), a.signature))
                    .collect::<Vec<_>>()
                    .join(", ");

                conn.execute(
                    "INSERT INTO methods
                     (interface_id, method_name, input_signature, output_signature, description)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(interface_id, method_name)
                     DO UPDATE SET
                        input_signature = excluded.input_signature,
                        output_signature = excluded.output_signature,
                        description = excluded.description",
                    params![
                        interface_id,
                        &method.name,
                        &input_sig,
                        &output_sig,
                        format!("{}.{}", interface.name, method.name)
                    ],
                )?;
            }

            // Insert properties
            for property in &interface.properties {
                let access_str = match property.access {
                    op_core::types::PropertyAccess::Read => "read",
                    op_core::types::PropertyAccess::Write => "write",
                    op_core::types::PropertyAccess::ReadWrite => "readwrite",
                };

                conn.execute(
                    "INSERT INTO properties
                     (interface_id, property_name, type_signature, access, description)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(interface_id, property_name)
                     DO UPDATE SET
                        type_signature = excluded.type_signature,
                        access = excluded.access,
                        description = excluded.description",
                    params![
                        interface_id,
                        &property.name,
                        &property.signature,
                        access_str,
                        format!("{}.{}", interface.name, property.name)
                    ],
                )?;
            }

            // Insert signals
            for signal in &interface.signals {
                let sig = signal
                    .args
                    .iter()
                    .map(|a| format!("{}:{}", a.name.as_deref().unwrap_or("arg"), a.signature))
                    .collect::<Vec<_>>()
                    .join(", ");

                conn.execute(
                    "INSERT INTO signals
                     (interface_id, signal_name, signature, description)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(interface_id, signal_name)
                     DO UPDATE SET
                        signature = excluded.signature,
                        description = excluded.description",
                    params![
                        interface_id,
                        &signal.name,
                        &sig,
                        format!("{}.{}", interface.name, signal.name)
                    ],
                )?;
            }
        }

        Ok(())
    }

    /// Store index statistics
    fn store_statistics(&self, stats: &IndexStatistics) -> Result<()> {
        let conn = self.conn.write().map_err(|e| anyhow::anyhow!("{}", e))?;

        conn.execute(
            "INSERT OR REPLACE INTO index_stats
             (id, total_services, total_objects, total_interfaces, total_methods,
              total_properties, total_signals, scan_duration_seconds, indexed_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                stats.total_services,
                stats.total_objects,
                stats.total_interfaces,
                stats.total_methods,
                stats.total_properties,
                stats.total_signals,
                stats.scan_duration_seconds,
                stats.indexed_at,
            ],
        )?;

        Ok(())
    }

    /// Get index statistics
    pub fn get_statistics(&self) -> Result<Option<IndexStatistics>> {
        let conn = self.conn.read().map_err(|e| anyhow::anyhow!("{}", e))?;

        let result = conn.query_row(
            "SELECT total_services, total_objects, total_interfaces, total_methods,
                    total_properties, total_signals, scan_duration_seconds, indexed_at
             FROM index_stats WHERE id = 1",
            [],
            |row| {
                Ok(IndexStatistics {
                    total_services: row.get(0)?,
                    total_objects: row.get(1)?,
                    total_interfaces: row.get(2)?,
                    total_methods: row.get(3)?,
                    total_properties: row.get(4)?,
                    total_signals: row.get(5)?,
                    scan_duration_seconds: row.get(6)?,
                    indexed_at: row.get(7)?,
                })
            },
        );

        match result {
            Ok(stats) => Ok(Some(stats)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Search methods using FTS5
    pub fn search_methods(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let conn = self.conn.read().map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut stmt = conn.prepare(
            "SELECT service_name, object_path, interface_name, method_name,
                    description, rank
             FROM methods_fts
             WHERE methods_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        let results = stmt
            .query_map(params![query, limit], |row| {
                Ok(SearchResult {
                    service: row.get(0)?,
                    object_path: row.get(1)?,
                    interface: row.get(2)?,
                    item_name: row.get(3)?,
                    item_type: "method".to_string(),
                    description: row.get(4)?,
                    relevance_score: row.get::<_, f64>(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }

    /// Search properties using FTS5
    pub fn search_properties(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let conn = self.conn.read().map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut stmt = conn.prepare(
            "SELECT service_name, object_path, interface_name, property_name,
                    description, rank
             FROM properties_fts
             WHERE properties_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        let results = stmt
            .query_map(params![query, limit], |row| {
                Ok(SearchResult {
                    service: row.get(0)?,
                    object_path: row.get(1)?,
                    interface: row.get(2)?,
                    item_name: row.get(3)?,
                    item_type: "property".to_string(),
                    description: row.get(4)?,
                    relevance_score: row.get::<_, f64>(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }

    /// Search everything (methods, properties, signals, interfaces)
    pub fn search_all(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let mut results = Vec::new();

        // Search methods
        results.extend(self.search_methods(query, limit / 4)?);

        // Search properties
        results.extend(self.search_properties(query, limit / 4)?);

        // Search signals
        let conn = self.conn.read().map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut stmt = conn.prepare(
            "SELECT service_name, object_path, interface_name, signal_name,
                    description, rank
             FROM signals_fts
             WHERE signals_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        let signals = stmt
            .query_map(params![query, limit / 4], |row| {
                Ok(SearchResult {
                    service: row.get(0)?,
                    object_path: row.get(1)?,
                    interface: row.get(2)?,
                    item_name: row.get(3)?,
                    item_type: "signal".to_string(),
                    description: row.get(4)?,
                    relevance_score: row.get::<_, f64>(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        results.extend(signals);

        // Sort by relevance
        results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Trim to limit
        results.truncate(limit);

        Ok(results)
    }

    /// Clear the entire index
    pub fn clear_index(&self) -> Result<()> {
        let conn = self.conn.write().map_err(|e| anyhow::anyhow!("{}", e))?;

        conn.execute_batch(
            "DELETE FROM methods_fts;
             DELETE FROM properties_fts;
             DELETE FROM signals_fts;
             DELETE FROM interfaces_fts;
             DELETE FROM signals;
             DELETE FROM properties;
             DELETE FROM methods;
             DELETE FROM interfaces;
             DELETE FROM objects;
             DELETE FROM services;
             DELETE FROM index_stats;",
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_indexer_creation() {
        let indexer = DbusIndexer::new(":memory:").await.unwrap();
        let stats = indexer.get_statistics().unwrap();
        assert!(stats.is_none());
    }
}
