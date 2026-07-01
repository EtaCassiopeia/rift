---
layout: default
title: Hot Reload
parent: Features
nav_order: 26
---

# Hot Reload

`POST /admin/reload` re-reads the startup config source and atomically replaces every running
imposter — useful for editing imposters in a file and applying them without restarting the process.

---

## Requirements & behavior

- Rift must have been started with a config source: `--configfile <file>` or `--datadir <dir>`.
  Without one, reload is a **no-op** that returns `200`.
- The new config is **validated before** the running imposters are torn down. If it fails to parse
  or has duplicate ports / unsupported protocols, the running imposters are left untouched and the
  call errors.
- A successful reload **resets transient state**: recorded requests, scenario state, response
  cyclers (`repeat`), and flow-state TTLs all start fresh.

```bash
rift --configfile ./imposters.json      # start with a config source

# ...edit imposters.json...

curl -X POST http://localhost:2525/admin/reload   # 200; new config now live
```

To reload from a directory of one-imposter-per-file configs, start with `--datadir ./mb-data`
instead; `POST /admin/reload` re-reads the directory.
