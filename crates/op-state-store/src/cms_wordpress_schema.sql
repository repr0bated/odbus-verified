-- ===================================================================
-- FULL WORDPRESS CMS SCHEMA
-- Complete WordPress object classes for org.opdbus.cms
-- Status: LIVE AND UNFILLED (ready for CMS deployment)
-- WordPress powers 43% of all websites (most popular CMS)
-- ===================================================================

-- ===================================================================
-- Posts (Blog posts, pages, custom post types)
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPPost', NULL, TRUE,
       json('{
         "post_id": {"type": "int", "mandatory": true},
         "post_guid": {"type": "string", "mandatory": true},
         "post_type": {"type": "string", "mandatory": true},
         "post_title": {"type": "string", "mandatory": true},
         "post_content": {"type": "string"},
         "post_excerpt": {"type": "string"},
         "post_status": {"type": "string", "default": "draft"},
         "post_name": {"type": "string"},
         "post_author": {"type": "int"},
         "post_date": {"type": "string"},
         "post_modified": {"type": "string"},
         "post_parent": {"type": "int", "default": 0},
         "menu_order": {"type": "int", "default": 0},
         "comment_status": {"type": "string", "default": "open"},
         "ping_status": {"type": "string", "default": "open"},
         "comment_count": {"type": "int", "default": 0},
         "featured_image_id": {"type": "int"},
         "post_password": {"type": "string"},
         "post_meta": {"type": "dict"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Pages
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPPage', 'WPPost', TRUE,
       json('{
         "page_template": {"type": "string"},
         "parent_page_id": {"type": "int"},
         "is_front_page": {"type": "bool", "default": false},
         "is_posts_page": {"type": "bool", "default": false}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- WordPress Users
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPUser', NULL, TRUE,
       json('{
         "user_id": {"type": "int", "mandatory": true},
         "user_login": {"type": "string", "mandatory": true},
         "user_email": {"type": "string", "mandatory": true},
         "user_nicename": {"type": "string"},
         "display_name": {"type": "string"},
         "user_registered": {"type": "string"},
         "user_status": {"type": "int", "default": 0},
         "user_url": {"type": "string"},
         "role": {"type": "string", "default": "subscriber"},
         "capabilities": {"type": "dict"},
         "user_meta": {"type": "dict"},
         "first_name": {"type": "string"},
         "last_name": {"type": "string"},
         "nickname": {"type": "string"},
         "description": {"type": "string"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Comments
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPComment', NULL, TRUE,
       json('{
         "comment_id": {"type": "int", "mandatory": true},
         "comment_post_id": {"type": "int", "mandatory": true},
         "comment_author": {"type": "string"},
         "comment_author_email": {"type": "string"},
         "comment_author_url": {"type": "string"},
         "comment_author_ip": {"type": "string"},
         "comment_date": {"type": "string"},
         "comment_content": {"type": "string"},
         "comment_approved": {"type": "string", "default": "0"},
         "comment_parent": {"type": "int", "default": 0},
         "user_id": {"type": "int", "default": 0},
         "comment_type": {"type": "string"},
         "comment_meta": {"type": "dict"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Categories & Tags (Taxonomies)
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPTaxonomy', NULL, TRUE,
       json('{
         "taxonomy_name": {"type": "string", "mandatory": true},
         "taxonomy_label": {"type": "string"},
         "object_types": {"type": "array"},
         "hierarchical": {"type": "bool", "default": false},
         "public": {"type": "bool", "default": true},
         "show_ui": {"type": "bool", "default": true},
         "show_in_rest": {"type": "bool", "default": true}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPTerm', NULL, TRUE,
       json('{
         "term_id": {"type": "int", "mandatory": true},
         "term_name": {"type": "string", "mandatory": true},
         "term_slug": {"type": "string", "mandatory": true},
         "term_taxonomy": {"type": "string", "mandatory": true},
         "term_description": {"type": "string"},
         "parent_term_id": {"type": "int", "default": 0},
         "count": {"type": "int", "default": 0},
         "term_meta": {"type": "dict"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Menus
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPMenu', NULL, TRUE,
       json('{
         "menu_id": {"type": "int", "mandatory": true},
         "menu_name": {"type": "string", "mandatory": true},
         "menu_slug": {"type": "string", "mandatory": true},
         "menu_location": {"type": "string"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPMenuItem', NULL, TRUE,
       json('{
         "item_id": {"type": "int", "mandatory": true},
         "menu_id": {"type": "int", "mandatory": true},
         "parent_item_id": {"type": "int", "default": 0},
         "title": {"type": "string", "mandatory": true},
         "url": {"type": "string"},
         "target": {"type": "string"},
         "classes": {"type": "array"},
         "xfn": {"type": "string"},
         "description": {"type": "string"},
         "object_id": {"type": "int"},
         "object_type": {"type": "string"},
         "menu_order": {"type": "int", "default": 0}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Media Library
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPAttachment', 'WPPost', TRUE,
       json('{
         "attachment_url": {"type": "string", "mandatory": true},
         "attachment_file": {"type": "string"},
         "mime_type": {"type": "string"},
         "file_size": {"type": "int"},
         "width": {"type": "int"},
         "height": {"type": "int"},
         "alt_text": {"type": "string"},
         "caption": {"type": "string"},
         "description": {"type": "string"},
         "attached_to_post": {"type": "int"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Widgets
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPWidget', NULL, TRUE,
       json('{
         "widget_id": {"type": "string", "mandatory": true},
         "widget_name": {"type": "string", "mandatory": true},
         "widget_class": {"type": "string"},
         "sidebar_id": {"type": "string"},
         "widget_position": {"type": "int"},
         "widget_options": {"type": "dict"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Sidebars
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPSidebar', NULL, TRUE,
       json('{
         "sidebar_id": {"type": "string", "mandatory": true},
         "sidebar_name": {"type": "string", "mandatory": true},
         "description": {"type": "string"},
         "before_widget": {"type": "string"},
         "after_widget": {"type": "string"},
         "before_title": {"type": "string"},
         "after_title": {"type": "string"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Themes
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPTheme', NULL, TRUE,
       json('{
         "theme_slug": {"type": "string", "mandatory": true},
         "theme_name": {"type": "string", "mandatory": true},
         "theme_uri": {"type": "string"},
         "author": {"type": "string"},
         "author_uri": {"type": "string"},
         "description": {"type": "string"},
         "version": {"type": "string"},
         "template": {"type": "string"},
         "status": {"type": "string"},
         "tags": {"type": "array"},
         "text_domain": {"type": "string"},
         "screenshot": {"type": "string"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Plugins
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPPlugin', NULL, TRUE,
       json('{
         "plugin_file": {"type": "string", "mandatory": true},
         "plugin_name": {"type": "string", "mandatory": true},
         "plugin_uri": {"type": "string"},
         "description": {"type": "string"},
         "version": {"type": "string"},
         "author": {"type": "string"},
         "author_uri": {"type": "string"},
         "network": {"type": "bool", "default": false},
         "requires_wp": {"type": "string"},
         "requires_php": {"type": "string"},
         "text_domain": {"type": "string"},
         "active": {"type": "bool", "default": false}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Options (Site Settings)
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPOption', NULL, TRUE,
       json('{
         "option_name": {"type": "string", "mandatory": true},
         "option_value": {"type": "string"},
         "autoload": {"type": "string", "default": "yes"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Site Configuration
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPSiteConfig', NULL, TRUE,
       json('{
         "site_url": {"type": "string", "mandatory": true},
         "home_url": {"type": "string"},
         "site_title": {"type": "string"},
         "tagline": {"type": "string"},
         "admin_email": {"type": "string"},
         "timezone": {"type": "string"},
         "date_format": {"type": "string"},
         "time_format": {"type": "string"},
         "language": {"type": "string", "default": "en_US"},
         "permalink_structure": {"type": "string"},
         "posts_per_page": {"type": "int", "default": 10},
         "comments_enabled": {"type": "bool", "default": true},
         "default_role": {"type": "string", "default": "subscriber"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Multisite Network (for WordPress Multisite)
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPNetwork', NULL, TRUE,
       json('{
         "network_id": {"type": "int", "mandatory": true},
         "network_name": {"type": "string", "mandatory": true},
         "domain": {"type": "string", "mandatory": true},
         "path": {"type": "string", "default": "/"},
         "sites": {"type": "array"},
         "cookie_domain": {"type": "string"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPSite', NULL, TRUE,
       json('{
         "site_id": {"type": "int", "mandatory": true},
         "blog_id": {"type": "int", "mandatory": true},
         "network_id": {"type": "int"},
         "domain": {"type": "string", "mandatory": true},
         "path": {"type": "string"},
         "registered": {"type": "string"},
         "last_updated": {"type": "string"},
         "public": {"type": "bool", "default": true},
         "archived": {"type": "bool", "default": false},
         "mature": {"type": "bool", "default": false},
         "spam": {"type": "bool", "default": false},
         "deleted": {"type": "bool", "default": false},
         "lang_id": {"type": "int", "default": 0}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- Custom Fields (Post Meta)
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WPCustomField', NULL, TRUE,
       json('{
         "meta_id": {"type": "int", "mandatory": true},
         "post_id": {"type": "int", "mandatory": true},
         "meta_key": {"type": "string", "mandatory": true},
         "meta_value": {"type": "string"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

-- ===================================================================
-- WooCommerce (E-commerce extension) - most popular plugin
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WCProduct', 'WPPost', TRUE,
       json('{
         "product_type": {"type": "string", "default": "simple"},
         "sku": {"type": "string"},
         "regular_price": {"type": "string"},
         "sale_price": {"type": "string"},
         "stock_quantity": {"type": "int"},
         "stock_status": {"type": "string", "default": "instock"},
         "manage_stock": {"type": "bool", "default": false},
         "weight": {"type": "string"},
         "length": {"type": "string"},
         "width": {"type": "string"},
         "height": {"type": "string"},
         "shipping_class": {"type": "string"},
         "tax_status": {"type": "string", "default": "taxable"},
         "tax_class": {"type": "string"},
         "downloadable": {"type": "bool", "default": false},
         "virtual": {"type": "bool", "default": false},
         "product_attributes": {"type": "dict"},
         "variations": {"type": "array"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';

INSERT OR IGNORE INTO object_classes (interface_id, class_name, parent_class, structural, attributes_schema)
SELECT id, 'WCOrder', 'WPPost', TRUE,
       json('{
         "order_number": {"type": "string"},
         "order_status": {"type": "string", "default": "pending"},
         "customer_id": {"type": "int"},
         "billing_address": {"type": "dict"},
         "shipping_address": {"type": "dict"},
         "payment_method": {"type": "string"},
         "payment_method_title": {"type": "string"},
         "transaction_id": {"type": "string"},
         "customer_ip": {"type": "string"},
         "customer_user_agent": {"type": "string"},
         "order_currency": {"type": "string"},
         "order_total": {"type": "string"},
         "order_subtotal": {"type": "string"},
         "order_tax": {"type": "string"},
         "order_shipping": {"type": "string"},
         "order_discount": {"type": "string"},
         "line_items": {"type": "array"},
         "shipping_lines": {"type": "array"},
         "fee_lines": {"type": "array"},
         "coupon_lines": {"type": "array"}
       }')
FROM service_interfaces WHERE interface_name = 'org.opdbus.cms.Manager';
