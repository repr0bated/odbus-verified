-- ===================================================================
-- Operation D-Bus Namespace Schema
-- Enterprise-ready schema for org.opdbus.* services
-- Status: LIVE AND UNFILLED (tables ready, will be populated)
-- ===================================================================

-- Core namespace services (org.opdbus.*)
CREATE TABLE IF NOT EXISTS namespace_services (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    service_name TEXT NOT NULL UNIQUE,  -- e.g., "org.opdbus.network"
    description TEXT,
    version TEXT DEFAULT 'v1',
    enabled BOOLEAN DEFAULT TRUE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Service interfaces (methods, properties, signals)
CREATE TABLE IF NOT EXISTS service_interfaces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    service_id INTEGER NOT NULL REFERENCES namespace_services(id) ON DELETE CASCADE,
    interface_name TEXT NOT NULL,  -- e.g., "org.opdbus.network.Manager"
    version TEXT DEFAULT 'v1',
    methods_schema TEXT,  -- JSON: method definitions
    signals_schema TEXT,  -- JSON: signal definitions
    properties_schema TEXT, -- JSON: property definitions
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(service_id, interface_name, version)
);

-- Object classes (for directory/LDAP integration)
CREATE TABLE IF NOT EXISTS object_classes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    interface_id INTEGER NOT NULL REFERENCES service_interfaces(id) ON DELETE CASCADE,
    class_name TEXT NOT NULL,  -- e.g., "NetworkInterface", "User", "Group"
    ldap_oid TEXT,  -- LDAP Object Identifier (for AD migration)
    parent_class TEXT,  -- Inheritance support
    structural BOOLEAN DEFAULT TRUE,
    attributes_schema TEXT,  -- JSON: attribute definitions
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(interface_id, class_name)
);

-- Attribute definitions
CREATE TABLE IF NOT EXISTS attribute_definitions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    class_id INTEGER NOT NULL REFERENCES object_classes(id) ON DELETE CASCADE,
    attribute_name TEXT NOT NULL,
    ldap_name TEXT,  -- Original LDAP attribute name (for migration)
    attribute_type TEXT NOT NULL,  -- "string", "int", "bool", "array", "dict"
    single_valued BOOLEAN DEFAULT TRUE,
    mandatory BOOLEAN DEFAULT FALSE,
    default_value TEXT,
    validation_schema TEXT,  -- JSON: validation rules
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(class_id, attribute_name)
);

-- Live objects (actual D-Bus object instances)
CREATE TABLE IF NOT EXISTS live_objects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    service_id INTEGER NOT NULL REFERENCES namespace_services(id) ON DELETE CASCADE,
    object_path TEXT NOT NULL,  -- D-Bus object path: /org/opdbus/network/connection/eth0
    object_class TEXT NOT NULL,  -- Class name
    state TEXT NOT NULL,  -- JSON: current object state
    metadata TEXT,  -- JSON: metadata (owner, tags, etc.)
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(service_id, object_path)
);

-- Live links (relationships between objects)
CREATE TABLE IF NOT EXISTS live_links (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_object_id INTEGER NOT NULL REFERENCES live_objects(id) ON DELETE CASCADE,
    target_object_id INTEGER NOT NULL REFERENCES live_objects(id) ON DELETE CASCADE,
    link_type TEXT NOT NULL,  -- "contains", "references", "depends_on", etc.
    metadata TEXT,  -- JSON: additional link data
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(source_object_id, target_object_id, link_type)
);

-- Change log (audit trail for all object changes)
CREATE TABLE IF NOT EXISTS namespace_change_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    object_id INTEGER REFERENCES live_objects(id) ON DELETE SET NULL,
    object_path TEXT NOT NULL,
    change_type TEXT NOT NULL,  -- "created", "updated", "deleted", "property_changed"
    old_state TEXT,  -- JSON: state before change
    new_state TEXT,  -- JSON: state after change
    changed_by TEXT,  -- User/service that made the change
    timestamp TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Subscriptions (for real-time updates)
CREATE TABLE IF NOT EXISTS object_subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subscriber_id TEXT NOT NULL,  -- Session/client ID
    object_id INTEGER REFERENCES live_objects(id) ON DELETE CASCADE,
    object_path_pattern TEXT,  -- Glob pattern: /org/opdbus/network/*
    event_types TEXT NOT NULL,  -- JSON array: ["property_changed", "signal"]
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ===================================================================
-- LDAP/Active Directory Migration Tables
-- ===================================================================

-- LDAP schemas (imported from existing infrastructure)
CREATE TABLE IF NOT EXISTS ldap_schemas (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_domain TEXT NOT NULL,
    schema_source TEXT NOT NULL,  -- "active_directory", "openldap", "freeipa", "custom"
    imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    raw_schema TEXT NOT NULL,  -- JSON: raw LDAP schema from introspection
    metadata TEXT  -- JSON: import metadata
);

-- Migrated objects (tracking AD → op-dbus migration)
CREATE TABLE IF NOT EXISTS migrated_objects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ldap_schema_id INTEGER NOT NULL REFERENCES ldap_schemas(id) ON DELETE CASCADE,
    source_dn TEXT NOT NULL,  -- Original Distinguished Name
    target_service TEXT NOT NULL,  -- e.g., "org.opdbus.directory"
    target_object_id INTEGER REFERENCES live_objects(id) ON DELETE SET NULL,
    target_class TEXT NOT NULL,  -- Converted class name
    migration_map TEXT NOT NULL,  -- JSON: attribute mapping
    migrated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(ldap_schema_id, source_dn)
);

-- Migration rules (reusable mappings)
CREATE TABLE IF NOT EXISTS migration_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_name TEXT NOT NULL UNIQUE,
    source_schema TEXT NOT NULL,  -- "active_directory", "openldap", etc.
    source_object_class TEXT NOT NULL,  -- LDAP objectClass
    target_service TEXT NOT NULL,  -- org.opdbus.* service
    target_class TEXT NOT NULL,  -- Target class name
    attribute_mappings TEXT NOT NULL,  -- JSON: LDAP attr → op-dbus attr mappings
    transformation_rules TEXT,  -- JSON: data transformation rules
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ===================================================================
-- Indices for Performance
-- ===================================================================

CREATE INDEX IF NOT EXISTS idx_service_interfaces_service ON service_interfaces(service_id);
CREATE INDEX IF NOT EXISTS idx_object_classes_interface ON object_classes(interface_id);
CREATE INDEX IF NOT EXISTS idx_attribute_definitions_class ON attribute_definitions(class_id);
CREATE INDEX IF NOT EXISTS idx_live_objects_service ON live_objects(service_id);
CREATE INDEX IF NOT EXISTS idx_live_objects_path ON live_objects(object_path);
CREATE INDEX IF NOT EXISTS idx_live_links_source ON live_links(source_object_id);
CREATE INDEX IF NOT EXISTS idx_live_links_target ON live_links(target_object_id);
CREATE INDEX IF NOT EXISTS idx_change_log_object ON namespace_change_log(object_id);
CREATE INDEX IF NOT EXISTS idx_change_log_timestamp ON namespace_change_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_subscriptions_subscriber ON object_subscriptions(subscriber_id);
CREATE INDEX IF NOT EXISTS idx_migrated_objects_ldap ON migrated_objects(ldap_schema_id);
CREATE INDEX IF NOT EXISTS idx_migrated_objects_dn ON migrated_objects(source_dn);

-- ===================================================================
-- Pre-populated Namespace Services (Templates)
-- ===================================================================

-- Insert core org.opdbus.* services (LIVE AND UNFILLED)
INSERT OR IGNORE INTO namespace_services (service_name, description, version) VALUES
    ('org.opdbus.hardware', 'Hardware management (IPMI/BMC compatible)', 'v1'),
    ('org.opdbus.network', 'Network management (replaces NetworkManager)', 'v1'),
    ('org.opdbus.container', 'Container management (Docker/Podman)', 'v1'),
    ('org.opdbus.session', 'Session management', 'v1'),
    ('org.opdbus.policy', 'Policy management and enforcement', 'v1'),
    ('org.opdbus.config', 'Configuration management', 'v1'),
    ('org.opdbus.directory', 'Directory services (LDAP/AD integration)', 'v1'),
    ('org.opdbus.ipmi', 'IPMI protocol wrapper (enterprise compatibility)', 'v1'),
    ('org.opdbus.bmc', 'BMC interface wrapper (enterprise compatibility)', 'v1'),
    ('org.opdbus.storage', 'Storage management (iSCSI/NFS/Ceph)', 'v1'),
    ('org.opdbus.monitoring', 'Monitoring and metrics collection', 'v1'),
    ('org.opdbus.backup', 'Backup and restore services', 'v1'),
    ('org.opdbus.security', 'Security and access control', 'v1'),
    ('org.opdbus.virtualization', 'VM and hypervisor management', 'v1'),
    ('org.opdbus.cluster', 'Cluster coordination (Pacemaker/Corosync)', 'v1');

-- ===================================================================
-- Pre-populated Migration Rules (Active Directory Templates)
-- ===================================================================

-- AD User → op-dbus.directory User
INSERT OR IGNORE INTO migration_rules (rule_name, source_schema, source_object_class, target_service, target_class, attribute_mappings) VALUES
    ('ad_user_to_opdbus',
     'active_directory',
     'user',
     'org.opdbus.directory',
     'User',
     json('{"sAMAccountName": "username", "displayName": "full_name", "mail": "email", "telephoneNumber": "phone", "department": "department", "title": "job_title", "manager": "manager_dn", "memberOf": "groups"}'));

-- AD Group → op-dbus.directory Group
INSERT OR IGNORE INTO migration_rules (rule_name, source_schema, source_object_class, target_service, target_class, attribute_mappings) VALUES
    ('ad_group_to_opdbus',
     'active_directory',
     'group',
     'org.opdbus.directory',
     'Group',
     json('{"sAMAccountName": "group_name", "description": "description", "member": "members", "managedBy": "manager_dn"}'));

-- AD Computer → op-dbus.hardware Device
INSERT OR IGNORE INTO migration_rules (rule_name, source_schema, source_object_class, target_service, target_class, attribute_mappings) VALUES
    ('ad_computer_to_opdbus',
     'active_directory',
     'computer',
     'org.opdbus.hardware',
     'Device',
     json('{"dNSHostName": "hostname", "operatingSystem": "os", "operatingSystemVersion": "os_version", "description": "description", "location": "physical_location"}'));

-- OpenLDAP posixAccount → op-dbus.directory User
INSERT OR IGNORE INTO migration_rules (rule_name, source_schema, source_object_class, target_service, target_class, attribute_mappings) VALUES
    ('openldap_posix_to_opdbus',
     'openldap',
     'posixAccount',
     'org.opdbus.directory',
     'User',
     json('{"uid": "username", "cn": "full_name", "mail": "email", "uidNumber": "uid", "gidNumber": "gid", "homeDirectory": "home_dir", "loginShell": "shell"}'));
