# Agent guidelines — OpenCADStudio

Always applies to agents in this repository (Junie, Cursor, Codex, …).
Keep it short. Plans under `.junie/plans/` are history, not source of truth.

## AEC ≠ Core (merge with `main`)

AEC domain logic belongs **only** in `src/modules/aec/**`.

Do not grow AEC logic in:

- `src/app/**`
- `src/command.rs`
- `src/ui/window/*` without an AEC prefix
- `src/scene/**`
- `src/shaders/**`
- `src/entities/**`

Core may have at most **thin, stable hooks**:

- `Message::Aec`
- `aec::update` (dispatch)
- `spawn_command`
- Properties merge (`properties::extend`)

No new AEC fields on `CadApp` / `OpenCADStudio`. State lives in `AecState`.

Shaders, hatch, GPU: touch Core only when unavoidable. Prefer AEC workarounds.

## File layout like Draw

Model: `src/modules/draw/` — group → folder, tool/feature → **one file**
(`tool()` + `CadCommand`, `inventory::submit!` in the tool file).

- New AEC feature: its own file; do not bloat `commands.rs` / `update.rs`.
- `update.rs` is dispatch only.
- Rust `mod`, **no** `include!`.

AEC layout details: `src/modules/aec/AGENTS.md`.

## Tests and i18n

- Tests next to the feature (`#[cfg(test)]` in the AEC file), not in Core app update.
- UI strings: keys in `locales/`; do not hard-code them in Core app code.

## Other rules

- Do not loosen Core’s public API for AEC (visibility, types).
- Reuse existing Core patterns (commands, ribbon, properties) via hooks; do not duplicate them.
- No exploit/malware artifacts; local fixes only in the codebase sense.
