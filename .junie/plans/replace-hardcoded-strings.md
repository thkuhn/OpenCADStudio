---
sessionId: session-260907-000115-1wqf
---

# Status (2026-09-07)

**Done.** All three delivery steps implemented and committed.

- Commit `8ecef9bc` on `feature/aec-core-module`: *AEC: localize user-facing strings and English comments*
- Remaining AEC UI / prompts / errors use Fluent group `aec` (`tr!`) in `locales/en-US` and `locales/de-DE`
- German code comments in AEC engine/UI/app converted to English
- Command tokens, style IDs, `locale_catalog.rs` `t!` keys, and app-wide i18n left unchanged
- Follow-up (out of scope): rest of the application is not localized by this plan

# Requirements

### Overview & Goals
Nur das **AEC-Modul** (Architektur/Wände/Styles/Pläne): benutzersichtbare, hartcodierte englische Strings durch das bestehende i18n-System ersetzen. Kein App-weites Sweep über `src/ui/` oder Kern-CAD-Befehle.

### Scope
**In Scope**
- `src/modules/aec/` (vor allem `commands.rs` und sichtbare Engine-Meldungen).
- AEC-Fenster: `src/ui/window/aec_junction_editor.rs`, `aec_material_manager.rs`, `aec_plan_manager.rs`, `aec_project_explorer.rs`, `aec_style_picker.rs`, `aec_ui_util.rs`, `aec_wall_style_manager.rs`.
- Anbindung über `tr!`, `t!`, `tf!`; Keys in `locales/*/opencadstudio.ftl`; `src/locale_catalog.rs` nur bei neuen `t!`-Source-Keys.

**Out of Scope**
- Rest der Anwendung (`src/app/`, allgemeine Dialoge, Command-Line-Kern).
- Neues i18n-Framework oder Änderung von `src/i18n.rs` / `i18n.toml`.
- AutoCAD-kompatible Befehlsnamen (`WALLADD`, Style-IDs), Dateipfade, Debug/tracing, Tests-Asserts.
- IFC/interne Geometrie-Strings ohne UI.

### Functional Requirements
- AEC-UI, Prompts und Anwenderfehler folgen der aktiven Sprache.
- Fehlende Locale fällt auf `en-US` zurück.
- Semantische IDs (`tr!("aec", "key")`) für neue Strings; `t!`/`tf!` wo der Source-String schon Katalog-Key ist.
- Viele Call-Sites nutzen bereits `t!`/`tr!` — Arbeit ist Rest-Hartstrings.

# Technical Design

### Current Implementation
- AEC sitzt in `src/modules/aec/` (`mod.rs`, `commands.rs`, `engine/*`).
- UI-Manager unter `src/ui/window/aec_*.rs`; Icons unter `assets/icons/aec/`.
- i18n wie bisher: Fluent in `locales/<lang>/opencadstudio.ftl`, `tr!` / `t!` / `tf!`, Katalog `src/locale_catalog.rs`.
- Kaum eigene Fluent-Gruppe `aec`; AEC-Texte oft als Source-String-Keys im Katalog.

### Key Decisions
- **Nur AEC-Pfade.** Keine Inventur von `src/app/` oder allgemeinen Fenstern.
- **Kein neuer Mechanismus.** Bestehende Makros und FTL weiterverwenden.
- **Neue AEC-UI-Strings:** `tr!("aec", "...")` (oder bestehende Gruppe, wenn der String schon dort liegt).
- **Nicht anfassen:** Command-Tokens, Style-/Material-Namen aus der Zeichnung, Logs, Engine-interne IDs.

### Proposed Changes
1. Hartcodierte User-Strings in `src/modules/aec/` und `src/ui/window/aec_*.rs` finden.
2. Keys in `opencadstudio.ftl` anlegen (EN + DE mindestens).
3. Call-Sites umstellen; Katalog nur für neue `t!`-Keys.
4. Muster: bestehende `t!`/`tr!` in denselben AEC-Dateien.

### File Structure
- Ändern: `src/modules/aec/**`, `src/ui/window/aec_*.rs`
- Ändern: `locales/en-US/opencadstudio.ftl` (+ andere Locales)
- Ändern bei `t!`-Keys: `src/locale_catalog.rs`
- Nicht ändern: `src/i18n.rs`, `i18n.toml`, restliches `src/`

# Delivery Steps

### ✓ Step 1: Inventur AEC-Hartstrings
Klar, welche sichtbaren Literale in AEC noch nicht über i18n laufen.

- Durchsicht `src/modules/aec/commands.rs` und `engine/` auf user-facing Literale ohne `t!`/`tr!`/`tf!`.
- Durchsicht der sieben `src/ui/window/aec_*.rs`.
- Abgrenzung: UI/Prompt/Fehler vs. Befehlsnamen, Style-IDs, Logs.

### ✓ Step 2: AEC-Fenster auf tr!/t! vervollständigen
Manager- und Explorer-Texte kommen aus Fluent.

- Rest-Literale in `aec_wall_style_manager.rs`, `aec_material_manager.rs`, `aec_plan_manager.rs`, `aec_project_explorer.rs`, `aec_style_picker.rs`, `aec_junction_editor.rs`, `aec_ui_util.rs` wrappen.
- Keys in FTL (EN, DE) ergänzen; Katalog nur bei neuen `t!`-Keys.

### ✓ Step 3: AEC-Kommandos und Fehlermeldungen
Prompts und Anwenderfehler der AEC-Befehle folgen der UI-Sprache.

- User-facing Strings in `src/modules/aec/commands.rs` (und sichtbare Engine-Meldungen) auf `t!`/`tr!`/`tf!`.
- Command-Tokens und interne Diagnostics unverändert.