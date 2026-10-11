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
  engine/         # geometry, planes, join, XDATA, regen, IFC core
  ui/             # AEC modals/panels (aec_* prefix)
  message.rs
  spawn.rs
  properties.rs
  update.rs       # dispatch only
```

Draw model: `src/modules/draw/line.rs` — `tool()` + command in the same file,
`inventory::submit!` there.

## Do

- New preview/sync logic under `project/`, not a dump file.
- New ribbon tools: own file under the domain group (`walls/`, …).
- Tests in the feature file (`#[cfg(test)]`).
- State in `AecState` / engine types, not on `CadApp`.

## Don't

- Bloat `update.rs` or recreate a `commands.rs` dump.
- `include!` — always Rust `mod`.
- AEC fields or long handlers in Core (`src/app/**`, window without AEC prefix).
- Change Core shaders/hatch/GPU when an AEC workaround is enough.
- Add AEC variants on Core `ModalKind` / `ColorPickTarget`. Core may only
  carry `ModalKind::Aec(AecModalKind)` and `ColorPickTarget::Aec(AecColorPickTarget)`.
  New AEC windows and colour fields live under `ui/` (`modal_kind.rs`,
  `color_pick.rs`, `modal_views.rs`).
