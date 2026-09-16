# AEC module

Project-wide rules: repo-root `AGENTS.md`. This file is layout and do/don’t only.

## Layout

```
src/modules/aec/
  walls/          # wall tools, one file per tool
  rooms/
  styles/
  project/        # explorer, storeys, preview/sync — separate files
  ifc/
  engine/         # geometry, planes, join, IFC core
  ui/             # AEC modals/panels (aec_* prefix)
  message.rs
  spawn.rs
  properties.rs
  update.rs       # dispatch only
```

Draw model: `src/modules/draw/line.rs` — `tool()` + command in the same file,
`inventory::submit!` there.

## Do

- New preview/sync logic under `project/`, not in `commands.rs`.
- New ribbon tools: own file under the domain group (`walls/`, …).
- Tests in the feature file (`#[cfg(test)]`).
- State in `AecState` / engine types, not on `CadApp`.

## Don't

- Bloat `commands.rs` / `update.rs`.
- `include!` — always Rust `mod`.
- AEC fields or long handlers in Core (`src/app/**`, window without AEC prefix).
- Change Core shaders/hatch/GPU when an AEC workaround is enough.
