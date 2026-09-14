-- ===================================================================
-- FULL DRUPAL CMS SCHEMA
-- Complete Drupal object classes for org.opdbus.cms
-- Status: LIVE AND UNFILLED (ready for CMS deployment)
-- ===================================================================

-- Add CMS service if not exists
INSERT OR IGNORE INTO namespace_services (service_name, description, version)
VALUES ('org.opdbus.cms', 'Content Management System (Drupal-compatible)', 'v1');

-- Create CMS interface
INSERT OR IGNORE INTO service_interfaces (service_id, interface_name, version, methods_schema, signals_schema, properties_schema)
SELECT id, 'org.opdbus.cms.Manager', 'v1',
       json('{"CreateContent": {"params": ["content_type", "data"], "returns": "node_id"}, "UpdateContent": {"params": ["node_id", "data"]}, "DeleteContent": {"params": ["node_id"]}, "PublishContent": {"params": ["node_id"]}}'),
       json('{"ContentCreated": {"params": ["node_id", "content_type"]}, "ContentPublished": {"params": ["node_id"]}, "ContentDeleted": {"params": ["node_id"]}}'),
       json('{"TotalNodes": {"type": "int", "access": "read"}, "PublishedNodes": {"type": "int", "access": "read"}}')
FROM namespace_services WHERE service_name = 'org.opdbus.cms';

-- ===================================================================
-- Content Types
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'ContentType', NULL, TRUE,
       json('{
         "type_name": {"type": "string", "mandatory": true},
         "machine_name": {"type": "string", "mandatory": true},
         "description": {"type": "string"},
         "has_title": {"type": "bool", "default": true},
         "has_body": {"type": "bool", "default": true},
         "is_translatable": {"type": "bool", "default": false},
         "field_definitions": {"type": "array"},
         "display_settings": {"type": "dict"},
         "form_settings": {"type": "dict"},
         "created": {"type": "string"},
         "modified": {"type": "string"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Nodes (Content Items)
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'Node', NULL, TRUE,
       json('{
         "node_id": {"type": "int", "mandatory": true},
         "uuid": {"type": "string", "mandatory": true},
         "content_type": {"type": "string", "mandatory": true},
         "title": {"type": "string", "mandatory": true},
         "body": {"type": "string"},
         "summary": {"type": "string"},
         "language": {"type": "string", "default": "en"},
         "status": {"type": "string", "default": "draft"},
         "promoted": {"type": "bool", "default": false},
         "sticky": {"type": "bool", "default": false},
         "author_uid": {"type": "int"},
         "created": {"type": "string"},
         "modified": {"type": "string"},
         "published": {"type": "string"},
         "field_values": {"type": "dict"},
         "revision_id": {"type": "int"},
         "revision_log": {"type": "string"},
         "path_alias": {"type": "string"},
         "menu_link": {"type": "dict"},
         "moderation_state": {"type": "string"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- CMS Users
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'CMSUser', NULL, TRUE,
       json('{
         "uid": {"type": "int", "mandatory": true},
         "username": {"type": "string", "mandatory": true},
         "email": {"type": "string", "mandatory": true},
         "display_name": {"type": "string"},
         "password_hash": {"type": "string"},
         "roles": {"type": "array"},
         "status": {"type": "string", "default": "active"},
         "timezone": {"type": "string"},
         "language": {"type": "string", "default": "en"},
         "created": {"type": "string"},
         "last_login": {"type": "string"},
         "last_access": {"type": "string"},
         "picture": {"type": "string"},
         "signature": {"type": "string"},
         "profile_fields": {"type": "dict"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Roles & Permissions
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'Role', NULL, TRUE,
       json('{
         "role_id": {"type": "int", "mandatory": true},
         "role_name": {"type": "string", "mandatory": true},
         "machine_name": {"type": "string", "mandatory": true},
         "weight": {"type": "int", "default": 0},
         "permissions": {"type": "array"},
         "is_admin": {"type": "bool", "default": false}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Taxonomy Vocabularies
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'TaxonomyVocabulary', NULL, TRUE,
       json('{
         "vocabulary_id": {"type": "int", "mandatory": true},
         "vocabulary_name": {"type": "string", "mandatory": true},
         "machine_name": {"type": "string", "mandatory": true},
         "description": {"type": "string"},
         "hierarchy": {"type": "string", "default": "single"},
         "weight": {"type": "int", "default": 0},
         "field_definitions": {"type": "array"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Taxonomy Terms
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'TaxonomyTerm', NULL, TRUE,
       json('{
         "term_id": {"type": "int", "mandatory": true},
         "term_name": {"type": "string", "mandatory": true},
         "vocabulary_id": {"type": "int", "mandatory": true},
         "description": {"type": "string"},
         "parent_term_id": {"type": "int"},
         "weight": {"type": "int", "default": 0},
         "path_alias": {"type": "string"},
         "field_values": {"type": "dict"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Menus
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'Menu', NULL, TRUE,
       json('{
         "menu_id": {"type": "string", "mandatory": true},
         "menu_name": {"type": "string", "mandatory": true},
         "description": {"type": "string"},
         "language": {"type": "string"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Menu Links
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'MenuLink', NULL, TRUE,
       json('{
         "link_id": {"type": "int", "mandatory": true},
         "menu_id": {"type": "string", "mandatory": true},
         "parent_link_id": {"type": "int"},
         "title": {"type": "string", "mandatory": true},
         "url": {"type": "string", "mandatory": true},
         "description": {"type": "string"},
         "enabled": {"type": "bool", "default": true},
         "expanded": {"type": "bool", "default": false},
         "weight": {"type": "int", "default": 0},
         "external": {"type": "bool", "default": false}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Blocks
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'Block', NULL, TRUE,
       json('{
         "block_id": {"type": "string", "mandatory": true},
         "block_type": {"type": "string", "mandatory": true},
         "label": {"type": "string"},
         "theme": {"type": "string"},
         "region": {"type": "string"},
         "weight": {"type": "int", "default": 0},
         "visibility": {"type": "dict"},
         "settings": {"type": "dict"},
         "status": {"type": "bool", "default": true}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Views (Listings/Queries)
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'View', NULL, TRUE,
       json('{
         "view_id": {"type": "string", "mandatory": true},
         "view_name": {"type": "string", "mandatory": true},
         "description": {"type": "string"},
         "base_table": {"type": "string"},
         "displays": {"type": "array"},
         "filters": {"type": "array"},
         "sorts": {"type": "array"},
         "fields": {"type": "array"},
         "relationships": {"type": "array"},
         "pager": {"type": "dict"},
         "access": {"type": "dict"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Fields
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'Field', NULL, TRUE,
       json('{
         "field_name": {"type": "string", "mandatory": true},
         "field_type": {"type": "string", "mandatory": true},
         "label": {"type": "string"},
         "description": {"type": "string"},
         "required": {"type": "bool", "default": false},
         "cardinality": {"type": "int", "default": 1},
         "default_value": {"type": "string"},
         "widget_type": {"type": "string"},
         "widget_settings": {"type": "dict"},
         "formatter_type": {"type": "string"},
         "formatter_settings": {"type": "dict"},
         "storage_settings": {"type": "dict"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Files & Media
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'File', NULL, TRUE,
       json('{
         "file_id": {"type": "int", "mandatory": true},
         "uuid": {"type": "string", "mandatory": true},
         "filename": {"type": "string", "mandatory": true},
         "uri": {"type": "string", "mandatory": true},
         "filemime": {"type": "string"},
         "filesize": {"type": "int"},
         "status": {"type": "bool", "default": true},
         "created": {"type": "string"},
         "modified": {"type": "string"},
         "owner_uid": {"type": "int"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'Media', NULL, TRUE,
       json('{
         "media_id": {"type": "int", "mandatory": true},
         "uuid": {"type": "string", "mandatory": true},
         "media_type": {"type": "string", "mandatory": true},
         "name": {"type": "string", "mandatory": true},
         "file_id": {"type": "int"},
         "thumbnail_uri": {"type": "string"},
         "status": {"type": "bool", "default": true},
         "created": {"type": "string"},
         "modified": {"type": "string"},
         "owner_uid": {"type": "int"},
         "field_values": {"type": "dict"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Comments
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'Comment', NULL, TRUE,
       json('{
         "comment_id": {"type": "int", "mandatory": true},
         "entity_type": {"type": "string", "mandatory": true},
         "entity_id": {"type": "int", "mandatory": true},
         "parent_comment_id": {"type": "int"},
         "subject": {"type": "string"},
         "body": {"type": "string"},
         "author_uid": {"type": "int"},
         "author_name": {"type": "string"},
         "author_email": {"type": "string"},
         "status": {"type": "string", "default": "published"},
         "created": {"type": "string"},
         "modified": {"type": "string"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Workflows
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'Workflow', NULL, TRUE,
       json('{
         "workflow_id": {"type": "string", "mandatory": true},
         "workflow_name": {"type": "string", "mandatory": true},
         "description": {"type": "string"},
         "content_types": {"type": "array"},
         "states": {"type": "array"},
         "transitions": {"type": "array"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Configuration Objects
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'SiteConfig', NULL, TRUE,
       json('{
         "config_name": {"type": "string", "mandatory": true},
         "site_name": {"type": "string"},
         "site_slogan": {"type": "string"},
         "site_email": {"type": "string"},
         "default_language": {"type": "string"},
         "default_timezone": {"type": "string"},
         "maintenance_mode": {"type": "bool", "default": false},
         "cache_enabled": {"type": "bool", "default": true},
         "settings": {"type": "dict"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';
