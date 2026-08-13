---
sessionId: session-260812-011701-iuuy
---

# Requirements

### Overview & Goals
Weitere Anpassungen am AEC-Wandmodell (`src/modules/aec/`, Branch `feature/aec-core-module`), die auf dem bereits umgesetzten Wandstil-/Rendering-Stand aufbauen: (1) eine wählbare Bezugslinie (innere Kante / Achse-Mittellinie / äußere Kante) beim Zeichnen einer Wand, umschaltbar per Strg-Taste; (2) horizontaler Versatz zwischen Schichten eines Wandstils, damit auch undefinierte Luftschichten (Spalte ohne Material) abgebildet werden können; (3) optionaler vertikaler Versatz (unten/oben) je Schicht, damit einzelne Schichten von der Basishöhe der Wand abweichen können; (4) beim Selektieren einer Wand (Verschieben, Endpunkt-Grip, Verlängern) wird immer die komplette Wand (Achse + alle abgeleiteten Kontur-/Hatch-/Solid-Entities) als ein Paket behandelt, nicht die Einzelschichten.

### Scope
**In Scope:**
- Referenzlinien-Modus (`Interior`/`Center`/`Exterior`) für `AEC_WALL`, umschaltbar während des Zeichnens per Strg (analog zu bestehendem `ctrl_down`/`set_ctrl`-Mechanismus in `CadCommand`), inkl. Anzeige des aktiven Modus in der Kommandozeile.
- `Layer`-Datenmodell (`wall_style.rs`) erhält einen optionalen horizontalen Versatz (`gap_before: f64`, Spalt zur vorherigen Schicht) sowie optionale vertikale Versätze (`bottom_offset: f64`, `top_offset: f64`) relativ zur Wand-Basishöhe/-Oberkante.
- `engine/contour.rs` (`layer_contours`) erweitert um Spalt-Berücksichtigung zwischen Schichten (reine Geometrieberechnung, keine neue Schicht-"Luft"-Materialdefinition nötig).
- `wall_layer_extrusions` erweitert um pro Schicht abweichende Extrusionshöhe/-basis gemäß vertikalem Versatz.
- AEC Style Manager GUI (`src/ui/window/aec_style_manager.rs`) erweitert um Eingabefelder für Schicht-Spalt und vertikale Versätze im bestehenden Schicht-Editor.
- Auswahl-/Bearbeitungslogik (`ERASE` hat bereits `expand_with_wall_derived_handles` als Vorbild) wird auf Selektion/Verschieben/Grip-Editing verallgemeinert: Klick auf eine abgeleitete Wand-Entity (Kontur/Hatch/Solid) wählt effektiv die Wand als Ganzes.

**Out of Scope:**
- Persistente Geschoss-Verwaltung, Fenster/Türen, Kontrollflächen (weiterhin zurückgestellt).
- Echte Drag&Drop-Editierung des Layer-Spalts/Versatzes direkt im Viewport (nur über den Style-Manager-Formular-Editor).
- Automatische Wandverbindung/Trimmen an benachbarten Wänden bei T-Stößen/Ecken (bleibt bei der bestehenden vereinfachten Miter-Kontur).

### User Stories
- Als Planer möchte ich beim Zeichnen einer Wand per Strg zwischen "innere Kante", "Achse" und "äußere Kante" als Bezugslinie umschalten, damit ich die Wand exakt an eine gewünschte Flucht anlegen kann.
- Als Planer möchte ich in einem Wandstil zwischen zwei Materialschichten einen definierten Luftspalt angeben können, ohne dafür ein "Luft"-Material anlegen zu müssen.
- Als Planer möchte ich, dass z. B. eine Sockelschicht nicht bis zur vollen Wandhöhe reicht, sondern nur bis zu einer definierten Höhe (oberer/unterer Versatz).
- Als Nutzer möchte ich eine Wand anklicken (egal ob ich die Kontur, die Schraffur oder ein Schicht-Solid treffe) und sie dann als eine zusammenhängende Wand verschieben/dehnen können, ohne dass sich einzelne Schichten gegeneinander verschieben.

### Functional Requirements
- `WallCommand`: neue Phase-unabhängige Umschaltung der Bezugslinie per Ctrl-Taste während der Punktkette; der aktive Modus bestimmt, wie die Klick-Punkte relativ zur resultierenden Achse (Mittellinie) interpretiert werden (Versatz um die halbe/volle Gesamtdicke).
- `WALL_V2`-XDATA um ein `justification`-Feld (`Interior`/`Center`/`Exterior`) erweitert, damit die Wahl nach dem Zeichnen im Properties-Panel nachvollziehbar bleibt (rein informativ, die gespeicherte Achse bleibt geometrisch die Mittellinie).
- `Layer`: `gap_before: f64` (Default 0.0) und `bottom_offset`/`top_offset: f64` (Default 0.0) ergänzt; rückwärtskompatibel für bestehende Bibliotheken (fehlende Felder = 0.0 beim Deserialisieren).
- `layer_contours`: Boundary-Berechnung berücksichtigt `gap_before` als zusätzlichen Abstand zwischen den Randlinien benachbarter Schichten (kein Material/keine Hatch/kein Solid im Spaltbereich).
- `wall_layer_extrusions`: pro Schicht wird die tatsächliche Extrusionshöhe/-basis aus `bottom_offset`/`top_offset` relativ zur Wandhöhe berechnet; Solid wird entsprechend niedriger angesetzt/kürzer, nicht die volle Wandhöhe.
- Style-Manager-Formular: neue Eingabefelder je Schicht-Zeile für Spalt, unteren und oberen Versatz.
- Auswahl: Picking/Highlighting/Verschieben/Grip-Editing einer beliebigen von einer Wand abgeleiteten Entity (Kontur, Hatch, Solid) resultiert in derselben Behandlung wie die Auswahl der Achse selbst — die Achse wird intern als das eigentliche Bearbeitungsobjekt herangezogen, nach jeder Änderung wird `regenerate_wall_representation` erneut aufgerufen.

### Non-Functional Requirements
- Bestehende XDATA-Rundtrip-Fähigkeit darf nicht brechen: alte `WALL_V2`-Records ohne `justification`/Layer-Spalt-/Versatzfelder müssen weiterhin mit sinnvollen Defaults (Center, 0.0) lesbar bleiben.
- Kein Host-API-Umbau nötig; alle Bausteine (Kontur-/Extrusionsberechnung, Auswahl-Erweiterung) bleiben im AEC-Modul bzw. nutzen bereits vorhandene Scene-/Auswahl-Mechanismen.

# Technical Design

### Current Implementation
- `src/modules/aec/commands.rs`: `WallCommand` sammelt Klickpunkte (Mittellinie), fragt danach Stil/Höhe/Dicke ab und schreibt `WALL_V2`-XDATA; `regenerate_wall_representation(scene, wall_handle)` baut aus der Achse + `WALL_V2`-Layern Kontur-Polylinien, Hatch-Entities und `Solid3D`-Extrusionen und trägt deren Handles in `derived_handles` in die XDATA ein; die Achse selbst wird auf den unsichtbaren Layer `AEC_WALL_AXIS` verschoben.
- `engine/contour.rs::layer_contours(centerline, thicknesses)`: liefert N+1 parallele Randlinien für N Schichten, Mittellinie liegt in der Mitte der Gesamtdicke, keine Spalt-Unterstützung.
- `engine/wall_style.rs`: `Layer { material_id, thickness, function }`, `WallStyle { style, layers }`, `effective_layers()` löst Einzel-Elternvererbung linear auf (nächster Vorfahre mit nicht-leerer Layer-Liste gewinnt komplett, kein Feld-Merging).
- `commands.rs::wall_layer_extrusions`: liefert je Schicht `WallLayerExtrusion { footprint, height }`, aktuell für alle Schichten dieselbe Wandhöhe.
- Auswahl/Erase: `expand_with_wall_derived_handles(scene, &mut handles)` (bereits implementiert für `ERASE`/Delete) erkennt anhand der `WALL`/`WALL_V2`-XDATA auf einer Achse-Entity ihre `derived_handles` und fügt sie der zu löschenden Menge hinzu — das ist aktuell **nur** für Löschen verdrahtet, nicht für normale Klick-Selektion/Verschieben/Grip-Editing. Da die Achse selbst unsichtbar ist (`AEC_WALL_AXIS`-Layer), kann der Nutzer aktuell nur die sichtbaren abgeleiteten Entities (Kontur/Hatch/Solid) anklicken — es gibt aber keinen Rückweg von einer abgeleiteten Entity zur Achse.
- `CadCommand`-Trait hat bereits `set_ctrl(bool)`/`ctrl_down`, das der `command_driver` bei jedem Eingabe-Event setzt (genutzt u. a. von bestehenden Zeichenkommandos) — Vorbild für die neue Bezugslinien-Umschaltung in `WallCommand`.

### Key Decisions
- **Bezugslinie per Ctrl-Umschaltung, Achse bleibt intern die gespeicherte Geometrie** (aus Nutzeranfrage übernommen): Der Nutzer klickt konzeptionell auf innere/äußere Kante oder Achse, aber intern wird immer die Mittellinie als `LwPolyline`-Geometrie berechnet und gespeichert (Versatz um `total_thickness/2` in die passende Richtung je nach Modus) — das hält `WALL_V2`/`derived_handles`/`AEC_ROOM`-Kompatibilität vollständig erhalten, nur die Klickpunkt-Interpretation ändert sich.
- **Schicht-Spalt als reines Geometriefeld statt "Luft"-Pseudomaterial** (vom Nutzer explizit gewünscht): `gap_before` erzeugt eine Lücke zwischen zwei Randlinienpaaren, für die keine Kontur/Hatch/Solid erzeugt wird — einfacher als ein Dummy-Material mit leerem Pattern zu pflegen.
- **Vertikaler Versatz nur pro Schicht, nicht pro Wand global**: `bottom_offset`/`top_offset` wirken nur auf die Extrusionshöhe/-basis der jeweiligen Solid-Erzeugung; die 2D-Kontur/Hatch bleibt unverändert (2D-Grundriss zeigt ohnehin keine Höheninformation) — vermeidet unnötige Komplexität im 2D-Pfad.
- **Auswahl-Kollabierung auf die Achse statt Mehrfachauswahl-Bündel**: Beim Anklicken einer abgeleiteten Entity wird intern die zugehörige Achse als das "echte" Auswahlobjekt ermittelt (Rückwärtssuche: jede abgeleitete Entity erhält eine Rückreferenz auf ihre Achse, s. u.) und für Verschieben/Grip-Editing verwendet; sichtbare Hervorhebung kann trotzdem das komplette Paket (Achse + alle `derived_handles`) umfassen. Nach jeder Änderung der Achse wird `regenerate_wall_representation` erneut aufgerufen, wodurch alte abgeleitete Entities verworfen und neu erzeugt werden — das ist konsistent mit dem bereits etablierten "Delete-and-Recreate"-Muster.

### Proposed Changes
1. **Referenzlinien-Modus in `WallCommand`** (`src/modules/aec/commands.rs`): neues Feld `justification: WallJustification` (`Interior`/`Center`/`Exterior`, Default `Center`), `set_ctrl(bool)`-Override togglet zwischen den drei Modi (Ctrl gedrückt = nächster Modus in der Reihenfolge, mit Kommandozeilen-Feedback "Justification: Center"); beim Finalisieren wird die eingegebene Punktkette um `total_thickness/2` (oder `0`/`total_thickness`, je nach Modus) senkrecht zur Wandrichtung verschoben, um die gespeicherte Mittellinie zu erhalten.
2. **`WALL_V2`-Feld `justification`**: rein informatives Feld (String "Interior"/"Center"/"Exterior"), rückwärtskompatibel per fehlendem Feld = "Center".
3. **`Layer`-Erweiterung** (`engine/wall_style.rs`): `gap_before: f64`, `bottom_offset: f64`, `top_offset: f64` mit `#[serde(default)]`, damit bestehende TOML/JSON-Bibliotheken ohne diese Felder weiterhin parsen.
4. **`layer_contours`-Erweiterung** (`engine/contour.rs`): Signatur nimmt statt `&[f64]` (nur Dicken) eine Liste von `(thickness, gap_before)` oder direkt `&[Layer]`-Slices entgegen; berechnet kumulative Offsets inkl. Spalt vor jeder Schicht; Randlinien-Paare für Schichten mit Spalt bleiben geometrisch getrennt (keine Hatch/Solid im Spalt).
5. **`wall_layer_extrusions`-Erweiterung**: pro Schicht wird `effective_height = wall_height - bottom_offset - top_offset` sowie eine Basis-Verschiebung (`bottom_offset` als Z-Versatz) berechnet, an `sweep_model::extruded` weitergegeben (sofern die Extrusionsfunktion einen Basis-Offset unterstützt — sonst wird der Solid nach der Extrusion um `bottom_offset` in Z verschoben).
6. **Style-Manager-Formular** (`src/ui/window/aec_style_manager.rs`): Schicht-Zeilen-Editor um drei zusätzliche `text_input`-Felder (Spalt, unten, oben) erweitert, analog zum bestehenden Dicke-Feld; Live-Vorschau der aufgelösten Schichten zeigt die neuen Werte mit an.
7. **Rückreferenz + Auswahl-Kollabierung**: jede beim Regenerieren erzeugte abgeleitete Entity (Kontur/Hatch/Solid) erhält selbst eine minimale XDATA-Rückreferenz (`OPENCAD_AEC`/`WALL_DERIVED` mit dem Achse-Handle) — analog zum bereits bestehenden Muster für Wirtsbeziehungen (z. B. `storey_id`); eine neue Funktion `resolve_wall_package(scene, handle) -> Handle` liefert für jede Entity (Achse oder Derivat) das Achse-Handle zurück. Selektions-/Move-/Grip-Pfade (`scene::selection`, MOVE-/STRETCH-Kommandos) rufen diese Auflösung auf und ersetzen die geklickte Entity durch die Achse in der eigentlichen Bearbeitungsmenge, ergänzt um `expand_with_wall_derived_handles` für die visuelle Hervorhebung des kompletten Pakets.

### Data Models / Contracts
```rust
// wall_style.rs
pub struct Layer {
    pub material_id: MaterialId,
    pub thickness: f64,
    pub function: LayerFunction,
    #[serde(default)] pub gap_before: f64,
    #[serde(default)] pub bottom_offset: f64,
    #[serde(default)] pub top_offset: f64,
}

// commands.rs
pub enum WallJustification { Interior, Center, Exterior }

// New XDATA kind for derived entities (contour/hatch/solid)
// ["WALL_DERIVED", axis_handle: Handle]
pub fn resolve_wall_package(scene: &Scene, clicked: Handle) -> Handle {
    // if clicked carries WALL_DERIVED XDATA -> return its axis_handle
    // else if clicked itself carries WALL/WALL_V2 -> return clicked
    // else -> return clicked unchanged (not a wall)
}
```

### Components
- `src/modules/aec/commands.rs` (geändert): `WallCommand` Justification-Toggle, `WALL_V2`-Feld, `WALL_DERIVED`-Tagging in `regenerate_wall_representation`, `resolve_wall_package`.
- `src/modules/aec/engine/wall_style.rs` (geändert): `Layer`-Felderweiterung.
- `src/modules/aec/engine/contour.rs` (geändert): Spalt-Unterstützung in `layer_contours`.
- `src/ui/window/aec_style_manager.rs` (geändert): neue Formularfelder je Schicht.
- `src/scene/selection.rs`/Move-/Grip-Pfade (geändert): Aufruf von `resolve_wall_package` beim Aufbau der Bearbeitungsmenge.

### Risks
- **Geometrische Korrektheit der Bezugslinien-Umrechnung an Ecken/Winkeln**: Die Umrechnung von Klickpunkten (innen/außen) auf die gespeicherte Mittellinie ist bei mehrsegmentigen Wänden mit Richtungswechseln nicht trivial exakt (ähnliche Vereinfachung wie bei den bestehenden Miter-Offsets) — bewusst vereinfachtes Verhalten, keine vollständige Eckversatz-Korrektur in dieser Iteration.
- **Rückreferenz-Konsistenz**: `WALL_DERIVED`-Tags müssen bei jeder Regenerierung korrekt neu geschrieben werden; verwaiste Tags (nach fehlerhaftem Cleanup) könnten `resolve_wall_package` auf ein bereits gelöschtes Achse-Handle verweisen lassen — Mitigation: Existenzprüfung vor Verwendung, Fallback auf die geklickte Entity selbst.
- **Migrationsrisiko**: bestehende Wände ohne `gap_before`/`bottom_offset`/`top_offset`/`justification` müssen weiterhin unverändert funktionieren — durch `#[serde(default)]` und String-Fallback "Center" abgesichert.

# Delivery Steps

###   Step 1: Ctrl-Umschaltung der Wand-Bezugslinie (innen/Achse/außen) implementieren
Beim Zeichnen einer Wand kann per Strg-Taste zwischen innerer Kante, Mittellinie und äußerer Kante als Bezugslinie umgeschaltet werden; die gespeicherte Geometrie bleibt weiterhin die Mittellinie.
- Enum `WallJustification { Interior, Center, Exterior }` in `src/modules/aec/commands.rs`.
- `WallCommand` erhält ein `justification`-Feld (Default `Center`) und überschreibt `set_ctrl(bool)`, um bei jedem Ctrl-Tastendruck zum nächsten Modus zu wechseln und den aktuellen Modus über die Kommandozeile anzuzeigen.
- Beim Finalisieren der Wand wird die gesammelte Punktkette abhängig vom Modus senkrecht zur Wandrichtung um 0 / halbe / volle Gesamtdicke verschoben, sodass stets die Mittellinie als `LwPolyline` gespeichert wird.
- `WALL_V2`-Record um ein rein informatives `justification`-Feld erweitert (String), mit Fallback "Center" bei fehlendem Feld für Altbestand.
- Unit-Tests: Umschaltung zyklisch Interior→Center→Exterior→Interior; Punktkette wird für jeden Modus korrekt auf die erwartete Mittellinie umgerechnet (einfacher gerader Wandabschnitt).

###   Step 2: Horizontalen Schicht-Spalt (Luftschicht ohne Material) im Wandstil-Modell und der Kontur-Berechnung ergänzen
Ein Wandstil kann zwischen zwei Schichten einen definierten Spalt angeben, der im Grundriss als Lücke ohne Kontur/Hatch erscheint.
- `Layer` in `src/modules/aec/engine/wall_style.rs` um `#[serde(default)] gap_before: f64` erweitert.
- `layer_contours` in `src/modules/aec/engine/contour.rs` berücksichtigt `gap_before` als zusätzlichen kumulativen Offset vor der jeweiligen Schicht, ohne dass für den Spaltbereich selbst eine Randlinie zur Kontur-/Hatch-Erzeugung genutzt wird.
- `wall_layer_contour_polylines`/`regenerate_wall_representation` in `src/modules/aec/commands.rs` überspringen die Kontur-/Hatch-Erzeugung für den Spaltbereich, erzeugen aber weiterhin korrekte Randlinien für die angrenzenden echten Schichten.
- Unit-Tests: Kontur-Berechnung für 2 Schichten mit Spalt liefert die erwartete Gesamtbreite und Randlinien-Positionen; bestehende Bibliotheken ohne `gap_before` (Feld fehlt) parsen weiterhin korrekt mit Spalt 0.

###   Step 3: Vertikalen Schicht-Versatz (unten/oben) im Wandstil-Modell und der 3D-Extrusion ergänzen
Einzelne Schichten können unten und/oder oben von der vollen Wandhöhe abweichen (z.B. Sockelschicht, die nicht bis zur Decke reicht).
- `Layer` um `#[serde(default)] bottom_offset: f64` und `#[serde(default)] top_offset: f64` erweitert.
- `wall_layer_extrusions` in `src/modules/aec/commands.rs` berechnet je Schicht `effective_height = wall_height - bottom_offset - top_offset` und die zugehörige Basis-Verschiebung; `regenerate_wall_representation` verschiebt das erzeugte `Solid3D` entsprechend `bottom_offset` in Z, bevor es dem Dokument hinzugefügt wird.
- Die 2D-Kontur/Hatch-Erzeugung bleibt unverändert (kein Höheneinfluss im Grundriss).
- Unit-Tests: Extrusionshöhe/-basis für eine Schicht mit unterem und oberem Versatz entspricht der erwarteten reduzierten Höhe und Position; Schicht ohne Versatz verhält sich wie bisher (volle Wandhöhe).

###   Step 4: Style-Manager-Formular um Spalt- und Versatz-Eingabefelder erweitern
Der bestehende Schicht-Editor im AEC Style Manager erlaubt das Eingeben von Schicht-Spalt sowie unterem/oberem Versatz je Schicht.
- `src/ui/window/aec_style_manager.rs`: Schicht-Zeilen-Layout um drei zusätzliche `text_input`-Felder (Spalt, unten, oben) neben dem bestehenden Dicke-Feld ergänzt, mit zugehörigen neuen `Message`-Varianten und Handlern in `src/app/update/mod.rs` (analog zum bestehenden Dicke-Feld-Pattern).
- Die Live-Vorschau der aufgelösten (vererbten) Schichten zeigt die neuen Werte mit an.
- Speichern schreibt die neuen Felder korrekt über `StyleLibrary::upsert_wall_style`/`save_to_default_path`.
- Tests: Formular-Save mit gesetzten Spalt-/Versatzwerten persistiert und lädt die Werte korrekt zurück (Roundtrip-Test auf `WallStyle`-Ebene).

###   Step 5: Wand-Pakete: Selektion, Verschieben und Grip-Editing behandeln Achse und abgeleitete Entities als eine Einheit
Das Anklicken einer beliebigen Wand-Teilentity (Kontur, Hatch, Solid) wählt effektiv die ganze Wand aus; Verschieben und Endpunkt-Grips wirken konsistent auf die gesamte Wand, deren sichtbare Darstellung danach automatisch neu aufgebaut wird.
- Jede in `regenerate_wall_representation` erzeugte abgeleitete Entity erhält eine minimale `WALL_DERIVED`-XDATA-Rückreferenz auf das Achse-Handle.
- Neue Funktion `resolve_wall_package(scene, handle) -> Handle` in `src/modules/aec/commands.rs`, die für eine abgeleitete Entity das zugehörige Achse-Handle liefert (Fallback: unveränderte Handle-Rückgabe für Nicht-Wand-Entities).
- Selektions-/Move-/Grip-Editing-Pfade (`src/scene/selection.rs`, MOVE-/STRETCH-Kommandos in `src/app/commands/`) rufen `resolve_wall_package` auf, um geklickte abgeleitete Entities durch die Achse in der eigentlichen Bearbeitungsmenge zu ersetzen; visuelle Hervorhebung nutzt weiterhin `expand_with_wall_derived_handles` für das komplette Paket.
- Nach jeder Bewegung/Grip-Änderung der Achse wird `regenerate_wall_representation` erneut aufgerufen, damit Kontur/Hatch/Solid konsistent nachgezogen werden.
- Tests: Klick-Simulation auf eine abgeleitete Entity löst über `resolve_wall_package` korrekt die Achse auf; Verschieben einer Wand über eine geklickte Kontur-Entity bewegt Achse und alle abgeleiteten Entities konsistent, ohne dass sich Einzelschichten gegeneinander verschieben.