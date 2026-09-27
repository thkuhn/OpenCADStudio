---
sessionId: session-260916-073620-1igx
---

# Requirements

### Overview & Goals
**Stand (2026-09-27):** Gesamtrecap aller Pläne unter `.junie/plans/` und des aktuellen Entwicklungsstands in `src/modules/aec/`.
Die Pläne dokumentieren die Konzeption und Historie; der Code unter `src/modules/aec/` und die 644 automatisierten Modultests bilden die verifizierte Quelle der Wahrheit.

**Ergebnis:** Alle wesentlichen Kernbereiche (Wände, Schichten, Verschneidungen, Öffnungen, geneigte und mehrteilige Kontrollebenen, Stile, Dialoge, Performance und Core-Entkopplung) sind **vollständig implementiert, getestet und versioniert**.

---

### Erledigte Meilensteine & Komponenten (Code + Plan-Steps ✓)

#### 1. Wand-Modellierung, Schichten & Geometrie
- **Wand-Workflow (`AEC_WALL`):** Einzelsegment-Architektur, Interaktives Zeichnen, Kontur-Vorschau, Achsausrichtung (`WallJustification`), Schichtversätze (`WallLayer.top_offset`, `bottom_offset`), Richtungs-Umkehrung (`AEC_WALLREVERSE`), Griff-Bearbeitung.
- **Wandverbindungen (Joins):** Automatische L- und T-Joins (`join.rs`, `join_ops.rs`), Gehrungen bei gleichem Material, Kern-Paarung (`Structural`), Putz-Taschen (`layer_gaps`), manuelle Join-Overrides per Griff/Kontextmenü, zuverlässige 2D-Verschneidung auch bei Geschosshöhen ($Z \neq 0$).
- **3D-B-Rep-Generierung & Kantenfilterung:** Schnelle direkte Erzeugung facettierter 2-Manifold B-Rep-Prismen (`faceted_solid`), Beseitigung störender Innenkanten (`filter_miter_deck_edges`), Z-Höhen-Schichtversätze.
- **Darstellungskonfigurationen (Planarten):** 2-Ebenen-Regelwerk (`DisplayConfig`), schichtweise Schraffuren, Farb-, Linientyp- und Detaillierungsgrad-Steuerung für Ausführungs-, Entwurfs- und Statikpläne.

#### 2. Geneigte & mehrteilige Kontrollebenen (Dachschrägen & Geschosse)
- **Analytische Ebenenmathematik (`ControlPlane`):** Stützpunkt + Normale, Neigungswinkel in Grad/Prozent (`from_slope`), Konstruktion über 3 Raumpunkte (`from_three_points`), $Z(x,y)$-Höhenauswertung mit Schutz vor vertikalen Ebenen.
- **Mehrteilige/polygonale Kontrollebenen (`ControlPlaneFacet`):** Unterstützung beliebig vieler polygonaler Teilflächen für Staffelgeschosse, Sheddächer und komplexe Dachlandschaften.
- **Wandoberkanten-Projektion:** Wandsegmente unter Facetten passen sich exakt der Neigung/Höhe an; nicht überdeckte Bereiche extrudieren sauber auf die definierte Wandhöhe mit vertikalem Höhensprung. Bindung strikt an die der Wand zugewiesene `top_plane_id`.
- **Interaktive Viewport-Tools:** `AEC_PLANE_3POINT` (3 Raumpunkte picken), `AEC_PLANE_FACET` (Teilpolygone zeichnen), `AEC_PLANE_ASSIGN` (CAD-Objekte als Facetten zuweisen).
- **Bidirektionale Synchronisation & Vorschau:** Ändern, Verschieben oder Löschen von `Face3D`-Körpern auf dem Layer `AEC_CONTROLPLANES` synchronisiert die Ebenen und Facetten im Projektmanager; Icon-Schaltfläche im Ebenenmanager zur temporären orangefarbenen Hervorhebung mit 2-Sekunden-Timer.
- **Hierarchische Ebenenverwaltung (`AEC_STOREYSETTINGS`):** Anzeige von Neigungswinkeln, separate Auflistung aller Facetten mit individueller $\Delta Z$-Höhe, Name, Hervorheben und Löschen.

#### 3. Wandöffnungen (Fenster, Türen, Durchbrüche, Nischen)
- **2D-Grundrissdarstellung:** Realistische Pfosten-/Zargenprofile mit lichtem Maß, wandstärkensensitive Innen- und Außenbänke (`bake_sill_lines`), Schwellen, Verglasungslinien, DIN 1356 Beschriftungs-Generator ($B/H, BRH, UK$).
- **Wiederverwendbare CAD-Blöcke:** Parametrische Komponenten-Slots (`SlotGeometry::Block`) mit `JambPair`, `StretchToFit` und `CenterAnchor`.
- **Positionierung & Aufschlag:** Bezugskanten (`Start`, `Center`, `End`), Querschnitts-Einbautiefe (`cross_axis_offset`), Aufschlagspiegelung (`SwingSide` Innen/Außen) mit XDATA-Persistierung und Flip-Grip.
- **Wandnischen & Wandschlitze:** Optionale Nischentiefe (`depth`) bei Durchbrüchen mit erhaltener Restwand in 2D und 3D.
- **2D-Fassadenansichten (`Elevation2D`):** DIN-Öffnungsdreiecke, Sprossen und Rahmenkonturen direkt in der Wandebene.
- **3D-B-Rep-Performance:** Direkte B-Rep-Konstruktion ohne teure CSG-Booleans (> 40-fache Beschleunigung), Sonderformen (Bogen, Kreis, Dreieck), Schrägschnitt unter geneigten Wänden (`cut_elevation_sloped`).
- **Planabhängiges Löschen:** `ERASE` und `Entf` löschen Öffnungen unter Berücksichtigung der aktiven Planart und regenerieren die Wand sofort.

#### 4. UI-Dialoge, Stil-Manager & Performance
- **Manager-Dialoge:** Eigene Manager für Wandstile (`AEC_WALLSTYLEMANAGER`), Öffnungsstile (`AEC_OPENINGSTYLEMANAGER`), Materialien (`AEC_MATERIALMANAGER`), Planarten (`AEC_PLANMANAGER`), Geschosse (`AEC_STOREYSETTINGS`) und Projekt-Explorer (`AEC_PROJECTEXPLORER`).
- **Entkoppelte Aktualisierung:** Saubere Trennung von Zwischenstand („Übernehmen“: Speichern & Zeichnung aktualisieren) und reinem Speichern in Bibliotheken.
- **Multi-Mode-Vorschau:** Umschaltung zwischen 2D-Grundriss, 2D-Fassadenansicht und 3D-Isometrie im Öffnungsstil-Manager.
- **Deduplizierung & Caching:** Vermeidung von Mehrfach-Regenerierungen bei Werteingaben im Eigenschaften-Panel.

#### 5. Architektur & Core-Entkopplung (`AEC ≠ Core`)
- **Strikte Modultrennung:** AEC-Fachlogik liegt ausschließlich unter `src/modules/aec/**`.
- **Schlanke Core-Hooks:** `Message::Aec`, `ModalKind::Aec(AecModalKind)`, `ColorPickTarget::Aec(AecColorPickTarget)`, `aec::update`, `properties::extend`, Live-Entity-Finish-Hook.
- **Internationalisierung (i18n):** Alle Benutzeroberflächentexte, Menüs und Meldungen über Fluent-Dateien (`locales/de-DE` und `locales/en-US`).
- **IFC4-Export:** Facettierte B-Rep-Volumenkörper (`IfcFacetedBrep`), Extrusionskörper und Geschossstrukturen.

---

### Offene Roadmap & Zukünftige Ausbaustufen (Backlog)

1. **Undo-Treue bei Öffnungslöschung:**
   - *Status:* Beim Löschen einer Öffnung schließt die Wand sofort. Bei `UNDO` wird die Öffnung aus dem OOPS-Cache wiederhergestellt; die Wand schneidet den Durchbruch erst bei der nächsten Wandbearbeitung/Regenerierung nach.
2. **Dynamische In-Viewport-Eingabe (`dyn_spec` für `AEC_WALL`):**
   - *Status:* Bemaßungseingaben erfolgen per Tastatur/Befehlszeile und Eigenschaften-Panel. Eine direkte Cursor-Bemaßung während des Zeichnens ist noch nicht verdrahtet.
3. **Erweiterte Wandverschneidungen:**
   - *Status:* L- und T-Stöße greifen vollautomatisch. Manuelle Schichtaussparungen bei N-Wege-Kreuzungen (4+ Wände an einem Knoten) sind vorgemerkt.
4. **Geschossdecken / Böden (`Slabs`):**
   - *Status:* Datenstrukturen und B-Rep-Extrusion vorbereitet; UI-Werkzeuge und Schichtaufbau-Manager für Decken als nächster großer Baustein geplant.
5. **Automatische AEC-Bemaßung (DIN 1356):**
   - *Status:* Automatische Maßketten mit Erkennung von Wandöffnungen, Pfeilern und Wandstärken.
6. **Räume & Flächen (`Rooms`):**
   - *Status:* Raumstempel für automatische DIN 277 / WoFlV Flächenberechnung.

---

### Verifikation & Teststatus
- **Testsuite:** Alle **644 Komponententests** des AEC-Moduls (`cargo test -p OpenCADStudio --lib modules::aec`) laufen fehlerfrei durch.
- **Git-Status:** Branch `feature/aec-core-module` ist sauber committet (letzter Commit: `7d7a2f07`).
- **Referenzzeichnungen:** `docs/examples/aec-wall-joins.dxf` und `docs/examples/aec-sloped-walls.dxf` vorhanden.
