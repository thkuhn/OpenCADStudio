---
sessionId: session-260823-224147-wgvj
---

# Requirements

### Overview & Goals
Bisher zeigt OpenCADStudio jede AEC-Wand immer mit derselben Geometrie/Darstellung: volle Mehrschicht-Kontur (`WALL_REP`-Kinder, siehe `regenerate_wall_representation` in `src/modules/aec/commands.rs`) mit Hatch je Layer (`hatch_pattern`/`hatch_override`, `wall_style.rs`/`material.rs`). Dieses Backlog-Thema (aus `.junie/plans/aec-material-style-enhancements.md`, Abschnitt "Planarten & Darstellungsvarianten") führt eine Ebene ein, mit der dieselbe Wand je nach Kontext unterschiedlich dargestellt wird — ohne die zugrunde liegende Geometrie/Achse zu verändern.

Nach Klärung mit dem Nutzer umfasst "Planart" alle drei Aspekte, und "Darstellungsvariante" ebenfalls alle drei Aspekte:

**Planart** (WAS gezeigt wird / aus welchem Blickwinkel):
1. Zeichnungsmaßstab/LOD (1:50 = volle Schicht-Kontur, 1:100 = vereinfacht, 1:200 = Einlinien-Darstellung)
2. Planungsphase (Bestand / Abbruch / Neubau) — relevant für Sanierungs-/Umbauprojekte
3. Ansichtstyp (Grundriss / Schnitt / Ansicht)

**Darstellungsvariante** (WIE dieselbe Geometrie gezeichnet wird):
1. Hatch-/Farbschemata (alternative Schraffuren/Farben je Material/Layer)
2. Sichtbarkeits-Overrides je Layer (z. B. Dämmung nur im Detailplan zeigen)
3. Vereinfachte vs. detaillierte Kontur (grobe Einlinie vs. volle Mehrschicht-Kontur)

### Scope
#### In Scope
- Neues Konzept "Plan" (benannte Kombination aus Maßstab/LOD + Planungsphase + Ansichtstyp), das pro Ansicht/Layout aktiv gesetzt werden kann.
- Neues Konzept "Darstellungsvariante" pro Plan: Hatch-/Farbschema-Auswahl, Layer-Sichtbarkeits-Overrides, LOD-Kontur-Stufe (detailliert/vereinfacht/Einlinie).
- Kopplung LOD ↔ Maßstab: bei Wechsel des Zeichnungsmaßstabs wechselt automatisch (bzw. nach konfigurierbarer Schwelle) die Kontur-Detailstufe.
- Persistenz der Plan-/Darstellungsvariante-Definitionen (analog Material-/Wandstil-Bibliothek) sowie der Zuordnung Plan → aktive Variante pro Layout/Ansicht.
- UI: Verwaltung der Pläne/Varianten (analog `aec_material_manager.rs`), Auswahl der aktiven Planart pro Ansicht/Layout.
- Rendering-Integration: `regenerate_wall_representation` (bzw. der Darstellungs-Erzeugungspfad) berücksichtigt die aktive Plan-/Darstellungsvariante bei Kontur-, Hatch- und Sichtbarkeitsentscheidung.

#### Out of Scope
- Planungsphasen-Werkzeuge zur Erfassung von Bestand/Abbruch/Neu-Zuständen selbst (z. B. "Bauteil als Abbruch markieren"-Workflow) — nur die Darstellungslogik dafür wird hier vorbereitet, die eigentliche Erfassung/Zuordnung ist ein Folgeschritt, falls gewünscht.
- Layout-/Papierraum-Verwaltung (Blattgrößen, Titelblöcke) — nicht Teil dieses Themas.
- Automatische Generierung von Schnitten/Ansichten aus dem 3D-Modell (reine Anzeige-/Stil-Logik, keine neue Schnitt-Engine).
- Änderungen an der Join-Constraints-Funktionalität (bereits ausgeliefert, unverändert).

### User Stories
- Als Planer stelle ich für eine Ansicht den Maßstab auf 1:200 und sehe automatisch vereinfachte Einlinien-Wände statt der vollen Mehrschicht-Kontur.
- Als Planer markiere ich eine Ansicht als "Bestand" und sehe alle Wände in einem abweichenden Bestands-Farbschema/Linienstil, ohne die Geometrie zu duplizieren.
- Als Planer blende ich in einer Detailansicht die Dämmschicht ein, die in der Übersichtsansicht standardmäßig ausgeblendet ist.
- Als Planer wähle ich zwischen mehreren gespeicherten Darstellungsvarianten (z. B. "Präsentation", "Ausführung") und die Wanddarstellung passt sich sofort an, ohne die Wanddaten zu verändern.

### Functional Requirements
- Eine Wand ohne explizit konfigurierte Planart/Variante verhält sich wie heute (volle Mehrschicht-Kontur, Standard-Hatch) — keine Regression.
- Wechsel der aktiven Planart/Variante einer Ansicht löst eine Regenerierung der betroffenen Wanddarstellungen aus, ohne die Wandachse/-schichten zu verändern.
- LOD-Stufen sind eindeutig definiert: Detailliert (volle Kontur + Hatch), Vereinfacht (Außenkontur ohne Layer-Trennung), Einlinie (Mittelachse als einzelne Linie).
- Sichtbarkeits-Overrides pro Layer sind additiv und persistent (Roundtrip bei Speichern/Laden).
- Pläne/Varianten sind benannte, wiederverwendbare Bibliothekseinträge (wie Material-/Wandstile), keine Ad-hoc-Einstellungen pro Wand.
- **Prozessauflage:** Bevor ein neues GUI-Element (Manager-Fenster, Dropdown, Formular) implementiert wird, wird zuerst eine Skizze/ein Vorschlag (Layout-Beschreibung bzw. ASCII-/Text-Mockup) zur Abstimmung vorgelegt — analog zum bisherigen Vorgehen bei `aec_material_manager.rs`/`aec_junction_editor.rs`.


# Technical Design

### Current Implementation
- `src/modules/aec/commands.rs::regenerate_wall_representation` erzeugt für jede Wand deterministisch die `WALL_REP`-Kinder (Kontur/Hatch/Solid je Layer), ausgehend von `wall.layers` (`wall_style::Layer`) und der Material-Bibliothek (`engine/material.rs`); Hatch-Muster/Farbe kommen aus `layer.hatch_override` bzw. `material.hatch_pattern`/`line_color` (Zeilen ~1736-1771).
- Es gibt aktuell **keinen Kontextbegriff** "aktive Ansicht/Layout" mit eigener Darstellungslogik — jede Regenerierung erzeugt exakt eine feste, globale Darstellung pro Wand.
- Bereits etablierte Bibliotheksmuster: Material-Bibliothek (`engine/material.rs`) und Wandstil-Bibliothek (`engine/wall_style.rs`) werden über eigene Manager-UIs verwaltet (`aec_material_manager.rs`) und aus einer projektweiten Library-Struktur geladen — dieses Muster ist die Vorlage für "Plan"- und "Darstellungsvariante"-Bibliotheken.
- Join-Constraints (`join.rs`) zeigen bereits das Muster "additiver, persistenter Override, der die Standard-Berechnung ersetzt, mit automatischem Fallback bei Invalidierung" — dasselbe Muster wird für Sichtbarkeits-/Hatch-Overrides wiederverwendet.

### Key Decisions
1. **"Plan" und "Darstellungsvariante" als getrennte, aber verknüpfte Bibliothekstypen.** Ein "Plan" (Maßstab/LOD + Phase + Ansichtstyp) bestimmt primär die LOD-Stufe; eine "Darstellungsvariante" (Hatch/Farbe/Sichtbarkeit) ist unabhängig wählbar und wird einem Plan zugeordnet — vermeidet eine kombinatorische Explosion fester Kombinationen.
2. **LOD-Stufen sind eine feste Enum-Kette (Detailliert/Vereinfacht/Einlinie), keine freie Konfiguration.** Reduziert Komplexität gegenüber beliebigen Detailgraden und deckt die drei genannten Anwendungsfälle klar ab.
3. **Rendering bleibt zustandslos pro Regenerierungsaufruf**, d. h. `regenerate_wall_representation` erhält die aktive Plan-/Varianten-Referenz als Parameter/Kontext statt globalem Mutable State (analog zum bereits bestehenden `static Mutex<Vec<Storey>>`-Antipattern, das hier bewusst nicht wiederholt wird) — stattdessen wird die aktive Zuordnung pro Ansicht/Layout in der Szene/Dokument-XDATA gespeichert und explizit übergeben.
4. **Persistenz erfolgt projektweit statt dateigebunden.** Alle Bibliothekstypen (`DisplayConfig`, Wandstil, Material) werden auf Projektebene gespeichert (Projektdatei/-ordner referenziert von allen Zeichnungsdateien/Geschossen desselben Projekts); die einzelne Zeichnungsdatei speichert nur einen Verweis auf die Projekt-Bibliothek plus die aktive `DisplayConfig`-Auswahl (Referenz statt Kopie), keine eigenen Bibliothekskopien. Ein lokaler Datei-Override bleibt als bewusste, klar markierte Ausnahme möglich, ist aber nicht der Standardfall. Für bereits bestehende, dateigebundene Zeichnungen ist ein Migrationsschritt ("aktuelle Datei-Bibliothek als Projekt-Bibliothek exportieren/verknüpfen") vorzusehen, bevor `DisplayConfig` produktiv genutzt wird.
5. **Additive Sichtbarkeits-/Hatch-Overrides pro (Wand, Variante)**, nicht pro Wand global — eine Wand kann in Variante A sichtbar, in Variante B ausgeblendet sein, ohne dass sich die Basisdaten ändern.
6. **GUI-Elemente werden erst als Vorschlag skizziert, dann implementiert.** Für jedes neue Fenster/Formular (Plan-Manager, Varianten-Manager, Auswahl-Dropdown) wird vor der Implementierung ein Layout-Vorschlag (Text-/ASCII-Skizze der Struktur, Felder, Aktionen) vorgelegt und abgestimmt — erst nach Zustimmung erfolgt die Umsetzung im jeweiligen Delivery-Step.
7. **Sichtbarkeitssteuerung pro Darstellungskomponenten-Slot gilt elementtyp-übergreifend, ist unabhängig von Stil-Substitution nutzbar, und die Slot-Kataloge selbst sind additiv erweiterbar, ohne vorab vollständig final geklärt sein zu müssen.** `ComponentRuleSet.visibility` (Slot-Name → sichtbar?) verwendet denselben Mechanismus für alle Elementtypen (z. B. Wand-Slots wie `AxisLine`, Fenster/Tür-Slots wie `Symbol2D`/`OpeningLine`, spätere Elementtypen). Diese Sichtbarkeitssteuerung ist Teil des feingranularen `Detailed(ComponentRuleSet)`-Overrides, nicht von `StyleSubstitution` — eine reine Wandstil-Substitution kann keine Slots ein-/ausblenden, da Sichtbarkeit kein Wandstil-Attribut ist. Beide Override-Arten (`StyleSubstitution` und `Detailed` inkl. `visibility`) können pro `DisplayConfig` kombiniert werden, z. B. abweichende Layer-Hatch/-Farbe über `StyleSubstitution` **und** ausgeblendete Achslinie über `Detailed.visibility["AxisLine"] = false` in derselben Konfiguration. Voraussetzung dafür ist, dass `visibility`/`style_override` über den Slot-*Namen* (nicht Index/Position) indiziert werden (`HashMap<String, ...>`): Fehlt ein Eintrag in `visibility`/`style_override` für einen (auch neu hinzugefügten) Slot, gilt automatisch der Default (sichtbar, Standardstil) — konsistent mit der Non-Regression-Anforderung, und bestehende Konfigurationen ("Architekt 1:50", "Statik 1:50") müssen bei neuen Slots nicht nachträglich angepasst werden. `StyleSubstitution` ist ohnehin slot-unabhängig und "erbt" neue Slots automatisch vom Zielstil. Deshalb sind Slot-Kataloge pro Elementtyp (`WallComponentSlot`, später `WindowComponentSlot`, ...) additive, rein additive Code-Änderungen und müssen nicht vollständig final geklärt sein, bevor die Umsetzung beginnt — kritisch ist nicht die vollständige Slot-Liste, sondern das Indizierungsschema (typisiertes Enum pro Elementtyp, Adressierung über Namen statt Position); dieses Prinzip ist bereits in Step 1 als Test verankert (Roundtrip-Test mit unbekanntem/neu hinzugefügtem Slot muss auf Default zurückfallen). Der Wand-Slot-Katalog aus den Functional Requirements (a–i) dient als Ausgangspunkt für Step 1–3; der Fenster/Tür-Slot-Katalog wird bewusst als eigener, späterer Step zurückgestellt.

8. **`ComponentOverride::StyleSubstitution(WallStyleRef)` ist ein bestätigter, bevorzugter Schnellweg-Override neben dem feingranularen `Detailed(ComponentRuleSet)`.** Statt für jede Wand/jeden Slot manuell Overrides zu pflegen, kann eine `DisplayConfig` pro betroffenem Wandstil eine Substitution hinterlegen (`HashMap<WallStyleRef, WallStyleRef>`, z. B. "MW 24cm - Architekt" → "MW 24cm - Statik"): die Wand wird dann so dargestellt, als hätte sie den referenzierten Ziel-Wandstil, ohne dass sich Achse, Dicke, Anschlüsse oder Mengenermittlung ändern — nur Layer-Material/-Hatch/-Farbe werden ausgetauscht. Vorrangkette: `Detailed`-Override (`ComponentRuleSet`, falls vorhanden) > `StyleSubstitution` (falls vorhanden) > Original-Wandstil. Sichtbarkeit (`visibility`) bleibt ausschließlich Teil von `Detailed`, da ein Wandstil kein Sichtbarkeitskonzept hat; beide Override-Arten sind pro `DisplayConfig` kombinierbar (z. B. `StyleSubstitution` für Layer-Optik **und** `Detailed.visibility["AxisLine"] = false` gleichzeitig). Eine Konsistenzprüfung beim Anlegen einer Substitution stellt sicher, dass Quelle und Ziel dieselbe Gesamtdicke/Achslage haben, damit Anschlüsse nicht divergieren.

### Proposed Changes
- **`src/modules/aec/engine/plan_view.rs`**: bleibt bestehen, aber verschlankt auf die reine `PlanPhase`/`ViewType`/`DisplayConfig`-Hülle (kein `Lod`, kein `PlanDefinition.scale_to_lod`).
- **Neues Modul `src/modules/aec/engine/display_component.rs`** (ersetzt `display_variant.rs`): `WallComponentSlot`, `ComponentRuleSet`, `ComponentStyleOverride`, `LayerSelection`, referenziert Layer über denselben index-bewussten `LayerRef`-Ansatz wie `join::LayerRef` (siehe bereits gelöster Bug mit doppelten Materialien).
- **`commands.rs`**: `regenerate_wall_representation` erhält einen `&ComponentRuleSet`-Parameter statt `&PlanViewContext { plan, variant }`; pro `WallComponentSlot` wird einzeln geprüft, ob der Slot sichtbar ist (`visibility`), welcher Stil gilt (`style_override`/`layer_style_override`) und welche Schichten einfließen (`layer_filter`), statt eines globalen `Lod`-Schalters.
- **Persistenz**: neue Library-Datei (analog `MaterialLibrary`/`WallStyleLibrary`) für `DisplayConfig`-Einträge, jedoch auf **Projektebene** statt Zeichnungsdatei-Ebene gespeichert; Zeichnungsdateien referenzieren die Projekt-Bibliothek und speichern nur die aktive `DisplayConfig`-Zuordnung pro Ansicht/Layout als XDATA auf dem Layout-Objekt bzw. Ansichts-Entity. Für bestehende Material-/Wandstil-Bibliotheken wird geprüft, ob sie ebenfalls auf Projektebene migriert werden müssen (vorgezogener technischer Vorbereitungsschritt).
- **`ComponentOverride`**: `DisplayConfig` erhält zusätzlich `style_substitutions: HashMap<WallStyleRef, WallStyleRef>` als Schnellweg-Override neben `component_rules` (`Detailed`); Vorrang wie in Key Decision 8 beschrieben.
- **UI**: ein neues Manager-Fenster nach dem `aec_material_manager.rs`-Muster (`aec_plan_manager.rs`) zur Verwaltung von `DisplayConfig`-Einträgen inkl. Slot-Tabelle (Sichtbarkeit + Stil-Override + Schicht-Mehrfachauswahl) plus ein Auswahl-Dropdown im Ansichts-/Layout-Kontext zum Umschalten der aktiven `DisplayConfig` — jeweils erst als Layout-Skizze vorgeschlagen, dann implementiert (siehe Key Decision 6).
- **Auto-Maßstabskopplung** (später/optional, siehe Step 7): bei Änderung des aktiven Zeichnungsmaßstabs kann optional eine passende `DisplayConfig` vorgeschlagen/aktiviert werden — reiner Komfort-Mechanismus, kein Bestandteil des Kernmodells.

### Data Models / Contracts
```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PlanPhase { Existing, Demolition, New }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ViewType { FloorPlan, Section, Elevation }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WallComponentSlot {
    AxisLine,                 // a) Achslinie
    Contour2D,                // b) 2D Gesamtkontur
    ContourHatch2D,           // c) 2D Schraffur Gesamtkontur
    Layers2D,                 // d) 2D Wandschichten
    LayerHatch2D,             // e) 2D Schraffuren der Schichten
    Solid3D,                  // f) 3D Gesamtkörper
    SurfaceStyle3D,           // g) 3D Schraffur/Farbe Oberflächen
    SectionRepresentation,    // h) Darstellung im Schnitt
    ElevationRepresentation,  // i) Darstellung in der Ansicht
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ComponentStyleOverride {
    pub line_style: Option<LineStyleRef>,
    pub hatch_pattern: Option<HatchPatternRef>,
    pub hatch_color: Option<Color>,
    pub fill_color: Option<Color>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LayerSelection {
    All,
    Explicit(Vec<LayerRef>), // konkrete Auswahl einzelner Schichten
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ComponentRuleSet {
    pub visibility: HashMap<String, bool>,               // Slot-Name -> sichtbar?
    pub style_override: HashMap<String, ComponentStyleOverride>, // Slot-Name -> voller Stil-Override
    pub layer_style_override: HashMap<LayerRef, ComponentStyleOverride>, // einzelne Schicht -> Stil-Override
    pub layer_filter: LayerSelection,                     // welche Schichten fließen in Contour2D/Solid3D ein
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisplayConfig {
    pub name: String,                    // z.B. "Architekt 1:50"
    pub discipline: String,              // frei wählbar: "Architektur", "Statik", "Schalplan", ...
    pub scale: Option<f64>,              // informativ, keine Zwangssteuerung
    pub phase: PlanPhase,                // Bestand/Abbruch/Neu
    pub view_type: ViewType,             // Grundriss/Schnitt/Ansicht
    pub component_rules: HashMap<ElementTypeId, ComponentRuleSet>, // "Detailed"-Override, feingranular je Slot
    pub style_substitutions: HashMap<WallStyleRef, WallStyleRef>,  // "StyleSubstitution"-Schnellweg: Quell-Wandstil -> Ziel-Wandstil
}
```
`PlanPhase`/`ViewType` bleiben unverändert erhalten. `Lod` und `PlanDefinition.scale_to_lod` entfallen als Zwangssteuerung zugunsten von `DisplayConfig.component_rules`; der Wand-Slot-Katalog `WallComponentSlot` (a–i) dient als Ausgangspunkt, `WindowComponentSlot` folgt als späterer Step. `style_substitutions` deckt den `ComponentOverride::StyleSubstitution`-Schnellweg aus Key Decision 8 ab: Vorrang `component_rules` (Detailed) > `style_substitutions` (StyleSubstitution) > Original-Wandstil.

### Components
- `src/modules/aec/engine/plan_view.rs`: nur noch `PlanPhase`, `ViewType`, `DisplayConfig`-Hülle.
- `src/modules/aec/engine/display_component.rs` (neu, ersetzt `display_variant.rs`): `WallComponentSlot`, `ComponentRuleSet`, `ComponentStyleOverride`, `LayerSelection`.
- `src/modules/aec/commands.rs`: Erweiterung von `regenerate_wall_representation` um `ComponentRuleSet`-Parameter (slot-basierte Sichtbarkeits-/Style-/Layer-Filter-Prüfung).
- `src/ui/window/aec_plan_manager.rs` (neu): analog `aec_material_manager.rs`, verwaltet `DisplayConfig`-Einträge inkl. Slot-Tabelle; Implementierung erst nach vorgelegtem und abgestimmtem Layout-Vorschlag.
- `src/app/mod.rs`/`src/app/update/mod.rs`: neue `Message`-Varianten für aktive `DisplayConfig`-Auswahl je Ansicht/Layout.

### File Structure
- Neu: `src/modules/aec/engine/display_component.rs`, `src/ui/window/aec_plan_manager.rs`.
- Ändern: `src/modules/aec/engine/plan_view.rs` (verschlankt), `src/modules/aec/commands.rs`, `src/app/mod.rs`, `src/app/update/mod.rs`, `src/app/view/modal.rs`.
- Entfällt: `src/modules/aec/engine/display_variant.rs`, `src/ui/window/aec_display_variant_manager.rs` (ersetzt durch `display_component.rs`/`aec_plan_manager.rs`).

### Risks
- Kombinatorik Plan × Variante × Wand könnte bei vielen Wänden Performance kosten bei jedem Umschalten → Regenerierung bleibt auf betroffene Wände im aktiven Layout begrenzt, kein globaler Full-Rebuild bei jedem Wechsel.
- Sichtbarkeits-Overrides pro Layer könnten mit den bereits bestehenden Join-Constraints-Layer-Referenzen (`join::LayerRef`) kollidieren, falls beide unabhängig auf denselben Layer-Index verweisen → beide nutzen bewusst denselben `LayerRef`-Musteransatz, um konsistent zu bleiben.
- "Vereinfachte/Einlinien"-LOD-Kontur für bereits gejointe (gemiterte) Wände muss weiterhin an Knoten korrekt anschließen, sonst entstehen Lücken beim Maßstabswechsel → Vereinfachungslogik arbeitet auf der bereits berechneten Achsen-/Miter-Geometrie, nicht auf einer separat generierten Kontur.
- Wird der GUI-Skizzen-Vorschlag vor der Implementierung übersprungen (z. B. bei impliziter Freigabe), entsteht das Risiko unerwünschter UI-Iterationen → als verbindlicher Zwischenschritt in den betroffenen Delivery-Steps (Skizze vorlegen → Rückmeldung einholen → erst dann implementieren) festgehalten.


# Delivery Steps

###   Step 1: Datenmodell für DisplayConfig und Komponenten-Slots
Neue Typen `DisplayConfig`, `ComponentRuleSet`, `WallComponentSlot`, `ComponentStyleOverride`, `LayerSelection` (sowie unverändert `PlanPhase`/`ViewType`) existieren, sind serialisierbar und persistierbar als Bibliotheksdatei.
- `src/modules/aec/engine/plan_view.rs` auf `PlanPhase`, `ViewType`, `DisplayConfig`-Hülle verschlanken (kein `Lod`, kein `PlanDefinition.scale_to_lod` mehr).
- `src/modules/aec/engine/display_component.rs` mit `WallComponentSlot`, `ComponentRuleSet`, `ComponentStyleOverride`, `LayerSelection` (index-bewusster `LayerRef`, analog `join::LayerRef`) anlegen.
- Library-Lade-/Speicherfunktionen für `DisplayConfig` analog `MaterialLibrary`/`WallStyleLibrary` implementieren.
- Roundtrip-Tests für Serialisierung/Deserialisierung, inkl. Test "unbekannter/neu hinzugefügter Slot fällt auf Default (sichtbar, Standardstil) zurück".

###   Step 2: Slot-Integration in die Wandregenerierung
`regenerate_wall_representation` erzeugt jeden `WALL_REP`-Kindeintrag nur dann, wenn der zugehörige `WallComponentSlot` laut `ComponentRuleSet.visibility` sichtbar ist, statt einen globalen `Lod`-Schalter zu prüfen.
- `regenerate_wall_representation` (und Aufrufer in `commands.rs`) um einen `&ComponentRuleSet`-Parameter erweitern (statt `PlanViewContext`).
- Pro Slot (`AxisLine`, `Contour2D`, `Layers2D`, `Solid3D`, ...) Sichtbarkeitsprüfung gegen `visibility` einbauen, aufbauend auf der bestehenden gemiterten Geometrie.
- Regressionstests: ohne `ComponentRuleSet`-Overrides identisches Ergebnis zum bisherigen Verhalten (alle Slots sichtbar); mit einzelnen `visibility`-Overrides werden genau die referenzierten Slots ausgeblendet, auch an gejointen Knoten.

###   Step 3: Style- und Layer-Filter-Overrides je Slot (inkl. StyleSubstitution)
Eine aktive `DisplayConfig` überschreibt Stil (Linie/Hatch/Farbe) und Schicht-Auswahl einzelner Slots, ohne die Basisdaten der Wand zu verändern; alternativ kann eine `StyleSubstitution` den kompletten Layer-Stil-Ursprung austauschen.
- Vorrangkette in `commands.rs` einbauen: `style_override`/`layer_style_override` (Slot, aus `ComponentRuleSet`/`Detailed`) > `style_substitutions` (`StyleSubstitution`, ganzer Wandstil-Tausch) > `layer.hatch_override` (Original-Wandstil) > Material-Standard.
- `layer_filter: LayerSelection` auswerten (`All` vs. `Explicit(Vec<LayerRef>)`) für `Contour2D`/`Solid3D`, statt eines groben Alle/Außen/Innen-Filters.
- `style_substitutions: HashMap<WallStyleRef, WallStyleRef>` auswerten: liegt für den Wandstil der Wand ein Ziel-Wandstil vor, werden Layer-Material/-Hatch/-Farbe aus dem Ziel-Wandstil bezogen, Achse/Dicke/Anschlüsse bleiben unverändert; Konsistenzprüfung (gleiche Gesamtdicke/Achslage) beim Anlegen der Substitution.
- Tests: `ComponentRuleSet` mit `style_override` zeigt den überschriebenen Stil; `LayerSelection::Explicit` filtert genau die referenzierten Schichten in Kontur/Körper, andere Slots bleiben unverändert; `StyleSubstitution` zeigt den Ziel-Wandstil-Look bei unveränderter Achse; `Detailed`-Override gewinnt gegenüber gleichzeitig aktiver `StyleSubstitution`.

###   Step 4: Layout-Vorschlag für die Verwaltungs-UI (vor Implementierung)
Ein abgestimmter Layout-Vorschlag (Text-/ASCII-Skizze) für den `DisplayConfig`-Manager und das Auswahl-Dropdown liegt vor, bevor Code geschrieben wird.
- Skizze für `aec_plan_manager.rs` (Liste + Detailformular für `DisplayConfig` inkl. Slot-Tabelle mit Sichtbarkeit, Stil-Override-Button und Schicht-Mehrfachauswahl für `layer_filter`, analog Junction-Editor) entwerfen und zur Abstimmung vorlegen.
- Skizze für das Auswahl-Dropdown im Ansichts-/Layout-Kontext (Umschalten der aktiven `DisplayConfig`) entwerfen und zur Abstimmung vorlegen.
- Rückmeldung einholen und Skizzen entsprechend anpassen, bevor der nächste Step beginnt.

### ✓ Step 5: Verwaltungs-UI für DisplayConfig
Ein neues Manager-Fenster (gemäß abgestimmtem Layout-Vorschlag aus dem vorherigen Step) erlaubt das Anlegen, Bearbeiten und Löschen von `DisplayConfig`-Einträgen inkl. Slot-Sichtbarkeit/-Stil/-Schicht-Filter, `StyleSubstitution`-Zuordnungen sowie das Umschalten der aktiven `DisplayConfig` pro Ansicht/Layout.
- `src/ui/window/aec_plan_manager.rs` gemäß abgestimmter Skizze implementieren (Slot-Tabelle mit Sichtbarkeit + Stil-Override + Schicht-Mehrfachauswahl statt einfachem Lod-Dropdown; zusätzlich eine schlanke Zuordnungsliste "Wandstil → Ersatz-Wandstil" für `style_substitutions`).
- Dropdown/Auswahl im Ansichts-/Layout-Kontext zum Umschalten der aktiven `DisplayConfig` implementieren, mit sofortiger Regenerierung der betroffenen Wände.
- Neue `Message`-Varianten und Zustand in `src/app/mod.rs`/`src/app/update/mod.rs`; Persistenz der aktiven Zuordnung als XDATA auf dem Layout-/Ansichts-Entity.

### ✓ Step 6: Projektweite Bibliotheks-Persistenz
Alle Bibliothekstypen (`DisplayConfig`, Wandstil, Material) werden auf Projektebene statt Zeichnungsdatei-Ebene gespeichert; bestehende Dateien lassen sich verlustfrei migrieren.
- Ist-Zustand der heutigen Material-/Wandstil-Bibliotheks-Persistenz prüfen (datei- oder bereits projektgebunden) als Vorbedingung, bevor `DisplayConfig` darauf aufsetzt.
- Projektdatei-/Projektordner-Konzept einführen (bzw. bestehendes wiederverwenden), das von allen Zeichnungsdateien/Geschossen referenziert wird; `DisplayConfig`-, Wandstil- und Material-Bibliotheken dort ablegen.
- Einzelne Zeichnungsdatei speichert nur Projekt-Referenz plus aktive `DisplayConfig`-Auswahl (und optionale, explizit markierte lokale Overrides), keine Bibliothekskopie.
- Migrations-Werkzeug/-Routine: bestehende, dateigebundene Bibliothek einer Zeichnung als neue Projekt-Bibliothek exportieren/verknüpfen, ohne Datenverlust.
- Tests: mehrere Geschossdateien desselben Projekts sehen identische `DisplayConfig`-Einträge ohne Neudefinition; Änderung in der Projekt-Bibliothek wirkt sich auf alle referenzierenden Dateien aus; Migration einer bestehenden Alt-Datei erzeugt eine äquivalente Projekt-Bibliothek.

### ✓ Step 7 (später/optional): Auto-Maßstabskopplung an den Zeichnungsmaßstab
Reiner Komfort-Mechanismus oberhalb des Kernmodells: ein Wechsel des aktiven Zeichnungsmaßstabs kann optional eine passende `DisplayConfig` vorschlagen/aktivieren, ist aber kein Bestandteil von `DisplayConfig` selbst und bewusst zurückgestellt, bis das Kernmodell (Step 1-5) steht.
- Zuordnungstabelle "bei aktivem Zeichnungsmaßstab X automatisch `DisplayConfig` Y vorschlagen/aktivieren" implementieren.
- Anbindung an die bestehende Maßstabs-/Ansichtslogik, sodass ein Maßstabswechsel die Zuordnung auslöst.
- Manuelle Override-Möglichkeit (Nutzer kann die automatisch vorgeschlagene `DisplayConfig` pro Ansicht übersteuern).
- Tests für Zuordnungsauflösung an Grenzwerten sowie für manuelle Übersteuerung.

### ✓ Step 8: Style-Override-Editor pro Slot im DisplayConfig-Manager
Der in Step 5 dokumentierte offene Follow-up wird umgesetzt: In `aec_plan_manager.rs` kann pro `WallComponentSlot` ein `ComponentStyleOverride` (Linientyp/-farbe, Schraffurmuster/-farbe, Füllfarbe) bearbeitet werden, statt nur die Sichtbarkeit umzuschalten.
- "Bearbeiten"-Aktion pro Slot-Zeile in der Slot-Tabelle ergänzen, die einen kleinen Editor (Popup/Bereich) mit den `ComponentStyleOverride`-Feldern öffnet, analog zum bestehenden Material-Editor.
- Neue `Message`-Varianten zum Öffnen/Schließen des Editors und zum Ändern der einzelnen Override-Felder; Speichern übernimmt die Werte in `ComponentRuleSet.style_override` des bearbeiteten `DisplayConfig`.
- "Entfernen"-Aktion, um einen gesetzten Style-Override wieder auf "Standard" zurückzusetzen (Eintrag aus `style_override` entfernen).
- Tests/Verifikation: bestehende Roundtrip-/Regressionstests bleiben grün; manuelle Prüfung per Build, da UI-Rendering nicht automatisiert getestet wird (bestehende Projektkonvention).


# Geklärte Diskussionspunkte
- `ComponentOverride::StyleSubstitution(WallStyleRef)` wurde bestätigt und ist jetzt Key Decision 8 (siehe Technical Design) sowie Bestandteil von `DisplayConfig.style_substitutions` (Data Models) und Delivery Step 3/5.
- Projektweite statt dateigebundene Bibliotheks-Persistenz für `DisplayConfig`/Wandstil/Material wurde bestätigt und ist jetzt Key Decision 4 sowie Bestandteil von Delivery Step 6.