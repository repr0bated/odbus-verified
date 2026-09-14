---
name: plugin-schema
description: >-
  Drill into a sealed PluginSchema: fields, methods, and CallMethod dispatch for any plugin id.
author: 3tched
version: 0.1.0
category: control-plane
---

# Plugin Schema

Use when inspecting any sealed plugin. Open `/plugin/<id>`, read fields and methods from the sealed PluginSchema, then invoke methods through PluginService.CallMethod / MutationEngine. Skills drill down under zeroclaw — they are not a separate render surface.
