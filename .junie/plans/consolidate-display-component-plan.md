---
sessionId: session-260826-002905-b8vb
---

# Requirements

### Overview & Goals
Der AEC-Plan `.junie/plans/aec-plan-view-display-variants.md` hat inzwischen (Steps 1-8 + Layer-Filter-UI-Follow-up) ein vollständig funktionierendes Datenmodell für `DisplayConfig`/`ComponentRuleSet` samt Manager-UI (`src/ui/window/aec_plan_manager.rs`). Ein Baustein aus Key Decision 8 fehlt in der UI noch: der **Schnellweg-Override `style_substitutions: HashMap<WallStyleRef, WallStyleRef>`** auf `DisplayConfig` (Original-Wandstil → Ersatz-Wandstil) ist im Datenmodell und in `commands.rs` (`regenerate_wall_representation_with_rules_and_substitutions` und Varianten) sowie in `validate_style_substitution` (`display_component.rs`, prüft gleiche Gesamtdicke) bereits vollständig implementiert und getestet — aber es gibt **keine UI**, um Substitutionszeilen anzulegen/zu entfernen. Bestehende Configs behalten ihre `style_substitutions` beim Speichern nur unverändert bei (`AecPlanManagerApply` in `src/app/update/mod.rs` rührt das Feld nicht an).

Dieses Planungsdokument beschreibt und stimmt das Design für die **Style-Substitutions-UI** ab (Erweiterung des `DisplayConfig`-Managers), bevor sie implementiert wird — analog zum bereits durchlaufenen Prozess für die Layer-Filter-UI.

### Scope
#### In Scope
- Neue Sektion "Wandstil-Substitutionen" im `DisplayConfig`-Manager (`src/ui/window/aec_plan_manager.rs`), unterhalb der bereits vorhandenen Layer-Filter-Sektion.
- Liste bestehender Substitutionszeilen (Original-Wandstil → Ersatz-Wandstil) mit Entfernen-Aktion pro Zeile.
- Formular zum Hinzufügen einer neuen Zeile: zwei `pick_list`s (Original-/Ersatz-Wandstil), gespeist aus der bereits im Manager geladenen `StyleLibrary` (`self.aec_style_library`, siehe Layer-Filter-UI-Vorarbeit).
- Validierung beim Hinzufügen über die bestehende `validate_style_substitution(&WallStyle, &WallStyle)` (Gesamtdicken-Konsistenzprüfung) — bei Fehler wird die Zeile **nicht** übernommen, sondern ein Inline-Fehlertext angezeigt (siehe Key Decision unten).
- Verhalten bei bereits vorhandener Quelle: Auswahl derselben Original-Wandstil-Quelle **überschreibt** den bisherigen Ziel-Eintrag (Upsert-Semantik, passend zur `HashMap`-Struktur), ohne Fehlermeldung.
- Anbindung an `AecPlanManagerApply`: das Feld `style_substitutions` wird ab jetzt aktiv aus dem neuen Edit-Buffer geschrieben (bisher nur unverändert durchgereicht), exakt nach dem bei der Layer-Filter-UI etablierten Muster (`has_buffer_overrides`, `Select`/`New`/`Duplicate` befüllen den Buffer aus der bestehenden Config).

#### Out of Scope (bewusst nicht Teil dieser Runde)
- Mapping-Table-Editor-UI für `scale_display_config_mappings` (Step 7) — weiterhin nur per JSON-Library-Datei editierbar, separater, bereits dokumentierter Follow-up.
- Änderungen an `validate_style_substitution` selbst oder an der Vorrangkette in `commands.rs` (`style_override`/`layer_style_override` > `style_substitutions` > `hatch_override` > Material-Standard) — beide sind bereits implementiert/getestet und werden nicht verändert.
- Eine UI zum Bearbeiten/Anlegen von Wandstilen selbst (das leistet bereits `aec_wall_style_manager.rs`); hier werden nur bestehende Wandstile referenziert.

### Functional Requirements
- Der Nutzer kann im `DisplayConfig`-Manager für die aktuell bearbeitete Konfiguration beliebig viele Original→Ersatz-Wandstil-Zeilen anlegen, ansehen und entfernen, ohne die JSON-Library-Datei von Hand zu bearbeiten.
- Eine Zeile mit inkonsistenter Gesamtdicke (Original ≠ Ersatz) wird beim Hinzufügen abgelehnt; der Nutzer sieht sofort einen Fehlertext mit der Ursache (z. B. "Gesamtdicke weicht ab: 0.24 m vs. 0.30 m"), die Zeile erscheint nicht in der Liste.
- Wählt der Nutzer für eine neue Zeile eine Original-Wandstil-Quelle, die bereits eine Substitution hat, wird der bisherige Ziel-Wandstil in der Anzeige durch den neu gewählten ersetzt (kein Duplikat, kein Fehler).
- Beim Speichern ("Übernehmen") wird `DisplayConfig.style_substitutions` exakt dem Inhalt der UI-Liste entsprechen — nicht mehr unverändert aus der alten Version übernommen.
- Eine Konfiguration ohne jede Substitutionszeile verhält sich wie heute (leere `HashMap`) — keine Regression für bereits gespeicherte Configs, die dieses Feld nie genutzt haben.

# Technical Design

### Current Implementation (bereits vorhanden, wird wiederverwendet)
- `src/modules/aec/engine/plan_view.rs`: `DisplayConfig.style_substitutions: HashMap<WallStyleRef, WallStyleRef>` (`WallStyleRef = String`, entspricht `Style::id`).
- `src/modules/aec/engine/display_component.rs::validate_style_substitution(source: &WallStyle, target: &WallStyle) -> Result<(), String>` — vergleicht `base_width_from_layers` beider Stile, liefert bei Abweichung einen Fehlertext, der bereits das Wort "inconsistent" enthält (siehe bestehender Test `validate_style_substitution_rejects_mismatched_total_thickness`).
- `src/modules/aec/commands.rs`: `regenerate_wall_representation_with_rules_and_substitutions` und die `_corner_`/`_precomputed_miters_`-Varianten werten `style_substitutions` bereits zur Laufzeit aus (Vorrangkette bereits implementiert, siehe Step-3-Bericht).
- `src/ui/window/aec_plan_manager.rs`: `config_form_view` rendert bereits Slot-Tabelle (Step 8) und Layer-Filter-Sektion (`layer_filter_section_view`, letzter Follow-up) — die neue Substitutions-Sektion wird nach demselben Muster als weitere `fn ..._section_view(...) -> Element<'a, Message>` ergänzt, die direkt unterhalb von `layer_filter_section` in `config_form_view` eingehängt wird.
- `src/app/mod.rs`/`src/app/update/mod.rs`: `AecPlanManagerOpen` lädt bereits `self.aec_style_library` (via `project::resolve_style_library`, ergänzt für die Layer-Filter-UI) — dieselbe Quelle liefert die Wandstil-Namen für die beiden neuen `pick_list`s.

### Key Decisions
1. **Validierung blockiert das Hinzufügen (nicht nur eine Warnung).** Schlägt `validate_style_substitution` fehl, wird die Zeile nicht in den Edit-Buffer übernommen; ein Inline-Fehlertext (aus dem `Err(String)` von `validate_style_substitution`) wird direkt im Formular angezeigt, bis der Nutzer eine andere Kombination wählt oder den Fehler behebt. *(Vom Nutzer bestätigt.)*
2. **Wiederverwendung einer bereits substituierten Quelle überschreibt den bisherigen Ziel-Wandstil (Upsert), statt die Auswahl zu verhindern oder einen Fehler zu zeigen.** Das passt zur zugrunde liegenden `HashMap<WallStyleRef, WallStyleRef>`-Struktur, die je Quelle ohnehin nur einen Wert erlaubt, und vermeidet eine künstliche Einschränkung der Auswahlliste. *(Vom Nutzer bestätigt.)*
3. **Edit-Buffer als `Vec<(WallStyleRef, WallStyleRef)>` statt direkt als `HashMap`**, damit die UI-Liste eine stabile Anzeige-Reihenfolge hat (analog zu `layer_filter_selection: Vec<LayerRef>` beim Layer-Filter-Follow-up) — beim Upsert (Decision 2) wird ein bestehender Eintrag mit gleicher Quelle per `position()`+Ersetzen aktualisiert statt dupliziert, beim Speichern wird der Buffer in eine `HashMap` konvertiert.
4. **Zwei neue, unabhängige Auswahlfelder** ("Neue Substitution: Original [pick_list] → Ersatz [pick_list] [Hinzufügen]") statt eines kombinierten Widgets — konsistent mit dem bereits etablierten `pick_list`-Muster für Phase/Ansichtstyp im selben Formular.
5. **Diese Runde liefert nur Planung/Diskussion, keine Implementierung** — die Umsetzung folgt als separate Anfrage nach Abstimmung dieses Plans. *(Vom Nutzer bestätigt.)*

### Proposed Changes
- `src/modules/aec/engine/display_component.rs`: neue reine Hilfsfunktion `pub fn upsert_style_substitution(existing: &[(WallStyleRef, WallStyleRef)], source: WallStyleRef, target: WallStyleRef) -> Vec<(WallStyleRef, WallStyleRef)>` (Upsert-Semantik aus Decision 2/3, unabhängig testbar, mirrort `layer_filter_from_selection`/`layer_filter_to_ui_state`-Stil aus dem letzten Follow-up).
- `src/app/mod.rs`: neue State-Felder `aec_plan_manager_style_substitutions: Vec<(WallStyleRef, WallStyleRef)>`, `aec_plan_manager_new_substitution_source: Option<WallStyleRef>`, `aec_plan_manager_new_substitution_target: Option<WallStyleRef>`, `aec_plan_manager_substitution_error: Option<String>`; neue `Message`-Varianten `AecPlanManagerSubstitutionSourceChanged(WallStyleRef)`, `AecPlanManagerSubstitutionTargetChanged(WallStyleRef)`, `AecPlanManagerSubstitutionAdd`, `AecPlanManagerSubstitutionRemove(WallStyleRef)`.
- `src/app/update/mod.rs`: `AecPlanManagerSelect`/`New`/`Duplicate` befüllen `aec_plan_manager_style_substitutions` aus `cfg.style_substitutions` (in `Vec` konvertiert); `AecPlanManagerSubstitutionAdd` löst beide Wandstile aus `self.aec_style_library` auf, ruft `validate_style_substitution` auf und setzt bei `Err` `aec_plan_manager_substitution_error`, bei `Ok` `upsert_style_substitution(...)`; `AecPlanManagerSubstitutionRemove` filtert die Quelle heraus; `AecPlanManagerApply` schreibt `config.style_substitutions` jetzt aktiv aus dem Buffer (`.into_iter().collect()`) statt es unverändert zu belassen.
- `src/ui/window/aec_plan_manager.rs`: neue Funktion `substitution_section_view(...)` — Tabelle bestehender Zeilen (Original/Ersatz/Entfernen-Button) + Formularzeile mit zwei `pick_list`s (Wandstil-Namen aus `StyleLibrary.wall_styles`) + "Hinzufügen"-Button + optionaler Inline-Fehlertext; eingehängt in `config_form_view` direkt nach `layer_filter_section`.
- `src/app/view/modal.rs`: neue Felder an `PlanConfigFormState` durchgereicht (Substitutionsliste, Auswahl-Buffer, Fehlertext), analog zum bereits bestehenden Muster für `layer_filter_*`.

### Components
- `src/ui/window/aec_plan_manager.rs` (bestehend, erweitert): neue Sektion + Struct-Erweiterung von `PlanConfigFormState`.
- `src/modules/aec/engine/display_component.rs` (bestehend, erweitert): neue pure Helper-Funktion `upsert_style_substitution`.
- `src/app/mod.rs`/`src/app/update/mod.rs` (bestehend, erweitert): neue State-Felder, `Message`-Varianten, Handler.
- `src/app/view/modal.rs` (bestehend, erweitert): Wiring der neuen Felder in den `AecPlanManager`-Modal-Arm.

### Risks
- Wandstil-Löschung nach Substitutions-Anlage: löscht der Nutzer später einen referenzierten Wandstil aus der Bibliothek, wird die Substitution zu einer "toten Referenz" (analog zum bereits dokumentierten, bewusst tolerierten Fall bei `resolve_display_config_for_scale` mit toter `DisplayConfig`-Referenz in Step 7) — `commands.rs` fällt dann auf den Original-Wandstil zurück, kein Absturz; die UI zeigt in diesem Fall den rohen `WallStyleRef`-String statt eines aufgelösten Namens (dokumentierte Anzeige-Einschränkung, kein Blocker).
- Reihenfolge-Instabilität einer `HashMap`-basierten Anzeige wird durch den `Vec`-Edit-Buffer (Decision 3) vermieden.

# Delivery Steps

### * Step 1: Pure Upsert-Helper und Tests
`upsert_style_substitution` existiert in `display_component.rs`, ist unabhängig von der UI testbar und deckt Upsert-, Neu-Zeilen- und Entfernen-Fälle ab.
- Funktion mit der in Key Decision 3 beschriebenen Signatur implementieren.
- Unit-Tests: neue Quelle wird angehängt; bestehende Quelle wird überschrieben statt dupliziert (Decision 2); Reihenfolge bestehender, nicht betroffener Zeilen bleibt erhalten.

###   Step 2: State- und Message-Wiring im Manager
`App` hält den Substitutions-Edit-Buffer, neue `Message`-Varianten sind verdrahtet, `AecPlanManagerSelect`/`New`/`Duplicate` befüllen den Buffer korrekt aus einer bestehenden `DisplayConfig`.
- Neue State-Felder und `Message`-Varianten gemäß Proposed Changes ergänzen.
- `Select`/`New`/`Duplicate`-Handler um Buffer-Befüllung aus `cfg.style_substitutions` erweitern.
- `AecPlanManagerSubstitutionAdd`/`Remove`-Handler inkl. `validate_style_substitution`-Aufruf und Fehlertext-Zustand implementieren.

###   Step 3: UI-Sektion im DisplayConfig-Manager
Die "Wandstil-Substitutionen"-Sektion ist im Formular sichtbar, funktioniert end-to-end und ist über `AecPlanManagerApply` persistent.
- `substitution_section_view` mit Zeilenliste, Auswahl-Formular und Fehlertext-Anzeige implementieren.
- In `config_form_view` unterhalb der Layer-Filter-Sektion einhängen; `PlanConfigFormState`/`modal.rs`-Wiring ergänzen.
- `AecPlanManagerApply` so anpassen, dass `style_substitutions` aktiv aus dem Buffer geschrieben wird statt unverändert übernommen zu werden.
- Manuelle/Build-Verifikation: `cargo build`/`cargo build --lib` erfolgreich, `cargo test --lib modules::aec::` und `cargo test --lib ui::window::aec_plan_manager::` ohne Regression.