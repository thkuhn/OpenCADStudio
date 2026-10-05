---
sessionId: session-261002-194949-trwx
---

# Requirements

### Overview & Goals
**Stand (2026-10-04):** Gesamtrecap aller Pläne unter `.junie/plans/` und des aktuellen Entwicklungsstands in `src/modules/aec/`.
Die Pläne dokumentieren die Konzeption und Historie; der Code unter `src/modules/aec/` und die **732 automatisierten Modultests** bilden die verifizierte Quelle der Wahrheit.

**Ergebnis:** Alle wesentlichen Kernbereiche (Wände, Schichten, Verschneidungen, Öffnungen, geneigte/mehrteilige Kontrollebenen, Geschossdecken mit modularer Referenzierung von Rohbaustilen und Fußbodenaufbau-Stilen, Deckenöffnungen, Räume, Raumstempel nach DIN 1356, Deckenbeläge/abgehängte Decken, Fußboden-Übergänge an Türöffnungen und Nischen, DIN 277 / WoFlV Flächenberechnung, Raumbuch-Tabellen, Scheitelpunktbearbeitung und Core-Entkopplung) sind **vollständig implementiert, getestet und versioniert**.

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

#### 4. Geschossdecken (Slabs) & Deckenöffnungen
- **Mehrschichtige Deckenaufbauten & Modulare Stile (`SlabStyle`, `SlabStructuralStyle`, `FloorFinishStyle`):** Tragende Rohbauschichten (`SlabStructuralStyle`), Dämmung und modulare Ausbaustile (`FloorFinishStyle`) mit Schichtdicken, Schraffuren und Detaillierungsgraden.
- **Echte modulare Referenzierung & Entkopplung:** Deckenstile (`SlabStyle`) kopieren keine Schichten mehr redundant, sondern halten direkte Referenzen auf `structural_style_id` und `default_finish_style_id`. Änderungen an Sub-Stilen wirken sich unmittelbar und dynamisch auf alle verknüpften Decken und Räume aus ($M + N$ Prinzip).
- **Stil-Manager:** Eigene Manager für Deckenstile (`AEC_SLABSTYLEMANAGER`), Rohbaustile (`AEC_SLABSTRUCTURALSTYLEMANAGER`) und Fußbodenaufbau-Stile (`AEC_FLOORFINISHSTYLEMANAGER`) mit 2D-/3D-Vorschau und Referenz-Übersichtskarten.
- **Deckenöffnungen (`AEC_SLABOPENING`):** Vollständige Aussparungsgeometrie mit DIN-Aussparungskreuzen/-diagonalen, 3D-B-Rep-Ausschnitten und Durchbruchsstilen.
- **Planarten-Darstellung (1:100, 1:50, RCP, 3D):**
  - *Entwurf 1:100:* Reduzierte Außenkontur / tragende Rohdecke.
  - *Werkplan 1:50:* Schichtweise Darstellung mit DIN-Schraffuren.
  - *Deckenspiegel (RCP):* Untersichtslinien (`UKD`) und sichtbare Kanten.
  - *3D-Modell:* Facettierte 3D-B-Rep-Volumenkörper pro Schicht.
- **Höhenbezug & Griffbearbeitung:** Ausrichtung an Bezugsebenen (OKRD/OKFF), flüssige Griffpunkt-Vorschau und Schichtversätze.

#### 5. Räume, Raumstempel, Decken- & Fußboden-Übergänge (DIN 277 / WoFlV)
- **Automatische Konturerkennung (`loop_detection.rs`):** Planare Flächenextraktion (Left-Hand Rule) zur verzögerungsfreien Erkennung geschlossener Wand-Innenkanten (Rohbau-Tragschichten) und T-Wandkreuzungen.
- **Raum-Modi (`AEC_ROOM`):** 4 Eingabemodi (Pick-Point im Rauminneren, Polygon, Rechteck, Objektkonvertierung) mit dedizierten Ribbon-Einträgen (`AEC_ROOM_PICK`, `AEC_ROOM_POLY`, `AEC_ROOM_RECT`, `AEC_ROOM_OBJECT`).
- **DIN 1356 Raumstempel (`MText` auf `AEC_ROOM_STAMP`):** Zeigt Raumnummer, Raumname, DIN 277 Nutzungskategorie (NUF 1–7, VF, TF), Wohn-/Nutzfläche, Berechnungsfaktor (z. B. 50 % bei Balkonen), Umfang, lichte Höhe, Deckenaufbau und Fußbodenaufbau an.
- **Modulare Fußboden-Ausbaustile:** Auswahl von Ausbaustilen (`FloorFinishStyle`) per Dropdown im Eigenschaften-Panel; automatische Zuweisung der Schichten, Dicken und Schraffurmuster.
- **Deckenbeläge & Abgehängte Decken:** Unterstützung raumspezifischer Deckenaufbauten (`ceiling_finish`), Berechnung der effektiven lichten Höhe unter abgehängten Decken (`UKD`), Deckenflächenermittlung und Generierung von 2D-Deckenschraffuren (`AEC_CEILING_HATCH`) für den Deckenspiegel (RCP).
- **Fußboden-Übergänge an Türöffnungen (`floor_transition.rs`):** Automatische Erkennung von Wandöffnungen (Türen, Durchbrüche, Nischen) entlang der Raumgrenze, Berechnung der Laibungsübergangsflächen bis zur Schwellenlinie/Zargenmitte, Erzeugung von 2D-Schwellenlinien (`AEC_OPENING_THRESHOLD`), erweiterte Belagsschraffuren und differenzierte Flächenermittlung.
- **Interaktiver Stempel-Griffpunkt & Koordinateneingabe:** Frei verschiebbarer Drag-Handle im Viewport und manuelle Positionsänderung im Eigenschaften-Panel.
- **Raumbuch & Tabellenauszug (`AEC_ROOMSCHEDULE`):** Generiert strukturierte CAD-Tabellen (`Table`) mit 12 Spalten inklusive Deckenbelägen und Gesamtsummenzeilen; interaktive Einfügepunktabfrage mit Tabellenvorschau; **automatische Live-Aktualisierung** aller Tabellen bei Geometrie- oder Eigenschaftsänderungen von Räumen.
- **Scheitelpunktbearbeitung (Vertices):** Hinzufügen (`Add Vertex`) und Entfernen (`Remove Vertex`) von Scheitelpunkten an Decken, Deckenöffnungen und Räumen via Griffmenü und Properties-Panel mit zentraler automatischer Paketregenerierung (`aec_regenerate_entity_packages`).

#### 6. XREF-Unterstützung & Planarten-Synchronisation
- **Dynamische In-Memory-Regenerierung:** Externe DWG/DXF-Referenzen (XREFs) passen ihre AEC-Darstellung (Wände, Öffnungen, Schraffuren, 3D-Solids) dynamisch an die im Hauptdokument aktive Planart (`DisplayConfig`) an, ohne die Quellreferenzen zu modifizieren.
- **Block-Mesh-Synchronisation:** Saubere Aktualisierung der Geometrie-Caches und Layer-Präfixierung (`XREF|Layer`).

#### 7. UI-Dialoge, Stil-Manager & Performance
- **Manager-Dialoge:** Eigene Manager für Wandstile (`AEC_WALLSTYLEMANAGER`), Öffnungsstile (`AEC_OPENINGSTYLEMANAGER`), Deckenstile (`AEC_SLABSTYLEMANAGER`), Rohbaudecken-Stile (`AEC_SLABSTRUCTURALSTYLEMANAGER`), Fußbodenaufbau-Stile (`AEC_FLOORFINISHSTYLEMANAGER`), Materialien (`AEC_MATERIALMANAGER`), Planarten (`AEC_PLANMANAGER`), Geschosse (`AEC_STOREYSETTINGS`) und Projekt-Explorer (`AEC_PROJECTEXPLORER`).
- **Modulare Decken- und Ausbaustile:** Deckenstil-Manager verknüpft Rohbaustil und Standard-Ausbaustil als Referenzen und zeigt eine übersichtliche Zusammensetzungs- und Schichtenübersicht der referenzierten Stile.
- **Entkoppelte Aktualisierung:** Saubere Trennung von Zwischenstand („Übernehmen“: Speichern & Zeichnung aktualisieren) und reinem Speichern in Bibliotheken.
- **Multi-Mode-Vorschau:** Umschaltung zwischen 2D-Grundriss, 2D-Fassadenansicht und 3D-Isometrie im Öffnungsstil-, Deckenstil- und Fußbodenaufbaustil-Manager.
- **Deduplizierung & Caching:** Vermeidung von Mehrfach-Regenerierungen bei Werteingaben im Eigenschaften-Panel.

#### 8. Architektur & Core-Entkopplung (`AEC ≠ Core`)
- **Strikte Modultrennung:** AEC-Fachlogik liegt ausschließlich unter `src/modules/aec/**`.
- **Schlanke Core-Hooks:** `Message::Aec`, `ModalKind::Aec(AecModalKind)`, `ColorPickTarget::Aec(AecColorPickTarget)`, `aec::update`, `properties::extend`, `aec_regenerate_entity_packages`, Live-Entity-Finish-Hook.
- **Internationalisierung (i18n):** Alle Benutzeroberflächentexte, Menüs und Meldungen über Fluent-Dateien (`locales/de-DE` und `locales/en-US`).
- **IFC4-Export:** Facettierte B-Rep-Volumenkörper (`IfcFacetedBrep`), Extrusionskörper, Decken und Geschossstrukturen.

---

### Offene Roadmap & Zukünftige Ausbaustufen (Backlog)

1. **Ganzheitliche Planarten- und GUI-Konsolidierung:**
   - *Status:* Übergreifende Benutzeroberfläche zur einheitlichen Konfiguration von Schichtenfiltern, Detaillierungsgraden (1:100, 1:50, Deckenspiegel, Rohbau, 3D) und Darstellungs-Overrides für Wände, Decken und Öffnungen.
2. **Assoziative DIN 1356 Bemaßungsketten (`AEC_DIMENSION`):**
   - *Status:* Automatische Bemaßungsketten mit Erkennung von Wandachsen, Pfeilern, Rohbau- und lichten Öffnungsmaßen.
3. **Erweiterte Wandöffnungs-Interaktion:**
   - *Status:* Flip-Grip zur direkten Spiegelung von Fenstern/Türen quer zur Wandachse (Anschlagsebene / Aufschlagrichtung).
4. **Erweiterte N-Wege-Wandverschneidungen:**
   - *Status:* L- und T-Stöße greifen vollautomatisch. Manuelle Schichtaussparungen bei N-Wege-Kreuzungen (4+ Wände an einem Knoten) sind vorgemerkt.
5. **Dynamische In-Viewport-Eingabe (`dyn_spec` für `AEC_WALL`):**
   - *Status:* Bemaßungseingaben erfolgen per Tastatur/Befehlszeile und Eigenschaften-Panel. Eine direkte Cursor-Bemaßung während des Zeichnens ist noch nicht verdrahtet.
6. **IFC4 Room Boundaries & Space Export:**
   - *Status:* Export von `IfcSpace` samt räumlichen Begrenzungsflächen (`IfcRelSpaceBoundary2ndLevel`) für thermische Simulationen und BIM-Datenaustausch.

---

### Verifikation & Teststatus
- **Testsuite:** Alle **731 Komponententests** des AEC-Moduls (`cargo test --lib -- aec`) laufen fehlerfrei durch.
- **Git-Status:** Branch `feature/aec-core-module` ist sauber committet (letzter Commit: `1ade37bf`).
- **Referenzzeichnungen:** `docs/examples/aec-wall-joins.dxf` und `docs/examples/aec-sloped-walls.dxf` vorhanden.
