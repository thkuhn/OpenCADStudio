---
sessionId: session-261026-003939-1xpu
---

# Requirements

### Goal / Outcome
Robuste und geometrisch verlässliche Steuerung von Geschossen, Kontrollebenen, Decken-Ebenenbezügen und Ebenen-Auswahl im Eigenschaften-Panel:
- Kontrollebenen besitzen stets mindestens eine gültige Facette (Polygon); Vorschau-Objekte in der Zeichnung können gefahrlos ausgeblendet, gelöscht oder in anderen Zeichnungen geöffnet werden, ohne gespeicherte Ebenengeometrien zu verfälschen.
- Das Löschen von Kontrollebenen im Geschoss-Manager entkoppelt abhängige Bauteile sicher (`plane_id = None` / Dangling ID) und bewahrt deren gebackene Geometrie (Ursprung, Normale, Facetten) ohne Höhensprünge bei geneigten Dächern oder Formeln.
- Neue Facetten können interaktiv per Punkt-Pick (`[+ Fläche]`) gezielt bestehenden Ebenen zugewiesen werden.
- Decken nutzen ein rückwärtskompatibles Einzelbezugsebenen-Modell mit Justierung (`OKFF`, `OKRD`, `UKD`) und Versatz.
- Ebenen-Dropdowns im Eigenschaften-Panel zeigen wohldefinierte Z-Höhen und Neigungen an, unterstützen Namen mit Klammern, disambiguieren Namenskollisionen und parsen Auswahlen ID-sicher.

### Scope
- **In Scope:**
  - Facetten-Invariante (`!plane.facets.is_empty()`) und Löschschutz für die letzte Facette in `src/modules/aec/engine/control_plane.rs`, `project.rs`, `drawing_sync.rs` und `update.rs`.
  - Geometriesicheres Entkoppeln (Safe Unbind) und Erhalt gebackener Facetten-Snapshots bei Lookup-Misses in `src/modules/aec/engine/wall.rs`, `storey_z.rs`, `update.rs` und `wall_planes.rs` / `slab_planes.rs` / `opening_planes.rs`.
  - Interaktiver Facetten-Pick mit Ziel-Ebenen-Plumbing (`PlaneFacetCommand::with_target`, `AEC_PLANE_FACET_DO <bid|sid|pid>`, Dialog-Reopen) in `src/modules/aec/project/plane_facet.rs`, `spawn.rs` und `aec_storey_settings.rs`.
  - Harmonisierung des Decken-Ebenenbezugs im Eigenschaften-Panel (`slab_top_plane`/`slab_top_offset` als primäre Referenz) unter Beibehaltung der XDATA-Feld-IDs und Formel-Keys in `src/modules/aec/properties.rs`, `slabs/slab.rs` und `slab_planes.rs`.
  - ID-sichere Z-Höhen- und Neigungs-Formatierung in Ebenen-Dropdowns, Disambiguierung gleichnamiger Ebenen sowie tolerantes Suffix-Stripping (`rfind(" (Z")`) in `src/modules/aec/project/wall_planes.rs` und `properties.rs`.
- **Out of Scope:**
  - Modifikationen an Core-Datenstrukturen oder Core-Rendering (`AEC ≠ Core`).
  - Schemaänderungen am IFC4-B-Rep-Exporter.

### Done When
- Das Löschen von Vorschau-Entities in einer Zeichnung oder das Öffnen einer anderen Zeichnung überschreibt niemals die im Manager gespeicherten Facetten oder Neigungen.
- Die letzte verbleibende Facette einer Kontrollebene kann im Geschoss-Dialog nicht gelöscht werden; leere Facettenlisten aus Altdaten heilen sich automatisch über `ensure_control_planes`.
- Das Löschen einer Kontrollebene im Geschoss-Manager belässt alle Bauteile auf ihrer exakten absoluten Z-Lage (auch bei geneigten Ebenen, versetzten Decken und Formel-Offsets), selbst nach Geschosshöhenänderungen oder Rebakes in geschlossenen Zeichnungen.
- Im Geschoss-Dialog startet `[+ Fläche]` den Punkt-Pick für die gewählte Ebene und fügt das neue Polygon direkt hinzu.
- Decken im Eigenschaften-Panel zeigen Bezugsebene, Justierung und Versatz konsistent an, bestehende Formeln (`slab_top_offset`, `slab_base_offset`) bleiben voll funktionsfähig.
- Ebenen-Dropdowns zeigen Z-Höhen/Neigungen an, unterscheiden gleichnamige Ebenen über Geschossnamen und speichern saubere Ebenennamen ohne Z-Suffixe.

# Technical Design

### Decisions
- **Manager-Facetten als Source of Truth / Safe Sync**: `sync_storey_planes_from_drawing` modifiziert Facetten nur, wenn getaggte Vorschau-Entities in der aktiven Szene vorhanden sind. Fehlende Vorschau-Entities (z. B. bei inaktivem Geschoss-Tab, ausgeblendeten Ebenen `visible=false` oder gelöschten Entities) setzen gespeicherte Facetten *nicht* auf Standard-Rechtecke zurück. Das Wiederherstellen von Vorschau-Entities erfolgt explizit über `regenerate_control_plane_previews(&mut Scene, &mut StoreyRef)`.
- **Invariante auf Mutationsebene (`facets.len() >= 1`)**: `ControlPlane::remove_facet`, `StoreyRef::delete_facet` und `AecStoreySettingsDeleteFacet` verweigern das Löschen der letzten Facette (bzw. die UI deaktiviert den Löschen-Button bei `facets.len() == 1`), sodass eine Ebene niemals 0 Facetten aufweist. `ensure_control_planes` erzeugt nur dann eine Fallback-Rechteckfacette, wenn Altdaten oder korrupte JSON-Dateien eingelesen werden.
- **Wall Safe Unbind & Lookup-Miss-Invariante**: In `Wall::rebake_lookup` belässt ein fehlendes Lookup-Ergebnis (`base_plane_id.is_some()` bzw. `top_plane_id.is_some()`, aber `lookup(id) == None`) den bestehenden gebackenen Snapshot (`*_facets`, Ursprung, Normale) unverändert und flacht die Wand *nicht* auf `base_origin + height` ab. Beim Löschen einer Ebene im Geschoss-Manager (`AecStoreySettingsDeletePlane`) wird die gelöschte Ebene aus dem Projekt entfernt; die Bauteile behalten die gelöschte ID als Dangling ID bzw. ungebundenen Snapshot, sodass aktive Zeichnungen, andere offene Zeichnungen und geschlossene Zeichnungen beim Öffnen/Rebaken einheitlich die gebackene Geometrie bewahren. Die explizite Auswahl von `(none)` im Eigenschaften-Panel löscht hingegen gezielt die Plane-ID und Facetten und stellt die Wand auf rein höhenbasiertes Verhalten zurück.
- **Ziel-Plumbing für `AEC_PLANE_FACET`**: `PlaneFacetCommand` wird um `with_target(bid, sid, pid)` erweitert und dispatchet `AEC_PLANE_FACET_DO <bid|sid|pid> <p1_x> <p1_y> <p1_z> ...`. `aec_plane_facet_do` fügt die Facette der Zielebene hinzu (`plane.add_facet(...)`) und `spawn.rs` öffnet den Geschoss-Dialog anschließend automatisch wieder. Ohne Zielargument bleibt das bisherige Verhalten (Erzeugung einer neuen `CompositePlane_*`) für CLI-Aufrufe und bestehende Tests erhalten.
- **Rückwärtskompatible Decken-Bezugsebene**: Die bestehenden Feld-IDs `slab_top_plane` und `slab_top_offset` werden im Eigenschaften-Panel als "Bezugsebene" und "Versatz" dargestellt, während `slab_base_plane`/`slab_base_offset` in der UI verborgen oder als sekundär behandelt werden. Die Formelauswertung in `properties.rs` unterstützt weiterhin alle Alias-Keys (`slab_top_offset`, `top_offset`, `slab_offset`, `slab_base_offset`).
- **Wohldefinierte Z- und Neigungs-Berechnung für Dropdowns**:
  - Horizontal (< 0.1°): `format!("{name} (Z = {:.3} m)", origin_z)` mit `origin_z = plane.origin[2]`.
  - Geneigt (>= 0.1°): `format!("{name} (Z ≈ {:.3} m, {:.1}°)", origin_z, slope)` mit `origin_z = first_facet.origin()[2]` und `slope = first_facet.slope_degrees()`.
  - Multi-Facetten (`facets.len() > 1`): `format!("{name} (Multi, Z ≈ {:.3} m, {:.1}°)", origin_z, slope)` auf Basis der ersten Facette.
  - Die Z- und Neigungsangabe wird auf Ebenenebene berechnet (unabhängig von der XY-Lage des selektierten Objekts), sodass der Dropdown-Label auch bei Mehrfachselektion identisch bleibt.
- **ID-sichere Dropdown-Optionen & Resilientes Parsen**:
  - Optionen werden nach Plane-ID de-dupliziert. Bei Namenskollisionen in verschiedenen Geschossen/Gebäuden wird der Geschossname zur Disambiguierung ergänzt: `format!("{name} [{storey_name}] (Z = ...)")`.
  - `parse_plane_choice` matcht primär exakt gegen die formatierten Optionen der Liste. Fallback schneidet nur bekannte Z-Suffixe ab `rfind(" (Z")` ab (damit Namen wie `Dach (Nord)` nicht beschnitten werden) und matcht gegen den bereinigten Namen nur, wenn dieser projektweit eindeutig ist; abschließender Fallback auf `Uuid::parse_str`.
  - Apply-Routinen persistieren stets den reinen Ebenennamen im XDATA.
- **AEC ≠ Core & Dispatch-only `update.rs`**: Unbind-, Migrations- und Preview-Bereinigungshilfen liegen in Feature-Dateien (`src/modules/aec/project/wall_planes.rs` bzw. `plane_delete.rs`), `update.rs` enthält nur Dispatch-Code.

### Approach & Touches
- `src/modules/aec/engine/control_plane.rs`: Invariante in `remove_facet` absichern; Z- und Neigungsauswertung.
- `src/modules/aec/engine/project.rs`: `delete_facet` und `ensure_control_planes` mit Mindest-1-Facetten-Garantie.
- `src/modules/aec/engine/wall.rs`: `rebake_lookup` Lookup-Miss-Schutz (Snapshot und Facetten bei fehlender Plane-ID nicht löschen; nur bei explizitem `None` ohne Facetten abflachen).
- `src/modules/aec/project/storey_z.rs`: Rebake-Logik bei Geschossänderungen gegen Snapshot-Verlust absichern.
- `src/modules/aec/project/drawing_sync.rs`: Sync schützt gespeicherte Facetten bei fehlenden Vorschau-Entities in anderen Tabs/Zeichnungen.
- `src/modules/aec/update.rs`: `AecStoreySettingsDeleteFacet` (Schutz der letzten Facette) und `AecStoreySettingsDeletePlane` (Dispatch auf Löschhelfer und Preview-Cleanup).
- `src/modules/aec/project/plane_facet.rs` & `src/modules/aec/spawn.rs`: Ziel-Plumbing für `PlaneFacetCommand` und `AEC_PLANE_FACET_DO`.
- `src/modules/aec/ui/aec_storey_settings.rs`: Button `[+ Fläche]` für Punkt-Pick; Löschen-Button für letzte Facette deaktivieren.
- `src/modules/aec/engine/slab.rs`, `src/modules/aec/slabs/slab.rs` & `src/modules/aec/project/slab_planes.rs`: Decken-Ebenenbezug & Justierung.
- `src/modules/aec/properties.rs` & `src/modules/aec/project/wall_planes.rs`: Disambiguierte Z-Höhen-Formatierung, `parse_plane_choice` Suffix-Stripping via `rfind(" (Z")`, ID-Matching und Bereinigung gespeicherter Namen.

### Nuances & Risks
- **Namen mit Klammern**: Durch Suchen nach `rfind(" (Z")` statt `find('(')` bleiben Bezeichnungen wie `Dach (Nord)` vollständig erhalten.
- **Stale Z-Labels**: Wurde eine Ebene verschoben, matcht der veraltete Dropdown-String über den Suffix-Stripper auf den eindeutigen Ebenennamen.
- **Formeln auf Offsets**: Beim Entkoppeln gelöschter Ebenen bleiben Formeln wie `_STORY_H - 0.25` in `formulas` intakt und re-evaluieren relativ zum gebackenen Referenzwert.
- **AEC ≠ Core**: Alle Änderungen verbleiben isoliert in `src/modules/aec/**`.

# Testing

- **Inaktive Tabs / Hidden Planes im Sync**: `sync_storey_planes_from_drawing` bei inaktiver Zeichnung oder `visible == false` ausführen -> gespeicherte Facetten und Neigungen im Manager bleiben unverändert.
- **Löschschutz für letzte Facette**: Aufruf von `delete_facet` bei einer Ebene mit 1 Facette -> Löschung wird verweigert, Facette bleibt erhalten.
- **Safe Unbind & Wall Rebake**: Wand mit geneigter `TopPlane` verknüpfen -> Ebene im Manager löschen -> Geschosshöhe ändern / `rebake_bound_walls` ausführen -> `top_z_at_xy` bleibt an zwei verschiedenen XY-Punkten exakt erhalten (kein Abflachen).
- **Dangling ID / Geschlossene Zeichnungen**: Wand mit Dangling ID in `rebake_lookup` behält Facetten-Snapshot.
- **Explizites `(none)` im Eigenschaften-Panel**: Setzt Wand wie gewohnt auf rein höhenbasiert zurück.
- **Interaktiver Facetten-Zuwachs (`AEC_PLANE_FACET_DO`)**: Punkt-Pick mit Ziel-`plane_id` -> Facette wird zur bestehenden Ebene hinzugefügt, Dialog öffnet sich wieder; CLI-Aufruf ohne Ziel erzeugt neue Ebene.
- **Decken-Ebenenbezug & Justierung**: Wechsel von `OKFF` auf `OKRD` verschiebt Schichten exakt um den Kernversatz (`structural_top_offset`); `slab_top_offset` steuert den Gesamthöhenversatz.
- **ID-sichere Z-Höhen-Dropdowns**: Dropdown-Einträge zeigen `Name (Z = ...)` bzw. `Name (Z ≈ ..., ...°)`; gleichnamige Ebenen in verschiedenen Geschossen erhalten Storey-Zusatz; Auswahl wird sauber geparst und der reine Name ohne Z-Suffix im XDATA gesichert.
- **Namen mit Klammern & Stale Labels**: `Dach (Nord) (Z = 3.000 m)` und veraltete Z-Labels lösen zuverlässig auf.
- **Regression**: Vollständiger Durchlauf von `cargo test --lib -- aec`.

# Delivery Steps

### ✓ Step 1: Invariante für Kontrollebenen-Facetten und Auto-Healing
Goal: Sicherstellen, dass jede Kontrollebene stets mindestens eine gültige Facette besitzt und die im Manager gespeicherten Facetten bei Sync-Vorgängen in fremden/ausgeblendeten Zeichnungen niemals überschrieben werden.
Scope: `src/modules/aec/engine/control_plane.rs`, `src/modules/aec/engine/project.rs`, `src/modules/aec/project/drawing_sync.rs`, `src/modules/aec/update.rs`, `src/modules/aec/ui/aec_storey_settings.rs`
Acceptance Criteria:
- [ ] `ControlPlane::remove_facet` und `StoreyRef::delete_facet` verweigern das Löschen, wenn nur noch eine Facette vorhanden ist (`facets.len() <= 1`).
- [ ] Im Geschoss-Dialog (`aec_storey_settings.rs`) ist der Löschen-Button für Facetten deaktiviert oder ausgeblendet, wenn die Ebene nur eine Facette besitzt.
- [ ] `StoreyRef::ensure_control_planes` erzeugt eine Standard-Rechteckfacette (`preview_rectangle`), falls eine Ebene aus Altdaten mit 0 Facetten geladen wird.
- [ ] `sync_storey_planes_from_drawing` modifiziert Facetten nur bei vorhandenen Preview-Entities in der Szene und tastet Ebenen ohne Preview-Entities (z. B. bei anderer aktiver Zeichnung oder `visible == false`) nicht an.
- [ ] Unit-Tests in `drawing_sync.rs`, `control_plane.rs` und `project.rs` sichern das Verhalten bei inaktiver Zeichnung, ausgeblendeten Ebenen und Löschversuchen der letzten Facette ab.
Verification: `cargo test --lib -- engine::project control_plane drawing_sync` → green

### ✓ Step 2: Geometrie- und Höhensicherheit bei Ebenen-Löschung (Safe Unbind & Wall Rebake)
Goal: Zuverlässiges Löschen von Kontrollebenen im Geschoss-Manager ohne Geometriesprünge an verknüpften Bauteilen, Erhalt gebackener Facetten-Snapshots bei Lookup-Misses und saubere Trennung vom expliziten "(none)"-Unbind.
Scope: `src/modules/aec/engine/wall.rs`, `src/modules/aec/project/storey_z.rs`, `src/modules/aec/project/wall_planes.rs`, `src/modules/aec/project/slab_planes.rs`, `src/modules/aec/project/opening_planes.rs`, `src/modules/aec/update.rs`
Acceptance Criteria:
- [ ] `Wall::rebake_lookup` behält bei Lookup-Miss (`plane_id.is_some()`, aber `lookup(id) == None`) die gebackenen Facetten (`base_facets`/`top_facets`), Ursprung und Normale unverändert bei und flacht die Wand nicht ab.
- [ ] Explizites Zuweisen von "(none)" im Eigenschaften-Panel (`apply_wall_plane_choice`) setzt `plane_id = None`, leert Facetten und stellt die Wand wie gewohnt auf rein höhenbasiertes Verhalten zurück.
- [ ] Beim Löschen einer benutzerdefinierten Kontrollebene im Dialog (`AecStoreySettingsDeletePlane`) wird die Ebene aus dem Projekt entfernt und getaggte Vorschau-Entities in der aktiven Szene werden bereinigt; verknüpfte Bauteile (aktiv, andere offene Zeichnungen, geschlossene Zeichnungen) behalten ihre gebackenen Snapshots ohne Höhensprung bei.
- [ ] Hinterlegte Formeln in `formulas` (z. B. `wall_top_offset = "_STORY_H - 0.25"`) bleiben intakt und re-evaluieren gegen den gebackenen Referenzwert.
- [ ] Unbind- und Löschhilfsfunktionen sind in `src/modules/aec/project/wall_planes.rs` implementiert und getestet; `update.rs` führt nur den Dispatch aus.
- [ ] Unit-Tests in `engine/wall.rs`, `storey_z.rs` und `wall_planes.rs` prüfen: Wand mit Boden und geneigter Top-Plane, Löschen der Top-Plane, Ändern der Geschosshöhe -> `top_z_at_xy` bleibt an zwei XY-Punkten exakt erhalten; Dangling-ID-Test; expliziter Unbind setzt auf flach zurück.
Verification: `cargo test --lib -- engine::wall storey_z wall_planes slab_planes opening_planes` → green

### ✓ Step 3: Interaktiver Facetten-Workflow im Geschoss-Dialog & Ziel-Plumbing
Goal: Bereitstellung eines intuitiven Workflows zum interaktiven Picken und Zuweisen von Teilflächen (Dachschrägen, Gauben, Versprünge) direkt an bestehende Kontrollebenen.
Scope: `src/modules/aec/project/plane_facet.rs`, `src/modules/aec/spawn.rs`, `src/modules/aec/ui/aec_storey_settings.rs`, `src/modules/aec/update.rs`
Acceptance Criteria:
- [ ] `PlaneFacetCommand` unterstützt `with_target(bid, sid, pid)` und übergibt die Ziel-Identifikatoren an `AEC_PLANE_FACET_DO <bid|sid|pid> <points...>`.
- [ ] `aec_plane_facet_do` fügt bei vorhandenem Ziel die neue Facette per `plane.add_facet(...)` zur Ziel-Kontrollebene hinzu; ohne Zielargument erstellt es wie bisher eine neue `CompositePlane_*`.
- [ ] In `spawn.rs` öffnet der Abschluss von `AEC_PLANE_FACET_DO` mit Ziel den Geschoss-Dialog automatisch wieder (`AecStoreySettingsOpen(bid, sid)`).
- [ ] Im Geschoss-Dialog (`aec_storey_settings.rs`) erhält jede Kontrollebene einen Aktionsbutton `[+ Fläche]` zum Starten des interaktiven Punkt-Picks neben dem bestehenden `[Aus Polylinie]`-Button.
- [ ] Unit-Tests in `plane_facet.rs` und `plane_assign.rs` prüfen sowohl den gezielten Facetten-Zuwachs als auch den parameterlosen Fallback.
Verification: `cargo test --lib -- plane_facet plane_assign` → green

### ✓ Step 4: Decken-Ebenenbezug & Justierung mit Rückwärtskompatibilität
Goal: Konsolidierung der Decken-Eigenschaften auf eine primäre Bezugsebene mit Justierung (`OKFF`, `OKRD`, `UKD`) und Offset bei voller Kompatibilität zu bestehenden DWGs, Feld-IDs und Formel-Schlüsseln.
Scope: `src/modules/aec/engine/slab.rs`, `src/modules/aec/slabs/slab.rs`, `src/modules/aec/project/slab_planes.rs`, `src/modules/aec/properties.rs`
Acceptance Criteria:
- [x] Im Decken-Eigenschaften-Panel (`slab_prop_section`) werden `slab_top_plane` und `slab_top_offset` als primäre "Bezugsebene" und "Versatz" dargestellt; Justierung (`slab_justification`) steuert die Schichtausrichtung (`OKFF`, `OKRD`, `UKD`).
- [x] Formelauswertungen und Recompute-Hooks in `properties.rs` unterstützen `slab_top_offset`, `top_offset`, `slab_offset` sowie `slab_base_offset` nahtlos.
- [x] Live-Properties des Decken-Erzeugungsbefehls (`slabs/slab.rs:637-676`) sind an die harmonisierte Bezugsebene und den Versatz angepasst.
- [x] Bestehende Modultests (inklusive `properties.rs:4027-4034` und `engine/slab.rs`) kompilieren und bleiben ohne Schema-Bruch grün.
- [x] Unit-Tests für Höhenberechnung bei Justierungswechsel (`Top`, `StructuralTop`, `Bottom`) und Offset-Anpassung sind hinzugefügt und grün.
Verification: `cargo test --lib -- engine::slab slab_planes slab_prop_section` → green

### ✓ Step 5: ID-sichere Z-Wert-Anzeige in Ebenen-Dropdowns des Eigenschaften-Panels
Goal: Anzeige wohldefinierter Z-Höhen und Neigungswinkel in allen Ebenen-Dropdowns (`BasePlane`, `TopPlane`, `SillPlane`, `HeadPlane`, Decken-Bezugsebene), Disambiguierung gleichnamiger Ebenen, Unterstützung von Klammernamen und ID-sichere Auswahlauflösung.
Scope: `src/modules/aec/properties.rs`, `src/modules/aec/project/wall_planes.rs`, `src/modules/aec/project/slab_planes.rs`, `src/modules/aec/project/opening_planes.rs`
Acceptance Criteria:
- [x] Dropdown-Optionen werden einheitlich formatiert: horizontal `{name} (Z = X.XXX m)`, geneigt `{name} (Z ≈ X.XXX m, N.N°)`, Multi-Facetten `{name} (Multi, Z ≈ X.XXX m, N.N°)`. Die Z- und Neigungsangabe ist entity-unabhängig.
- [x] Optionen werden nach Plane-ID de-dupliziert; gleichnamige Ebenen in verschiedenen Geschossen/Gebäuden werden durch Geschosszusatz (`{name} [{storey_name}] (Z = ...)`) eindeutig unterscheidbar gemacht.
- [x] `plane_label` und Dropdown-Optionsliste nutzen denselben Formatter.
- [x] `parse_plane_choice` matcht primär exakt gegen formatierte Optionen; Fallback schneidet nur bekannte Suffixe via `rfind(" (Z")` ab und matcht Namen nur bei projektweiter Eindeutigkeit; Fallback auf UUID.
- [x] Namen mit Klammern (z. B. `Dach (Nord)`) werden nicht vorzeitig abgeschnitten.
- [x] Die Apply-Funktionen (`apply_wall_plane_choice`, `apply_slab_plane_choice`, `apply_opening_plane_choice`) speichern stets den reinen Ebenennamen im XDATA (kein Persistieren formatierter Z-Suffixe).
- [x] Unit-Tests in `aec::properties` und `wall_planes.rs` prüfen: gleichnamige Ebenen in verschiedenen Geschossen, Namen mit Klammern, Stale-Label-Auflösung nach Z-Änderung und saubere XDATA-Persistierung.
Verification: `cargo test --lib -- aec::properties wall_planes slab_planes opening_planes` → green