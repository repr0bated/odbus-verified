# Subid → Element Projection Catalog

Deterministic projection of every blob-catalog field to its render role.
Role vocabulary is data-semantic only; concrete element resolution is projector-internal.

## Role vocabulary

| subid cat | type | role |
|---|---|---|
| exp | any | surface |
| obs | scalar | display-value / state-flag |
| obs | list-record | collection-view |
| obs | record | record-view |
| mut | boolean | binary-control |
| mut | string | text-control |
| mut | integer | numeric-control |
| mut | list-scalar | multi-choice |
| mut | list-record | editable-collection |
| sch | any | validation-carrier (attaches to controls) |
| src | any | hydration-source |
| evt | any | trigger-binding |
| prj | any | repeat-binding |

## Coverage

- plugins: 67
- fields: 402
- mapped: 269 (66%)
- GAP (no subid): 133
- conflicts (mut + read_only): 0

## Role distribution

| role | fields |
|---|---|
| GAP(no-subid) | 133 |
| display-value | 55 |
| surface | 49 |
| validation-carrier | 35 |
| text-control | 27 |
| hydration-source | 17 |
| collection-view | 17 |
| record-view | 16 |
| record-editor | 11 |
| state-flag | 8 |
| editable-collection | 7 |
| structured-control | 7 |
| value-list | 5 |
| numeric-control | 4 |
| trigger-binding | 4 |
| binary-control | 3 |
| repeat-binding | 3 |
| multi-choice | 1 |

## Gaps by plugin

| plugin | unmapped fields |
|---|---|
| factory | 13 |
| full_system | 11 |
| netmaker | 10 |
| mail_server | 9 |
| fail2ban | 7 |
| memory | 5 |
| openflow | 5 |
| cognitive_mcp | 4 |
| qdrant | 3 |
| antigravity | 2 |
| antigravity_chat | 2 |
| snowball | 2 |
| compact_mcp | 2 |
| config | 2 |
| cozo | 2 |
| cron | 2 |
| ctl_plane_chatbot | 2 |
| dnsresolver | 2 |
| embedding_model | 2 |
| gemma_brain | 2 |
| human_principal | 2 |
| incus | 2 |
| json_render | 2 |
| keyring | 2 |
| large_language_model | 2 |
| oci | 2 |
| openflow_obfuscation | 2 |
| ovsdb_bridge | 2 |
| packagekit | 2 |
| pcidecl | 2 |
| rovs_commands | 2 |
| shared_unix_socket | 2 |
| unix_socket | 2 |
| web_ui | 2 |
| wg_opdbus | 2 |
| wgcf | 2 |
| xray | 2 |
| zeroclaw | 2 |
| freedesktop | 1 |
| host_runtime | 1 |
| login1 | 1 |
| oscal_subid_registry | 1 |
| privacy_routes | 1 |
| rtnetlink | 1 |
| service | 1 |
| users | 1 |

## Conflicts


## Full field catalog

| plugin | field | type | ro | req | cat | role |
|---|---|---|---|---|---|---|
| adc | configured | boolean | False | True | exp | surface |
| agent_config | agents | list-record | False | False | exp | surface |
| antigravity | actor_id | string | False | False | — | **GAP** |
| antigravity | auth | record | False | False | mut | record-editor |
| antigravity | capability_id | string | False | False | — | **GAP** |
| antigravity | config_schema | record | False | False | sch | validation-carrier |
| antigravity | endpoints | record | False | False | mut | record-editor |
| antigravity | generation_config | record | False | False | sch | validation-carrier |
| antigravity | inspector_fields | record | False | False | sch | validation-carrier |
| antigravity | llm_plugin | string | False | False | mut | text-control |
| antigravity | model_routes | list-record | False | False | mut | editable-collection |
| antigravity | project | record | False | False | exp | surface |
| antigravity | provider_route | string | False | False | mut | text-control |
| antigravity | providers | list-record | False | False | mut | editable-collection |
| antigravity | router | record | False | False | mut | record-editor |
| antigravity | safety_settings | list-record | False | False | mut | editable-collection |
| antigravity | selected_model | string | False | False | mut | text-control |
| antigravity | status | string | False | False | obs | display-value |
| antigravity | structured_output | record | False | False | exp | surface |
| antigravity | tools | list-record | False | False | exp | surface |
| antigravity | ui_surfaces | list-record | False | False | exp | surface |
| antigravity | usage | record | False | False | obs | record-view |
| antigravity_chat | actor_id | string | False | False | — | **GAP** |
| antigravity_chat | auth | record | False | False | mut | record-editor |
| antigravity_chat | bridge | record | False | False | mut | record-editor |
| antigravity_chat | capability_id | string | False | False | — | **GAP** |
| antigravity_chat | config | record | False | False | sch | validation-carrier |
| antigravity_chat | llm_plugin | string | False | False | mut | text-control |
| antigravity_chat | provider_route | string | False | False | mut | text-control |
| antigravity_chat | selected_model | string | False | False | mut | text-control |
| antigravity_chat | status | string | False | False | obs | display-value |
| snowball | actor_id | string | False | False | — | **GAP** |
| snowball | base_path | string | False | True | exp | surface |
| snowball | capability_id | string | False | False | — | **GAP** |
| snowball | retention | record | False | True | obs | record-view |
| snowball | snapshot_count | integer | False | True | exp | surface |
| snowball | snapshot_interval | string | False | True | mut | text-control |
| snowball | snapshots | list-record | False | True | exp | surface |
| snowball | status | string | False | True | obs | display-value |
| btrfs | config | any | False | False | obs | display-value |
| btrfs | dr_status | any | False | False | obs | display-value |
| btrfs | inspector_fields | record | False | False | obs | record-view |
| btrfs | send_state | any | False | False | obs | display-value |
| btrfs | snapshots | any | False | False | obs | display-value |
| btrfs | status | string | False | False | obs | display-value |
| btrfs | subvolumes | any | False | False | obs | display-value |
| cognitive_mcp | actor_id | string | False | False | — | **GAP** |
| cognitive_mcp | auth_status | any | True | False | obs | display-value |
| cognitive_mcp | capability_id | string | False | False | — | **GAP** |
| cognitive_mcp | citation | record | True | False | exp | surface |
| cognitive_mcp | code_context | record | True | False | exp | surface |
| cognitive_mcp | code_index | record | True | False | src | hydration-source |
| cognitive_mcp | code_search | record | True | False | obs | record-view |
| cognitive_mcp | dbus_enabled | boolean | False | False | mut | binary-control |
| cognitive_mcp | gemini_query_request | record | True | False | exp | surface |
| cognitive_mcp | healthy | boolean | True | False | obs | state-flag |
| cognitive_mcp | memory_tool | record | True | False | exp | surface |
| cognitive_mcp | notebook_count | integer | True | False | obs | display-value |
| cognitive_mcp | queries_limit | integer | True | False | obs | display-value |
| cognitive_mcp | queries_remaining | integer | True | False | obs | display-value |
| cognitive_mcp | running | boolean | True | False | obs | state-flag |
| cognitive_mcp | source_info | record | True | False | exp | surface |
| cognitive_mcp | source_locator | string | False | False | — | **GAP** |
| cognitive_mcp | source_system | string | False | False | — | **GAP** |
| cognitive_mcp | wg_interface | string | False | False | mut | text-control |
| compact_mcp | actor_id | string | False | False | — | **GAP** |
| compact_mcp | capability_id | string | False | False | — | **GAP** |
| compact_mcp | http | string | False | False | mut | text-control |
| compact_mcp | log_level | any | False | False | mut | structured-control |
| compact_mcp | mode | any | False | False | mut | structured-control |
| compact_mcp | running | boolean | True | False | obs | state-flag |
| compact_mcp | stdio | boolean | False | False | mut | binary-control |
| compact_mcp | wg_interface | string | False | False | mut | text-control |
| compact_mcp | ws | string | False | False | mut | text-control |
| config | actor_id | string | False | False | — | **GAP** |
| config | capability_id | string | False | False | — | **GAP** |
| config | configs | any | False | True | mut | structured-control |
| config | inspector_fields | record | False | False | sch | validation-carrier |
| cozo | engine | string | False | True | src | hydration-source |
| cozo | hnsw_indices | list-record | False | True | obs | collection-view |
| cozo | indices | list-record | False | True | obs | collection-view |
| cozo | path | string | False | True | src | hydration-source |
| cozo | relations | list-record | False | True | obs | collection-view |
| cozo | running_queries | integer | False | True | obs | display-value |
| cozo | source_locator | string | False | False | — | **GAP** |
| cozo | source_system | string | False | False | — | **GAP** |
| cozo | triggers | list-record | False | True | obs | collection-view |
| cozo | version | string | False | True | obs | display-value |
| cron | actor_id | string | False | False | — | **GAP** |
| cron | capability_id | string | False | False | — | **GAP** |
| cron | config | record | False | False | sch | validation-carrier |
| cron | jobs | list-record | False | False | mut | editable-collection |
| cron | schedules | record | False | False | sch | validation-carrier |
| cron | status | string | False | False | obs | display-value |
| ctl_plane_chatbot | actor_id | string | False | False | — | **GAP** |
| ctl_plane_chatbot | capability_id | string | False | False | — | **GAP** |
| ctl_plane_chatbot | chat_model | string | False | False | mut | text-control |
| ctl_plane_chatbot | dedup_window_hrs | integer | False | False | mut | numeric-control |
| ctl_plane_chatbot | embedding_plugin | string | False | False | mut | text-control |
| ctl_plane_chatbot | embedding_queue_depth | integer | True | False | obs | display-value |
| ctl_plane_chatbot | input_type | any | False | False | mut | structured-control |
| ctl_plane_chatbot | last_vectorized_at | string | True | False | obs | display-value |
| ctl_plane_chatbot | llm_plugin | string | False | False | mut | text-control |
| ctl_plane_chatbot | nesting_policy | any | False | False | mut | structured-control |
| ctl_plane_chatbot | output_dtype | any | False | False | mut | structured-control |
| ctl_plane_chatbot | qdrant_collection | string | False | False | mut | text-control |
| ctl_plane_chatbot | queue_alert_threshold | integer | False | False | mut | numeric-control |
| ctl_plane_chatbot | reasoning_active | boolean | True | False | obs | state-flag |
| ctl_plane_chatbot | reasoning_episode | record | True | False | obs | record-view |
| ctl_plane_chatbot | running | boolean | True | False | obs | state-flag |
| ctl_plane_chatbot | significance | record | True | False | obs | record-view |
| ctl_plane_chatbot | vector_dims | any | False | False | mut | structured-control |
| ctl_plane_chatbot | vector_id | string | True | False | obs | display-value |
| ctl_plane_chatbot | vectorization_enabled | boolean | False | False | mut | binary-control |
| datastore | snowball_count | integer | False | False | obs | display-value |
| datastore | execution_count | integer | False | False | obs | display-value |
| datastore | namespaces | list-record | False | False | obs | collection-view |
| datastore | object_count | integer | False | False | obs | display-value |
| datastore | objects | list-record | False | False | obs | collection-view |
| datastore | status | string | False | False | obs | display-value |
| dnsresolver | items | list-scalar | False | True | — | **GAP** |
| dnsresolver | version | integer | False | False | — | **GAP** |
| embedding_model | actor_id | string | False | False | — | **GAP** |
| embedding_model | available_models | list-scalar | False | False | obs | value-list |
| embedding_model | capability_id | string | False | False | — | **GAP** |
| embedding_model | dimensions | integer | False | False | mut | numeric-control |
| embedding_model | endpoint | string | False | False | mut | text-control |
| embedding_model | model_digest | string | False | False | obs | display-value |
| embedding_model | model_id | string | False | False | mut | text-control |
| embedding_model | provider | string | False | False | mut | text-control |
| embedding_model | status | string | False | False | obs | display-value |
| endpoint | endpoints | list-scalar | False | False | exp | surface |
| factory | api_keys | record | False | False | — | **GAP** |
| factory | auth_method | string | False | False | — | **GAP** |
| factory | byom_sources | record | False | False | — | **GAP** |
| factory | computers | record | False | False | — | **GAP** |
| factory | config_schema | record | False | False | — | **GAP** |
| factory | endpoint | string | False | False | — | **GAP** |
| factory | models | record | False | False | — | **GAP** |
| factory | providers | list-record | False | False | — | **GAP** |
| factory | session_settings | record | False | False | — | **GAP** |
| factory | sessions | record | False | False | — | **GAP** |
| factory | status | string | False | False | — | **GAP** |
| factory | tools | list-record | False | False | — | **GAP** |
| factory | ui_surfaces | list-record | False | False | — | **GAP** |
| fail2ban | actions | any | False | True | — | **GAP** |
| fail2ban | bans | any | False | True | — | **GAP** |
| fail2ban | config | any | False | True | — | **GAP** |
| fail2ban | filters | any | False | True | — | **GAP** |
| fail2ban | jails | any | False | True | — | **GAP** |
| fail2ban | logs | any | False | True | — | **GAP** |
| fail2ban | status | string | False | True | — | **GAP** |
| freedesktop | inspector_fields | record | False | False | — | **GAP** |
| full_system | captured_at | string | False | True | — | **GAP** |
| full_system | containers | record | False | True | — | **GAP** |
| full_system | hostname | string | False | True | — | **GAP** |
| full_system | network | record | False | True | — | **GAP** |
| full_system | packages | list-record | False | True | — | **GAP** |
| full_system | plugins | record | False | True | — | **GAP** |
| full_system | services | list-record | False | True | — | **GAP** |
| full_system | storage | record | False | True | — | **GAP** |
| full_system | system | record | False | True | — | **GAP** |
| full_system | users | list-record | False | True | — | **GAP** |
| full_system | version | integer | False | True | — | **GAP** |
| gcloud_adc | account | string | False | False | exp | surface |
| gcloud_adc | authenticated | boolean | False | True | exp | surface |
| gcloud_adc | project_id | string | False | False | exp | surface |
| gemma_brain | actor_id | string | False | False | — | **GAP** |
| gemma_brain | capability_id | string | False | False | — | **GAP** |
| gemma_brain | gallery | record | False | False | sch | validation-carrier |
| gemma_brain | llm_plugin | string | False | False | mut | text-control |
| gemma_brain | perspectives | list-scalar | False | False | mut | multi-choice |
| gemma_brain | routing | record | False | False | mut | record-editor |
| gemma_brain | status | string | False | False | obs | display-value |
| ghostbridge | bridge_identity | record | False | False | obs | record-view |
| ghostbridge | endpoints | list-record | False | False | obs | collection-view |
| ghostbridge | ghostrunner | record | False | False | exp | surface |
| ghostbridge | status | string | False | False | obs | display-value |
| hardware | cpu | record | False | False | exp | surface |
| hardware | disks | list-record | False | False | exp | surface |
| hardware | memory | record | False | False | exp | surface |
| host_runtime | last_queried_at | string | False | False | — | **GAP** |
| human_principal | actor_id | string | False | False | — | **GAP** |
| human_principal | capability_id | string | False | False | — | **GAP** |
| human_principal | principals | list-record | False | True | obs | collection-view |
| identity_sled | sleds | list-record | False | True | obs | collection-view |
| incus | inspector_fields | record | False | False | — | **GAP** |
| incus | instances | list-record | False | True | — | **GAP** |
| json_render | actions | list-record | False | False | exp | surface |
| json_render | components | list-record | False | False | exp | surface |
| json_render | config | record | False | False | sch | validation-carrier |
| json_render | core_exports | list-record | False | False | obs | collection-view |
| json_render | directives | list-record | False | False | obs | collection-view |
| json_render | inspector_fields | record | False | False | sch | validation-carrier |
| json_render | methods | list-record | False | False | exp | surface |
| json_render | packages | list-record | False | False | obs | collection-view |
| json_render | renderers | list-record | False | False | exp | surface |
| json_render | source | record | False | False | src | hydration-source |
| json_render | source_locator | string | False | False | — | **GAP** |
| json_render | source_system | string | False | False | — | **GAP** |
| json_render | spec_contract | record | False | False | sch | validation-carrier |
| json_render | status | string | False | False | obs | display-value |
| json_render | validation_checks | list-record | False | False | obs | collection-view |
| keypair | keypairs | list-record | False | False | exp | surface |
| keyring | collections | list-record | False | True | — | **GAP** |
| keyring | default_collection | string | False | False | — | **GAP** |
| large_language_model | actor_id | string | False | False | — | **GAP** |
| large_language_model | available_models | list-scalar | False | False | obs | value-list |
| large_language_model | capability_id | string | False | False | — | **GAP** |
| large_language_model | endpoint | string | False | False | mut | text-control |
| large_language_model | inspector_fields | record | False | False | sch | validation-carrier |
| large_language_model | model_digest | string | False | False | obs | display-value |
| large_language_model | model_id | string | False | False | mut | text-control |
| large_language_model | params | record | False | False | mut | record-editor |
| large_language_model | provider | string | False | False | mut | text-control |
| large_language_model | status | string | False | False | obs | display-value |
| login1 | sessions | any | False | True | — | **GAP** |
| mail_server | container_ip | string | True | False | — | **GAP** |
| mail_server | container_name | string | False | True | — | **GAP** |
| mail_server | container_status | string | True | True | — | **GAP** |
| mail_server | dbus_service_name | string | False | True | — | **GAP** |
| mail_server | domain | string | False | True | — | **GAP** |
| mail_server | endpoints | record | False | True | — | **GAP** |
| mail_server | healthy | boolean | True | True | — | **GAP** |
| mail_server | last_error | string | True | False | — | **GAP** |
| mail_server | xray_socket_path | string | False | True | — | **GAP** |
| mcp | compact_mode | any | False | False | exp | surface |
| mcp | inspector_fields | record | False | False | sch | validation-carrier |
| mcp | servers | any | False | False | exp | surface |
| mcp | tool_groups | any | False | False | exp | surface |
| memory | backend | string | False | True | — | **GAP** |
| memory | config | any | False | True | — | **GAP** |
| memory | namespaces | any | False | True | — | **GAP** |
| memory | stats | any | False | True | — | **GAP** |
| memory | status | string | False | True | — | **GAP** |
| net | interfaces | list-record | False | False | exp | surface |
| netmaker | config | record | False | True | — | **GAP** |
| netmaker | control_socket | string | False | False | — | **GAP** |
| netmaker | daemon_running | boolean | False | True | — | **GAP** |
| netmaker | dependencies | list-scalar | False | True | — | **GAP** |
| netmaker | installed | boolean | False | True | — | **GAP** |
| netmaker | networks | list-record | False | True | — | **GAP** |
| netmaker | public_ip | string | False | False | — | **GAP** |
| netmaker | software | string | False | True | — | **GAP** |
| netmaker | tools | any | False | True | — | **GAP** |
| netmaker | version | string | False | True | — | **GAP** |
| notebooklm | auth | record | False | True | obs | record-view |
| notebooklm | config | record | False | True | obs | record-view |
| notebooklm | corpus | record | False | True | obs | record-view |
| notebooklm | master_notebook | record | False | True | obs | record-view |
| notebooklm | status | string | False | True | obs | display-value |
| oci | containers | list-record | False | True | — | **GAP** |
| oci | inspector_fields | record | False | False | — | **GAP** |
| openflow | auto_discover_containers | boolean | False | False | — | **GAP** |
| openflow | bridges | list-record | False | True | — | **GAP** |
| openflow | controller_endpoint | string | False | False | — | **GAP** |
| openflow | enable_security_flows | boolean | False | False | — | **GAP** |
| openflow | obfuscation_level | integer | False | False | — | **GAP** |
| openflow_obfuscation | actor_id | string | False | False | — | **GAP** |
| openflow_obfuscation | bridge_name | string | False | False | mut | text-control |
| openflow_obfuscation | capability_id | string | False | False | — | **GAP** |
| openflow_obfuscation | custom_flows | list-record | False | False | exp | surface |
| openflow_obfuscation | enable_security_flows | boolean | False | False | obs | state-flag |
| openflow_obfuscation | obfuscation_level | integer | False | False | mut | numeric-control |
| openflow_obfuscation | privacy_ports | list-scalar | False | False | exp | surface |
| oscal_subid_registry | actor_id | string | False | False | mut | text-control |
| oscal_subid_registry | authority_rank | integer | False | False | src | hydration-source |
| oscal_subid_registry | capability_id | string | False | False | mut | text-control |
| oscal_subid_registry | category | any | False | True | sch | validation-carrier |
| oscal_subid_registry | component_type | any | False | True | sch | validation-carrier |
| oscal_subid_registry | consumer_surface | any | False | False | exp | surface |
| oscal_subid_registry | control_refs | list-scalar | False | False | sch | validation-carrier |
| oscal_subid_registry | control_source | string | False | False | sch | validation-carrier |
| oscal_subid_registry | dbus_path | string | False | False | prj | repeat-binding |
| oscal_subid_registry | event_hash | string | True | False | evt | trigger-binding |
| oscal_subid_registry | event_id | string | True | False | evt | trigger-binding |
| oscal_subid_registry | facet | string | False | False | sch | validation-carrier |
| oscal_subid_registry | idempotency_key | string | False | False | mut | text-control |
| oscal_subid_registry | inspector_fields | record | False | False | — | **GAP** |
| oscal_subid_registry | proof_root | string | True | False | evt | trigger-binding |
| oscal_subid_registry | query_scope | string | False | False | obs | display-value |
| oscal_subid_registry | schema_hash | string | True | False | sch | validation-carrier |
| oscal_subid_registry | schema_id | string | False | False | sch | validation-carrier |
| oscal_subid_registry | service_name | string | False | False | prj | repeat-binding |
| oscal_subid_registry | source_locator | string | False | False | src | hydration-source |
| oscal_subid_registry | source_subid | string | False | False | prj | repeat-binding |
| oscal_subid_registry | source_system | string | False | False | src | hydration-source |
| oscal_subid_registry | statement_refs | list-scalar | False | False | sch | validation-carrier |
| oscal_subid_registry | subid | string | False | True | sch | validation-carrier |
| oscal_subid_registry | subject | string | False | True | sch | validation-carrier |
| oscal_subid_registry | tags_touched | list-scalar | True | False | evt | trigger-binding |
| oscal_subid_registry | tool_name | string | False | False | exp | surface |
| oscal_subid_registry | uuid | string | True | True | obs | display-value |
| oscal_subid_registry | verb | string | False | True | sch | validation-carrier |
| oscal_subid_registry | version | integer | False | False | sch | validation-carrier |
| ovsdb_bridge | bridges | list-record | False | True | — | **GAP** |
| ovsdb_bridge | inspector_fields | record | False | False | — | **GAP** |
| packagekit | packages | any | False | True | — | **GAP** |
| packagekit | version | integer | False | False | — | **GAP** |
| pcidecl | items | any | False | True | — | **GAP** |
| pcidecl | version | integer | False | False | — | **GAP** |
| persona | persona_count | integer | False | True | obs | display-value |
| persona | personas | list-record | False | False | exp | surface |
| persona | status | string | False | True | obs | display-value |
| privacy_routes | routes | list-record | False | True | — | **GAP** |
| procfs | cpuinfo | any | True | False | obs | display-value |
| procfs | diskstats | any | True | False | obs | display-value |
| procfs | kernel | any | True | False | obs | display-value |
| procfs | loadavg | record | True | False | obs | record-view |
| procfs | memory | record | True | False | obs | record-view |
| procfs | mounts | list-record | True | False | obs | collection-view |
| procfs | net_dev | record | True | False | obs | record-view |
| procfs | stat | any | True | False | obs | display-value |
| procfs | uptime | record | True | False | obs | record-view |
| procfs | vmstat | any | True | False | obs | display-value |
| proxy_server | enabled | boolean | False | False | obs | state-flag |
| proxy_server | port | integer | False | False | obs | display-value |
| qdrant | cluster_status | any | False | False | obs | display-value |
| qdrant | collections | list-record | False | True | obs | collection-view |
| qdrant | commit | string | False | True | obs | display-value |
| qdrant | grpc_endpoint | string | False | True | src | hydration-source |
| qdrant | http_endpoint | string | False | True | src | hydration-source |
| qdrant | inspector_fields | record | False | False | — | **GAP** |
| qdrant | source_locator | string | False | False | — | **GAP** |
| qdrant | source_system | string | False | False | — | **GAP** |
| qdrant | telemetry | any | False | False | obs | display-value |
| qdrant | title | string | False | True | obs | display-value |
| qdrant | version | string | False | True | obs | display-value |
| rovs_commands | available | boolean | True | True | — | **GAP** |
| rovs_commands | schema_version | string | True | False | — | **GAP** |
| rtnetlink | interfaces | list-record | False | True | — | **GAP** |
| schema_renderer | element_types | any | False | False | exp | surface |
| schema_renderer | field_mappings | any | False | False | exp | surface |
| schema_renderer | gallery_config | any | False | False | exp | surface |
| schema_renderer | inspector_fields | record | False | False | sch | validation-carrier |
| schema_renderer | layouts | any | False | False | exp | surface |
| schema_renderer | render_config | any | False | False | sch | validation-carrier |
| schema_renderer | status | string | False | False | obs | display-value |
| schema_renderer | sub_views | any | False | False | exp | surface |
| service | services | any | False | True | — | **GAP** |
| sess_decl | sessions | any | False | True | obs | display-value |
| shared_unix_socket | registrations | list-record | False | True | obs | collection-view |
| shared_unix_socket | shared_socket | string | False | False | src | hydration-source |
| shared_unix_socket | source_locator | string | False | False | — | **GAP** |
| shared_unix_socket | source_system | string | False | False | — | **GAP** |
| software | packages | list-record | False | True | obs | collection-view |
| unix_socket | actor_id | string | False | False | — | **GAP** |
| unix_socket | capability_id | string | False | False | — | **GAP** |
| unix_socket | sockets | list-record | False | True | mut | editable-collection |
| users | users | list-record | False | True | — | **GAP** |
| web_ui | actor_id | string | False | False | — | **GAP** |
| web_ui | capabilities | record | False | False | obs | record-view |
| web_ui | capability_id | string | False | False | — | **GAP** |
| web_ui | identity | record | False | False | sch | validation-carrier |
| web_ui | tunables | record | False | False | mut | record-editor |
| wg_opdbus | config_path | string | False | False | src | hydration-source |
| wg_opdbus | excluded_plugins | list-scalar | False | False | obs | value-list |
| wg_opdbus | identity_role | string | False | False | src | hydration-source |
| wg_opdbus | interface | record | False | False | src | hydration-source |
| wg_opdbus | netmaker_plugin | string | False | False | obs | display-value |
| wg_opdbus | route_targets | list-scalar | False | False | src | hydration-source |
| wg_opdbus | source_locator | string | False | False | — | **GAP** |
| wg_opdbus | source_system | string | False | False | — | **GAP** |
| wg_opdbus | wireguard_plugin | string | False | False | src | hydration-source |
| wgcf | config | record | False | False | sch | validation-carrier |
| wgcf | dependencies | list-scalar | False | False | obs | value-list |
| wgcf | oscal_source | string | False | False | src | hydration-source |
| wgcf | software | string | False | False | obs | display-value |
| wgcf | source_locator | string | False | False | — | **GAP** |
| wgcf | source_system | string | False | False | — | **GAP** |
| wgcf | tools | any | False | False | exp | surface |
| wgcf | version | string | False | False | obs | display-value |
| wireguard | inspector_fields | record | False | False | sch | validation-carrier |
| wireguard | interfaces | list-record | False | False | exp | surface |
| workflows | config | any | False | False | sch | validation-carrier |
| workflows | status | string | False | False | obs | display-value |
| workflows | workflows | any | False | False | exp | surface |
| xray | config | record | False | False | sch | validation-carrier |
| xray | dependencies | list-scalar | False | False | obs | value-list |
| xray | oscal_source | string | False | False | src | hydration-source |
| xray | running | boolean | False | False | obs | state-flag |
| xray | software | string | False | False | obs | display-value |
| xray | source_locator | string | False | False | — | **GAP** |
| xray | source_system | string | False | False | — | **GAP** |
| xray | tools | any | False | False | exp | surface |
| xray | version | string | False | False | obs | display-value |
| zeroclaw | actor_id | string | False | False | — | **GAP** |
| zeroclaw | capability_id | string | False | False | — | **GAP** |
| zeroclaw | config_schema | record | False | False | sch | validation-carrier |
| zeroclaw | configurable_options | record | False | False | sch | validation-carrier |
| zeroclaw | inspector_fields | record | False | False | sch | validation-carrier |
| zeroclaw | model_assignments | record | False | False | mut | record-editor |
| zeroclaw | model_routes | list-record | False | False | mut | editable-collection |
| zeroclaw | providers | list-record | False | False | mut | editable-collection |
| zeroclaw | router | record | False | False | mut | record-editor |
| zeroclaw | selected_model | string | False | False | exp | surface |
| zeroclaw | selected_provider | string | False | False | mut | text-control |
| zeroclaw | status | string | False | False | obs | display-value |
| zeroclaw | structured_output | record | False | False | exp | surface |
| zeroclaw | tools | list-record | False | False | exp | surface |
| zeroclaw | transport | record | False | False | mut | record-editor |
| zeroclaw | ui_surfaces | list-record | False | False | exp | surface |
