---
sessionId: session-260812-011701-iuuy
---

# Requirements

### Overview & Goals
In Projekten mit vielen Wandstilen sind die aktuell verwendeten `pick_list`-Dropdowns (Parent-Stil im Style-Manager, Material je Schicht, Layer-Override) sowie das Kommandozeilen-Prompt von `AEC_WALL` unhandlich, da `pick_list` keine eingebaute Filterung/Suche bietet. Ziel ist, alle Stil-/Material-Auswahlfelder auf das bereits im Projekt genutzte, echte Autocomplete-Widget `iced::widget::combo_box` umzustellen (bereits produktiv in `src/ui/window/layers.rs` für Linetype/Lineweight-Auswahl verwendet) — statt eines separaten Auswahl-Modals mit eigener Suchlogik, da `combo_box` genau dieses Verhalten bereits nativ und ohne neue Infrastruktur bietet.

Zusätzlich wird der aus einer vorherigen Session bekannte, offene Bug behoben: das Properties-Panel zeigt für `WALL_V2`-Wände aktuell die interne Stil-**ID** (Slug) statt des lesbaren Namens an und ist read-only — es gibt keine Möglichkeit, einer bestehenden Wand nachträglich über die Properties-Palette einen anderen Stil zuzuweisen. Dieses Feld wird im selben Zug auf einen editierbaren, durchsuchbaren `combo_box` umgestellt.

### Scope
**In Scope:**
- `combo_box`-basierte, durchsuchbare Stil-/Material-Auswahl an allen bestehenden Stellen:
  1. Parent-Stil-Auswahl ("Based on") im Wandstil-Formular des AEC Style Managers.
  2. Material-Auswahl je Schicht-Zeile im Wandstil-Formular.
  3. Layer-Override-Auswahl (Ziel-Zeichnungsebene) je Schicht-Zeile.
  4. Wandstil-Auswahl im Kommandozeilen-Workflow von `AEC_WALL` (`WallPhase::AskStyle`) — Autocomplete-Vorschläge nach Tippbeginn statt nur Exact-Match/erste Option bei leerer Eingabe.
  5. Properties-Panel: neues, editierbares `combo_box`-Feld für den Wandstil einer selektierten `WALL_V2`-Wand (löst den bekannten Bug, dass dort nur die rohe Stil-ID read-only angezeigt wird).
- Auswahl eines neuen Stils im Properties-Panel schreibt den neuen `style_id` inkl. neu aufgelöster `effective_layers()` in die `WALL_V2`-XDATA und ruft `regenerate_wall_representation` auf.

**Out of Scope:**
- Ein komplett neues, separates Auswahl-Modal mit eigener Suchlogik (bewusst verworfen zugunsten der Wiederverwendung von `combo_box`, siehe Key Decision).
- Änderungen an der Stil-Vererbungslogik selbst (`resolve_chain`/`effective_layers`) — nur die Auswahl-UI wird geändert.
- Persistente Geschoss-Verwaltung, Fenster/Türen und andere weiterhin zurückgestellte AEC-Themen.

### User Stories
- Als Planer möchte ich im Style-Manager beim Zuweisen eines Eltern-Stils tippen können, um aus vielen Stilen schnell den richtigen zu finden, statt eine lange Dropdown-Liste durchzuscrollen.
- Als Planer möchte ich beim Definieren einer Wandstil-Schicht das gewünschte Material per Suchfeld statt per langer Liste auswählen.
- Als Nutzer möchte ich beim Zeichnen einer Wand (`AEC_WALL`) den gewünschten Stil durch Tippen der ersten Buchstaben schnell finden.
- Als Nutzer möchte ich einer bereits gezeichneten Wand über die Properties-Palette nachträglich einen anderen Stil zuweisen können, mit Sucheingabe statt einer reinen Textanzeige.

### Functional Requirements
- Alle vier genannten Auswahlpunkte (Parent-Stil, Material je Schicht, Layer-Override, Properties-Panel-Stil) nutzen `iced::widget::combo_box` mit Tipp-Filterung über den sichtbaren Namen.
- `AEC_WALL`s `AskStyle`-Phase bietet weiterhin die bestehende Kommandozeilen-Eingabe (Kompatibilität mit reinem Tastatur-Workflow), ergänzt um eine Fuzzy-Teilstring-Suche statt nur Exact-Match, damit Tippen eines Teilnamens die Liste sinnvoll eingrenzt.
- Properties-Panel-Stil-Feld: Auswahl eines neuen Stils schreibt sofort `style_id` + aufgelöste Schichten in die `WALL_V2`-XDATA und stößt `regenerate_wall_representation` an, analog zum bereits etablierten Muster bei anderen Properties-Panel-Änderungen.
- Rückwärtskompatibilität: bestehende `WALL_V2`-Wände mit einer `style_id`, die in der aktuell geladenen Bibliothek nicht mehr existiert, zeigen weiterhin den (jetzt aufgelösten, falls möglich, sonst rohen) Namen an, ohne dass die App abstürzt.

### Non-Functional Requirements
- Keine neue UI-Infrastruktur/kein neues Widget wird erfunden — ausschließlich Wiederverwendung des bereits im Projekt etablierten `combo_box`-Patterns aus `src/ui/window/layers.rs`.
- Bestehende Tests (`cargo test --lib aec`, `polyline`, `properties`) bleiben grün; neue Tests für die Properties-Panel-Schreiblogik werden ergänzt.

# Technical Design

### Current Implementation
- `src/ui/window/aec_style_manager.rs`: Parent-Stil ("Based on", Zeile ~655), Material je Schicht (Zeile ~291) und Layer-Override (Zeile ~333) nutzen aktuell `iced::widget::pick_list` — ein reines Klick-Dropdown ohne Texteingabe/Filterung, das bei vielen Einträgen unhandlich wird.
- `src/ui/window/layers.rs`: bereits produktiv genutztes Referenzmuster für `combo_box`: `pub linetype_combo: combo_box::State<LinetypeItem>` als State-Feld, `combo_box(state, placeholder, current_value, on_selected)` als Widget-Aufruf, `sync_linetypes(...)` zum Nachziehen der Optionsliste bei Änderungen — dieses exakte Muster wird für Stile/Materialien übernommen.
- `src/modules/aec/commands.rs`: `WallPhase::AskStyle` (Zeile ~990-1097) ist ein reiner Kommandozeilen-Textprompt; bei leerer Eingabe wird `lib.wall_styles.first()` gewählt, ansonsten Exact-Match auf Name/ID (`eq_ignore_ascii_case`) — keine Teilstring-/Fuzzy-Suche.
- `src/app/properties.rs` (Zeile ~418-422): Wand-Stil-Feld ist aktuell `crate::entities::common::ro_prop(t!("Style"), "wall_style", wall_v2.style_id.clone())` — reine read-only Textanzeige der internen Stil-ID, kein Lookup in der `StyleLibrary`, keine Bearbeitungsmöglichkeit; es existiert noch kein Commit-Handler in `src/app/update/command.rs` für `WALL_V2`-Wand-Properties-Änderungen (nur die alte Single-Layer-`WALL`-Variante unterstützt Properties-Panel-Edits inkl. Aufruf von `regenerate_wall_representation`).
- `StyleLibrary`/`Style`/`resolve_chain` (`src/modules/aec/engine/style.rs`, `library.rs`) sind bereits vollständig vorhanden und werden nur gelesen, nicht verändert.

### Key Decisions
- **`combo_box` statt separatem Auswahl-Modal** (Antwort auf die Nutzerfrage): Das Projekt hat mit `iced::widget::combo_box` bereits ein produktiv genutztes, natives Autocomplete-Widget (siehe `layers.rs`). Ein zusätzliches, eigenständiges Auswahl-Modal mit eigener Suchlogik wäre reine Doppelarbeit und ein neues UI-Pattern, das dem Projektgrundsatz "bestehende Widgets wiederverwenden statt neu erfinden" widerspricht — `combo_box` liefert dieselbe UX (Tippen filtert, Klick wählt) ohne zusätzliche Infrastruktur.
- **`AskStyle`-Kommandozeilen-Workflow bleibt textbasiert, wird aber um Teilstring-Matching ergänzt**: Der Kommandozeilen-Workflow von `AEC_WALL` bleibt bewusst tastaturbasiert (kein Popup mitten im Zeichenfluss), erhält aber eine Teilstring-Suche (`to_lowercase().contains(...)`) statt nur Exact-Match, um bei vielen Stilen zumindest teilweises Tippen zu erlauben — ein `combo_box`-Popup während eines aktiven Zeichenkommandos würde den bestehenden Klick-Workflow stören und ist nicht vorgesehen.
- **Properties-Panel-Stil-Fix wird im selben Zug erledigt**: Da die Properties-Panel-Anzeige ohnehin auf ein neues Widget umgestellt wird, wird der bereits identifizierte Bug (rohe ID statt Name, read-only) direkt mitbehoben, statt ihn als separate Nacharbeit zu verschieben.

### Proposed Changes
1. **`AecStyleManagerCombos`-State** (`src/app/mod.rs`): neue `combo_box::State<StyleChoiceItem>`/`combo_box::State<MaterialChoiceItem>`-Felder (bzw. Wiederverwendung eines generischen `NamedChoiceItem { id: String, name: String }`-Wrapper-Typs) für Parent-Stil-, Material- und Layer-Override-Auswahl im `WallStyleFormState`; Synchronisierung analog zu `layers.rs::sync_linetypes` bei jedem Laden/Ändern der Bibliothek.
2. **`aec_style_manager.rs`**: Ersetzen der drei `pick_list`-Aufrufe (Parent-Stil, Material je Schicht, Layer-Override) durch `combo_box(...)`-Aufrufe nach dem `layers.rs`-Muster; bestehende `Message`-Handler (`AecStyleManagerWallStyleParentChanged`/`LayerMaterialChanged`/`LayerOverrideChanged`) bleiben inhaltlich unverändert, nur der Auslöser wechselt von `on_select` (pick_list) auf `on_selected` (combo_box).
3. **`commands.rs::WallPhase::AskStyle`**: Erweiterung der Text-Matching-Logik um einen Teilstring-Fallback (`iter().find(|s| s.style.name.to_lowercase().contains(&query))`), falls kein Exact-Match gefunden wird; bei mehreren Teilstring-Treffern wird wie bisher der erste Treffer gewählt und die Auswahl über die Kommandozeile bestätigt (kein neues Verzweigungsverhalten nötig, da das bestehende `CmdOption`-Vorschlagssystem bereits alle Namen aussortiert anzeigt).
4. **`src/app/properties.rs`**: Ersetzen von `ro_prop(...)` für das Wand-Stil-Feld durch ein neues editierbares Property (z. B. `combo_prop`/analoges neues Hilfsmuster, falls `entities::common` noch keinen combo-fähigen Property-Typ hat — dann minimaler neuer Property-Varianten-Typ, der intern denselben `combo_box` nutzt); Anzeigename wird per Lookup in der aktuell geladenen `StyleLibrary` aufgelöst (`lib.wall_styles.iter().find(|s| s.style.id == wall_v2.style_id)`), Fallback auf die rohe ID, falls der Stil nicht gefunden wird (z. B. Bibliothek nicht geladen oder Stil gelöscht).
5. **`src/app/update/command.rs`**: neuer Commit-Handler für Änderungen am Wand-Stil-Property-Feld: liest den gewählten `style_id`, löst `effective_layers()` über die aktuell geladene `StyleLibrary` auf, schreibt Stil-ID + Schicht-Snapshot in die `WALL_V2`-XDATA (wiederverwendet die bestehende `wall_v2_record`-Schreiblogik) und ruft anschließend `crate::modules::aec::commands::regenerate_wall_representation` auf, analog zum bestehenden Muster für Höhe/Dicke/Material-Änderungen bei Alt-Wänden.

### Data Models / Contracts
```rust
// Reused/shared choice-item wrapper, mirrors layers.rs::LinetypeItem's role
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedChoiceItem { pub id: String, pub name: String }
impl std::fmt::Display for NamedChoiceItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.name) }
}

// properties.rs: new combo-based property replacing ro_prop for wall style
// combo_prop(label, key, current: NamedChoiceItem, options: &combo_box::State<NamedChoiceItem>) -> PropertyRow
```

### Components
- `src/ui/window/aec_style_manager.rs` (geändert): 3 `pick_list`→`combo_box`-Umstellungen.
- `src/app/mod.rs` (geändert): neue `combo_box::State`-Felder für Stil-/Material-Auswahl.
- `src/modules/aec/commands.rs` (geändert): Teilstring-Matching in `WallPhase::AskStyle`.
- `src/app/properties.rs` (geändert): editierbares, namensauflösendes Wand-Stil-Feld statt `ro_prop`.
- `src/app/update/command.rs` (geändert): neuer Commit-Handler für Wand-Stil-Änderung inkl. `regenerate_wall_representation`-Aufruf.

### Risks
- **Zusätzlicher State-Sync-Aufwand**: `combo_box::State` muss bei jedem Laden/Ändern der Bibliothek (z. B. nach `AEC_STYLEMANAGER`-Öffnen, nach Speichern eines neuen Materials/Stils) neu synchronisiert werden, sonst zeigt die Combo veraltete Optionen — Mitigation: zentrale Sync-Funktion analog zu `layers.rs::sync_linetypes`, an allen Stellen aufgerufen, die die Bibliothek neu laden/ändern.
- **Properties-Panel-Migration**: der neue Commit-Handler für `WALL_V2`-Stiländerungen muss sauber neben dem bestehenden Höhe-/Dicke-Handler für Alt-Wände (`WALL`) koexistieren, ohne dessen Verhalten zu beeinflussen.
- **Gelöschte/fehlende Stile**: Wände, deren referenzierter Stil nicht mehr in der Bibliothek existiert, müssen im Properties-Panel und in der `AskStyle`-Suche robust behandelt werden (Fallback-Anzeige der rohen ID statt Absturz).

# Delivery Steps

### ✓ Step 1: Gemeinsamen NamedChoiceItem-Typ und combo_box-States für Style-Manager-Auswahlfelder einführen
**Abweichung vom Plan (bereits anderweitig gelöst):** Statt `combo_box`-States wird die Auswahl über ein eigenständiges, durchsuchbares Picker-Modal (`src/ui/window/aec_style_picker.rs`, geöffnet über `Message::AecStylePickerOpen(StylePickerTarget)`) realisiert. Es bietet ein `text_input`-Suchfeld (`aec_style_picker_filter`) plus gefilterte, scrollbare Liste inkl. Herkunfts-Badge — funktional identisch zur geplanten Autocomplete-Anforderung, ohne einen separaten `NamedChoiceItem`/`combo_box::State`-Unterbau zu benötigen. Kein Implementierungsbedarf.

### ✓ Step 2: pick_list durch combo_box im AEC-Style-Manager-Formular ersetzen
**Bereits erledigt (anderer Ansatz):** In `src/ui/window/aec_wall_style_manager.rs` sind Parent-Stil-, Material-je-Schicht- und Layer-Override-Auswahl keine `pick_list`s mehr, sondern Buttons, die das durchsuchbare `AecStylePickerOpen`-Modal öffnen (siehe `StylePickerTarget::WallStyleParent`/`LayerMaterial`/`LayerOverride`). Tippen im Modal-Suchfeld filtert die Liste; Auswahl bestätigt über `AecStylePickerConfirm` und aktualisiert den bestehenden State (`AecStyleManagerWallStyleParentChanged`/`LayerMaterialChanged`/`LayerOverrideChanged`) unverändert. Kein Implementierungsbedarf.

### ✓ Step 3: Teilstring-Suche für die AEC_WALL-Kommandozeilen-Stilauswahl ergänzen
**Gegenstandslos:** `WallPhase::AskStyle` als separater Kommandozeilen-Textprompt existiert nicht mehr; der Wandstil wird beim Zeichnen (`AEC_WALL`) über eine live editierbare Eigenschaft (`apply_live_property("wall_style", ...)`) gesetzt, die ebenfalls über `StylePickerTarget::ActiveCommand` dasselbe durchsuchbare Picker-Modal öffnet. Ein separates Teilstring-Matching im Kommandozeilen-Pfad ist damit nicht mehr anwendbar. Kein Implementierungsbedarf.

### ✓ Step 4: Properties-Panel-Wandstil-Feld auf editierbaren, namensauflösenden combo_box umstellen
**Bereits erledigt (anderer Ansatz):** In `src/app/properties.rs` ist das Wand-Stil-Feld kein `ro_prop` mehr, sondern ein `PropValue::Picker` mit per Lookup aufgelöstem Stilnamen (Fallback auf die rohe `style_id`, falls kein Treffer in der `StyleLibrary`). Auswahl öffnet über `AecStylePickerOpenForWallProperties` dasselbe durchsuchbare Modal. Kein Implementierungsbedarf.

### ✓ Step 5: Stiländerung im Properties-Panel schreibt WALL_V2-XDATA und regeneriert die Wanddarstellung
**Bereits erledigt:** Der `Message::AecStylePickerConfirm`-Handler (Fall `StylePickerTarget::WallPropertiesStyle`, `src/app/update/mod.rs`) löst `resolve_wall_style_layers_ids` auf, schreibt `style_id` + Layer-Snapshot über `wall_record` in die `WALL_V2`-XDATA und ruft anschließend `regenerate_wall_respecting_active_display_config` auf (respektiert zusätzlich die aktive `DisplayConfig`, siehe Folgeplan `aec-plan-view-display-variants.md`). Kein Implementierungsbedarf.