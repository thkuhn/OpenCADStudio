# AEC Plugin — Reference

**Status:** Moved

The AEC (Architecture / basic BIM) add-on described in the project plan is
implemented as an external, standalone plugin repository,
[`opencad-aec-plugin`](https://github.com/HakanSeven12/opencad-aec-plugin),
in line with the project rule that Open CAD Studio
[ships no built-in plugins](plugin-architecture.md).

This repo (the host) only keeps a discovery entry for it in
[`plugins/registry.json`](../plugins/registry.json); no AEC code lives here.

For the full XDATA schema (APPID `OPENCAD_AEC`, Wall/Room/Storey record
layouts), the Layer A/B/C architecture mapping, and the `AEC_*` command
inventory, see `AEC-XDATA-SCHEMA.md` in the `opencad-aec-plugin` repository —
that is now the single source of truth for AEC-specific plugin authors.

See [`plugin-architecture.md`](plugin-architecture.md) for the general
add-on model (manifest, `CadModule`/`BuiltinPlugin`, IPC, engine crates) that
any add-on, including `opencad-aec-plugin`, builds on.
