-- ===================================================================
-- FULL ACTIVE DIRECTORY REPLACEMENT SCHEMA
-- Complete AD object classes for org.opdbus.directory
-- Status: LIVE AND UNFILLED (ready for enterprise deployment)
-- ===================================================================

-- Create Directory service interface if not exists
INSERT OR IGNORE INTO service_interfaces (service_id, interface_name, version, methods_schema, signals_schema, properties_schema)
SELECT id, 'org.opdbus.directory.Manager', 'v1',
       json('{"CreateUser": {"params": ["user_data"], "returns": "user_id"}, "CreateGroup": {"params": ["group_data"], "returns": "group_id"}, "AddUserToGroup": {"params": ["user_id", "group_id"]}, "AuthenticateUser": {"params": ["username", "password"], "returns": "bool"}}'),
       json('{"UserCreated": {"params": ["user_id", "username"]}, "UserDeleted": {"params": ["user_id"]}, "GroupCreated": {"params": ["group_id", "group_name"]}}'),
       json('{"TotalUsers": {"type": "int", "access": "read"}, "TotalGroups": {"type": "int", "access": "read"}}')
FROM namespace_services WHERE service_name = 'org.opdbus.directory';

-- ===================================================================
-- AD Domain/Forest Structure
-- ===================================================================

-- Domain objects
INSERT OR IGNORE INTO object_classes (interface_id, class_name, ldap_oid, parent_class, structural, attributes_schema)
SELECT id, 'Domain', '1.2.840.113556.1.5.2', NULL, TRUE,
       json('{"domain_name": {"type": "string", "mandatory": true}, "dns_root": {"type": "string"}, "forest_name": {"type": "string"}, "functional_level": {"type": "int"}, "domain_sid": {"type": "string"}, "netbios_name": {"type": "string"}}')
FROM service_interfaces WHERE interface_name LIKE 'org.opdbus.directory%' LIMIT 1;

-- Organizational Units
INSERT OR IGNORE INTO object_classes (interface_id, class_name, ldap_oid, parent_class, structural, attributes_schema)
SELECT id, 'OrganizationalUnit', '2.5.6.5', NULL, TRUE,
       json('{"ou_name": {"type": "string", "mandatory": true}, "description": {"type": "string"}, "parent_dn": {"type": "string"}, "gpo_links": {"type": "array"}, "managed_by": {"type": "string"}}')
FROM service_interfaces WHERE interface_name LIKE 'org.opdbus.directory%' LIMIT 1;

-- Sites (for multi-site AD)
INSERT OR IGNORE INTO object_classes (interface_id, class_name, ldap_oid, parent_class, structural, attributes_schema)
SELECT id, 'Site', '1.2.840.113556.1.3.14', NULL, TRUE,
       json('{"site_name": {"type": "string", "mandatory": true}, "description": {"type": "string"}, "subnets": {"type": "array"}, "site_links": {"type": "array"}, "location": {"type": "string"}}')
FROM service_interfaces WHERE interface_name LIKE 'org.opdbus.directory%' LIMIT 1;

-- ===================================================================
-- Complete AD User Schema
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, ldap_oid, parent_class, structural, attributes_schema)
SELECT id, 'ADUser', '1.2.840.113556.1.5.9', NULL, TRUE,
       json('{
         "username": {"type": "string", "mandatory": true, "ldap": "sAMAccountName"},
         "user_principal_name": {"type": "string", "ldap": "userPrincipalName"},
         "display_name": {"type": "string", "ldap": "displayName"},
         "given_name": {"type": "string", "ldap": "givenName"},
         "surname": {"type": "string", "ldap": "sn"},
         "email": {"type": "string", "ldap": "mail"},
         "telephone": {"type": "string", "ldap": "telephoneNumber"},
         "mobile": {"type": "string", "ldap": "mobile"},
         "department": {"type": "string", "ldap": "department"},
         "title": {"type": "string", "ldap": "title"},
         "company": {"type": "string", "ldap": "company"},
         "manager": {"type": "string", "ldap": "manager"},
         "direct_reports": {"type": "array", "ldap": "directReports"},
         "office": {"type": "string", "ldap": "physicalDeliveryOfficeName"},
         "street_address": {"type": "string", "ldap": "streetAddress"},
         "city": {"type": "string", "ldap": "l"},
         "state": {"type": "string", "ldap": "st"},
         "postal_code": {"type": "string", "ldap": "postalCode"},
         "country": {"type": "string", "ldap": "co"},
         "home_directory": {"type": "string", "ldap": "homeDirectory"},
         "home_drive": {"type": "string", "ldap": "homeDrive"},
         "logon_script": {"type": "string", "ldap": "scriptPath"},
         "profile_path": {"type": "string", "ldap": "profilePath"},
         "member_of": {"type": "array", "ldap": "memberOf"},
         "account_expires": {"type": "string", "ldap": "accountExpires"},
         "password_last_set": {"type": "string", "ldap": "pwdLastSet"},
         "last_logon": {"type": "string", "ldap": "lastLogon"},
         "last_logon_timestamp": {"type": "string", "ldap": "lastLogonTimestamp"},
         "bad_password_count": {"type": "int", "ldap": "badPwdCount"},
         "user_account_control": {"type": "int", "ldap": "userAccountControl"},
         "object_sid": {"type": "string", "ldap": "objectSid"},
         "object_guid": {"type": "string", "ldap": "objectGUID"},
         "when_created": {"type": "string", "ldap": "whenCreated"},
         "when_changed": {"type": "string", "ldap": "whenChanged"}
       }')
FROM service_interfaces WHERE interface_name LIKE 'org.opdbus.directory%' LIMIT 1;

-- ===================================================================
-- Complete AD Group Schema
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, ldap_oid, parent_class, structural, attributes_schema)
SELECT id, 'ADGroup', '1.2.840.113556.1.5.8', NULL, TRUE,
       json('{
         "group_name": {"type": "string", "mandatory": true, "ldap": "sAMAccountName"},
         "display_name": {"type": "string", "ldap": "displayName"},
         "description": {"type": "string", "ldap": "description"},
         "group_type": {"type": "int", "ldap": "groupType"},
         "group_scope": {"type": "string"},
         "members": {"type": "array", "ldap": "member"},
         "member_of": {"type": "array", "ldap": "memberOf"},
         "managed_by": {"type": "string", "ldap": "managedBy"},
         "mail": {"type": "string", "ldap": "mail"},
         "object_sid": {"type": "string", "ldap": "objectSid"},
         "when_created": {"type": "string", "ldap": "whenCreated"},
         "when_changed": {"type": "string", "ldap": "whenChanged"}
       }')
FROM service_interfaces WHERE interface_name LIKE 'org.opdbus.directory%' LIMIT 1;

-- ===================================================================
-- AD Computer Objects
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, ldap_oid, parent_class, structural, attributes_schema)
SELECT id, 'ADComputer', '1.2.840.113556.1.5.9', NULL, TRUE,
       json('{
         "computer_name": {"type": "string", "mandatory": true, "ldap": "sAMAccountName"},
         "dns_hostname": {"type": "string", "ldap": "dNSHostName"},
         "operating_system": {"type": "string", "ldap": "operatingSystem"},
         "os_version": {"type": "string", "ldap": "operatingSystemVersion"},
         "os_service_pack": {"type": "string", "ldap": "operatingSystemServicePack"},
         "description": {"type": "string", "ldap": "description"},
         "location": {"type": "string", "ldap": "location"},
         "managed_by": {"type": "string", "ldap": "managedBy"},
         "member_of": {"type": "array", "ldap": "memberOf"},
         "last_logon": {"type": "string", "ldap": "lastLogon"},
         "last_logon_timestamp": {"type": "string", "ldap": "lastLogonTimestamp"},
         "password_last_set": {"type": "string", "ldap": "pwdLastSet"},
         "object_sid": {"type": "string", "ldap": "objectSid"},
         "when_created": {"type": "string", "ldap": "whenCreated"}
       }')
FROM service_interfaces WHERE interface_name LIKE 'org.opdbus.directory%' LIMIT 1;

-- ===================================================================
-- Group Policy Objects (GPOs)
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, ldap_oid, parent_class, structural, attributes_schema)
SELECT id, 'GroupPolicyObject', '1.2.840.113556.1.5.4', NULL, TRUE,
       json('{
         "gpo_name": {"type": "string", "mandatory": true, "ldap": "displayName"},
         "gpo_guid": {"type": "string", "mandatory": true},
         "gpo_status": {"type": "string"},
         "version_number": {"type": "int"},
         "computer_version": {"type": "int"},
         "user_version": {"type": "int"},
         "wmi_filter": {"type": "string"},
         "linked_ous": {"type": "array"},
         "security_filtering": {"type": "array"},
         "settings": {"type": "dict"},
         "when_created": {"type": "string", "ldap": "whenCreated"},
         "when_changed": {"type": "string", "ldap": "whenChanged"}
       }')
FROM service_interfaces WHERE interface_name LIKE 'org.opdbus.directory%' LIMIT 1;

-- ===================================================================
-- Contacts
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, ldap_oid, parent_class, structural, attributes_schema)
SELECT id, 'Contact', '2.5.6.6', NULL, TRUE,
       json('{
         "display_name": {"type": "string", "mandatory": true, "ldap": "displayName"},
         "email": {"type": "string", "ldap": "mail"},
         "telephone": {"type": "string", "ldap": "telephoneNumber"},
         "company": {"type": "string", "ldap": "company"},
         "title": {"type": "string", "ldap": "title"},
         "description": {"type": "string", "ldap": "description"}
       }')
FROM service_interfaces WHERE interface_name LIKE 'org.opdbus.directory%' LIMIT 1;

-- ===================================================================
-- Service Accounts
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, ldap_oid, parent_class, structural, attributes_schema)
SELECT id, 'ServiceAccount', '1.2.840.113556.1.5.9', 'ADUser', TRUE,
       json('{
         "service_name": {"type": "string", "mandatory": true},
         "service_type": {"type": "string"},
         "managed_service_account": {"type": "bool"},
         "group_managed": {"type": "bool"},
         "spn": {"type": "array", "ldap": "servicePrincipalName"},
         "associated_computers": {"type": "array"}
       }')
FROM service_interfaces WHERE interface_name LIKE 'org.opdbus.directory%' LIMIT 1;

-- ===================================================================
-- DNS Zones (for AD-integrated DNS)
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, ldap_oid, parent_class, structural, attributes_schema)
SELECT id, 'DNSZone', '1.2.840.113556.1.5.130', NULL, TRUE,
       json('{
         "zone_name": {"type": "string", "mandatory": true},
         "zone_type": {"type": "string"},
         "dynamic_update": {"type": "bool"},
         "secure_update": {"type": "bool"},
         "replication_scope": {"type": "string"},
         "dns_records": {"type": "array"}
       }')
FROM service_interfaces WHERE interface_name LIKE 'org.opdbus.directory%' LIMIT 1;

-- ===================================================================
-- Trust Relationships
-- ===================================================================

INSERT OR IGNORE INTO object_classes (interface_id, class_name, ldap_oid, parent_class, structural, attributes_schema)
SELECT id, 'TrustRelationship', '1.2.840.113556.1.5.37', NULL, TRUE,
       json('{
         "trusted_domain": {"type": "string", "mandatory": true},
         "trust_direction": {"type": "string"},
         "trust_type": {"type": "string"},
         "trust_attributes": {"type": "int"},
         "trust_partner": {"type": "string"},
         "when_created": {"type": "string"}
       }')
FROM service_interfaces WHERE interface_name LIKE 'org.opdbus.directory%' LIMIT 1;
