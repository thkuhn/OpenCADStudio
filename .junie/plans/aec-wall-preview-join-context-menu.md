---
sessionId: session-260812-011701-iuuy
---

# Requirements

### Overview & Goals
Weitere Ergänzungen am AEC-Wand-Workflow (`src/modules/aec/commands.rs::WallCommand`, Branch `feature/aec-core-module`), aufbauend auf dem bereits vorhandenen Live-Properties-Panel, Wandstil-System und `regenerate_wall_representation`. Ziel: eine realistischere Zeichen-/Bearbeitungs-Vorschau (Achse + Außenkontur statt nur Achslinie), frühere Nutzerführung bei fehlendem Wandstil, sowie neue Wand-spezifische Bearbeitungsfunktionen (L-/T-Verbindung, Verlängern, Ausrichtung ändern, automatisches Verschneiden beim Zeichnen, dynamische Längen-/Höhen-Attribute) inklusive eines Wand-Kontextmenüs.

### Scope
**In Scope:**
1. **Kontur-Vorschau statt reiner Achslinie**: Während `AEC_WALL` (Zeichnen) und während Grip-/Move-Bearbeitung wird zusätzlich zur Achse die aufgelöste Außenkontur (Gesamtdicke aller Schichten) live gerendert.
2. **Frühe Stilwarnung**: Der "kein Wandstil gewählt"-Hinweis erscheint bereits nach dem ersten gesetzten Punkt, nicht erst beim Versuch, die Wand zu beenden.
3. **Ausrichtung in der Vorschau**: Ctrl-Umschaltung der Justification (Interior/Center/Exterior) wird sofort in der Kontur-Vorschau sichtbar, nicht nur in der gespeicherten Achse.
4. **Wand-Verbindungen (Join)**: Neue Kommandos zum nachträglichen Erzeugen von L- und T-Verbindungen zwischen zwei ausgewählten Wänden (Achsen trimmen/verlängern, Konturen neu berechnen).
5. **Wand verlängern**: Neues Kommando, das eine Wand bis zu einem projizierten Punkt (z. B. Schnittpunkt mit einer anderen Wand/Linie oder einem gepickten Punkt) verlängert.
6. **Ausrichtung ändern (nachträglich)**: Bestehende Wand kann nachträglich zwischen Interior/Center/Exterior umgeschaltet werden (Properties-Panel oder Kontextmenü), Achse wird entsprechend neu positioniert.
7. **Automatisches Verschneiden beim Zeichnen (Ausbaustufe 2)**: Wird ein Punkt beim Zeichnen einer neuen Wand in der Nähe einer bestehenden Wand gefangen (Snap-Radius), wird automatisch eine L-/T-Verbindung erzeugt.
8. **Dynamische Länge/Höhe**: Soweit machbar, editierbare dynamische Maßzahlen (Länge, Höhe) direkt am gezeichneten/selektierten Objekt im Viewport (2D und 3D), analog zu bestehenden dynamischen Eingabefeldern anderer Zeichenkommandos.
9. **Wand-Kontextmenü**: Erweiterung des bestehenden Rechtsklick-Kontextmenüs (`src/app/view/overlay.rs::viewport_context_menu_overlay`, `selection.context_menu`) um Wand-spezifische Einträge (Join, Extend, Ausrichtung ändern), sichtbar wenn die Selektion (ausschließlich) Wände enthält.

**Out of Scope:**
- Persistente Geschoss-Verwaltung, Fenster/Türen, Kontrollflächen (weiterhin zurückgestellt).
- Vollautomatische Graph-basierte Erkennung *aller* Wandkreuzungen im Dokument (nur explizite/gepickte bzw. Snap-basierte Verschneidung zweier Wände zur Zeichenzeit).
- Änderungen an der Stil-/Material-Datenmodell-Logik selbst.

### User Stories
- Als Planer möchte ich beim Zeichnen einer Wand sofort die tatsächliche Wanddicke als Kontur sehen, damit ich die Platzierung besser einschätzen kann.
- Als Nutzer möchte ich sofort nach dem ersten Klickpunkt erfahren, dass noch kein Stil gewählt ist, statt erst am Ende überrascht zu werden.
- Als Planer möchte ich zwei sich kreuzende/stoßende Wände per Kommando sauber verbinden (L-Ecke, T-Stoß), ohne die Konturen manuell nachzuziehen.
- Als Nutzer möchte ich eine Wand bis zu einem Bezugspunkt verlängern können, ohne sie komplett neu zu zeichnen.
- Als Nutzer möchte ich beim Zeichnen einer neuen Wand automatisch eine saubere Verbindung zu einer bereits vorhandenen Wand bekommen, wenn ich in deren Nähe klicke.
- Als Nutzer möchte ich Länge und Höhe einer Wand direkt am Objekt ändern können, ohne den Umweg über das Properties-Panel.
- Als Nutzer möchte ich per Rechtsklick auf eine oder mehrere selektierte Wände die neuen Funktionen (Join, Extend, Ausrichtung) direkt erreichen.

### Functional Requirements
- Live-Vorschau während `AEC_WALL`/Grip-Editing rendert zusätzlich zur (unsichtbaren) Achse eine sichtbare Vorschau-Kontur basierend auf `wall_layer_contour_polylines`/Gesamtdicke.
- `WallCommand` zeigt die Stil-Warnung bereits ab dem ersten gesetzten Punkt (nicht erst bei `on_enter`).
- Justification-Wechsel (Ctrl) aktualisiert die Vorschau-Kontur in Echtzeit.
- `AEC_WALLJOIN`: wählt zwei Wände, erkennt L- oder T-Konfiguration anhand der Achsgeometrie, trimmt/verlängert die Achsen entsprechend, ruft `regenerate_wall_representation` für beide auf.
- `AEC_WALLEXTEND`: verlängert die Achse einer selektierten Wand bis zu einem gepickten/projizierten Punkt.
- Ausrichtung einer bestehenden Wand kann über Properties-Panel/Kontextmenü geändert werden; Achse wird entsprechend neu berechnet, Kontur/Solids regeneriert.
- Beim Zeichnen einer neuen Wand wird ein Punkt innerhalb eines konfigurierbaren Verschneidungs-Radius zu einer bestehenden Wandachse automatisch gefangen und löst denselben Join-Mechanismus wie `AEC_WALLJOIN` aus.
- Dynamische Attribute (Länge/Höhe) sind, soweit technisch machbar, direkt am Objekt im 2D- und 3D-Viewport editierbar.
- Rechtsklick auf eine Selektion, die ausschließlich `WALL_V2`-Wände enthält, zeigt zusätzliche Menüpunkte Join/Extend/Ausrichtung ändern.

### Non-Functional Requirements
- Bestehende XDATA-Rundtrip-Fähigkeit und bestehende Tests (`cargo test --lib aec`, `polyline`, `properties`) bleiben unverändert funktionsfähig/grün.
- Kein Host-API-Umbau nötig für die Kontextmenü-Erweiterung (bestehendes `selection.context_menu`-System wird wiederverwendet).

# Technical Design

### Current Implementation
- `src/modules/aec/commands.rs::WallCommand`: Punktkette (`Drawing`-Phase), Justification-Toggle per Ctrl (`WallJustification::{Interior,Center,Exterior}`), Live-Properties-Panel (`live_properties`/`apply_live_property`) für Stil/Höhe, Pflicht-Stilauswahl erst bei `on_enter`/Finalisieren (`no_style_warning`, gerade erst ergänzt), `sync_live`/`build_entity` senden nur die Achs-`LwPolyline` als Live-Vorschau-Entity.
- `regenerate_wall_representation` (gleiche Datei) baut aus der fertigen Achse + `WALL_V2`-Layern Kontur-Polylinien (`wall_layer_contour_polylines`/`engine/contour.rs::layer_contours`), Hatch-Entities und `Solid3D`-Extrusionen je Schicht — läuft aber erst **nach** Fertigstellung/Bearbeitung, nicht als Live-Vorschau während des Zeichnens.
- `engine/contour.rs::layer_contours` liefert bereits alle nötigen parallelen Randlinien inkl. Gap-Behandlung — direkt wiederverwendbar für eine Live-Vorschau-Außenkontur (nur äußerste + innerste Randlinie nötig, keine Einzel-Schicht-Aufteilung).
- `src/app/view/overlay.rs::viewport_context_menu_overlay` + `selection.context_menu`/`right_click_entered` (`src/scene/pick/selection_state.rs`): bereits produktives, generisches Rechtsklick-Kontextmenü-System mit bedingten Einträgen je nach Selektion (`has_selection`) — exaktes Vorbild für neue Wand-spezifische Einträge.
- `resolve_wall_package`/`WALL_DERIVED`-XDATA (aus vorheriger Session) sorgt bereits dafür, dass Klick auf Kontur/Hatch/Solid die Wandachse als eigentliches Bearbeitungsobjekt auflöst — wichtig als Grundlage für Join/Extend/Kontextmenü, die auf Achs-Handles operieren müssen.
- Kein bestehendes `HLR`/Trim-/Extend-Kommando für generische Linien in diesem Umfang gefunden (frühere Session-Recherche); Standard-`TRIM`/`EXTEND`-Kommandos existieren im Host für generische 2D-Geometrie und können als Vorbild für die Trimm-/Verlängerungs-Mathematik dienen (Schnittpunkt-Berechnung zwischen Segmenten).
- Dynamische Eingabefelder ("dynamic input") existieren bereits für andere Zeichenkommandos (Kommentar im Code zu "dynamic-input values" in `src/app/commands/mod.rs`) — als Vorbild für die geforderten dynamischen Längen-/Höhen-Attribute.

### Key Decisions
- **Wandverbindung in zwei Ausbaustufen** (vom Nutzer bestätigt): zuerst explizite Kommandos `AEC_WALLJOIN`/`AEC_WALLEXTEND` (Auswahl zweier Wände bzw. einer Wand + Zielpunkt), danach als Ausbaustufe automatisches Fangen/Verschneiden während `AEC_WALL` selbst — reduziert Risiko, liefert früh nutzbaren Mehrwert.
- **Priorisierung Rendering vor Funktionen** (vom Nutzer bestätigt): Kontur-Vorschau, frühe Stilwarnung und Ausrichtungs-Reflektion in der Vorschau werden zuerst umgesetzt, bevor Join/Extend/Auto-Verschneiden/dynamische Attribute/Kontextmenü folgen.
- **Live-Kontur als zusätzliche, rein visuelle Vorschau-Entity**: Die Achse bleibt weiterhin die einzige XDATA-tragende Geometrie während des Zeichnens; die Kontur wird als zusätzliche, nicht-persistente Vorschau-Entity über denselben `CmdResult::UpdateLiveEntity`-Mechanismus gerendert (kein neues Persistenzformat nötig), analog dazu, wie andere Zeichenkommandos bereits mehrere Live-Vorschau-Elemente parallel anzeigen.
- **Join/Extend als eigenständige `CadCommand`-Implementierungen**, nicht als Erweiterung der bestehenden `WallCommand`: Join/Extend operieren auf *bereits existierenden* Wänden (Selektion statt Zeichnen), daher eigene, fokussierte Kommandos statt Überladung von `WallCommand`.
- **Kontextmenü-Erweiterung rein additiv im bestehenden `viewport_context_menu_overlay`**: neue Bedingung "Selektion besteht ausschließlich aus Wänden" (Prüfung via `wall_v2_from_entity` auf alle selektierten Handles) fügt zusätzliche Einträge ein, ohne bestehende Einträge/Verhalten zu verändern.
- **Dynamische Länge/Höhe als "soweit machbar"-Ziel**: wird als letzter, explizit risikobehafteter Schritt eingeplant; falls die bestehende dynamische-Input-Infrastruktur nicht direkt auf 3D-Höhe erweiterbar ist, wird zumindest die 2D-Längenanzeige/-Eingabe umgesetzt und die 3D-Einschränkung dokumentiert.

### Proposed Changes
1. **Live-Kontur-Vorschau**: `WallCommand::sync_live`/`build_entity` erweitert um eine zweite Vorschau-Entity (Außenkontur, aus `layer_contours`/Gesamtdicke berechnet), die parallel zur Achse als Live-Entity gesendet wird; Justification-Wechsel (Ctrl) berechnet die Kontur relativ zur aktuellen Klickpunkt-Interpretation neu.
2. **Frühe Stilwarnung**: `no_style_warning`-Logik wird bereits nach `on_point` (erstem Punkt) statt erst bei `on_enter` gesetzt, sofern `requires_style_selection()` wahr ist und kein Stil gewählt wurde; `prompt()` zeigt den Hinweis entsprechend früher.
3. **`AEC_WALLJOIN`**: neues `CadCommand` (2-Punkt-artige Auswahl zweier Wand-Achsen oder Nutzung der aktuellen Selektion), Geometrie-Helfer `join_wall_axes(axis_a, axis_b) -> (trimmed_a, trimmed_b, corner_type: L|T)` in `engine/` (Schnittpunkt-Berechnung analog zu bestehenden Trim/Extend-Mustern im Host), schreibt getrimmte Achsen zurück, ruft `regenerate_wall_representation` für beide Wände.
4. **`AEC_WALLEXTEND`**: neues `CadCommand`, verlängert die gewählte Wandachse bis zum gepickten/projizierten Punkt (Schnitt mit Ziel-Linie oder direkter Punkt), ruft `regenerate_wall_representation`.
5. **Ausrichtung nachträglich ändern**: neue Properties-Panel-/Kontextmenü-Aktion, die die Achse gemäß gewählter Justification neu berechnet (Wiederverwendung der bereits in `WallCommand` vorhandenen Umrechnungslogik) und regeneriert.
6. **Automatisches Verschneiden (Ausbaustufe 2)**: `WallCommand::on_point` prüft bei jedem gesetzten Punkt einen konfigurierbaren Snap-Radius gegen vorhandene Wandachsen im Dokument; bei Treffer wird beim Finalisieren automatisch derselbe Join-Mechanismus wie `AEC_WALLJOIN` angewendet.
7. **Dynamische Attribute**: Prüfung/Erweiterung der bestehenden dynamischen-Input-Infrastruktur um Länge (2D, sicher machbar) und Höhe (3D, ggf. eingeschränkt) für Wände; Umsetzung im Rahmen der bereits vorhandenen Live-Properties-Panel-Mechanik ergänzt um direktes In-Viewport-Editing, soweit die Host-Infrastruktur das zulässt.
8. **Wand-Kontextmenü**: `viewport_context_menu_overlay` erhält einen neuen Zweig, der aktiv wird, wenn alle selektierten Handles `wall_v2_from_entity` erfolgreich liefern; zeigt Einträge "Join", "Extend", "Change Justification" (letzteres ggf. als Untermenü Interior/Center/Exterior analog zum bestehenden Draw-Order-Untermenü-Muster).

### Components
- `src/modules/aec/commands.rs` (geändert): Live-Kontur-Vorschau, frühe Stilwarnung, neue `WallJoinCommand`/`WallExtendCommand`, Auto-Snap-Erweiterung in `WallCommand::on_point`.
- `src/modules/aec/engine/contour.rs` (ggf. erweitert): Hilfsfunktion für reine Außenkontur-Berechnung (Gesamtdicke) zur Wiederverwendung in der Live-Vorschau.
- Neues Modul (z. B. `src/modules/aec/engine/join.rs`): Geometrie-Helfer für L-/T-Verbindungs-/Verlängerungs-Berechnung, `std`-only und testbar.
- `src/app/view/overlay.rs` (geändert): neuer bedingter Zweig für Wand-Kontextmenü-Einträge.
- `src/app/update/mod.rs`/`src/app/commands/draw.rs` (geändert): neue Kommandos `AEC_WALLJOIN`/`AEC_WALLEXTEND` registriert, neue `Message`-Varianten für Kontextmenü-Aktionen und Ausrichtungsänderung.

### Risks
- **Geometrische Robustheit von Join/Extend**: Schnittpunkt-/Trimm-Berechnung bei nicht-orthogonalen oder degenerierten Wandkonfigurationen (parallele Wände, keine echte Kreuzung) muss robust mit Fehlermeldung statt Panic behandelt werden.
- **Auto-Snap-Komplexität**: automatisches Verschneiden während des Zeichnens kann bei dicht liegenden Wänden zu unerwünschten Verbindungen führen — konfigurierbarer Radius und klares visuelles Feedback (Snap-Marker) sind nötig.
- **Dynamische 3D-Attribute**: falls die bestehende Host-Infrastruktur für dynamische Eingaben nicht ohne Weiteres auf 3D-Viewport-Interaktion übertragbar ist, wird dieser Teilpunkt ggf. auf die 2D-Ansicht beschränkt umgesetzt — wird im entsprechenden Delivery-Schritt explizit geprüft und dokumentiert.
- **Kontextmenü-Bedingungslogik**: gemischte Selektionen (Wände + andere Entities) dürfen die neuen Einträge nicht fälschlich anzeigen — strikte "alle selektierten Handles sind Wände"-Prüfung nötig.

# Delivery Steps

###   Step 1: Live-Kontur-Vorschau beim Zeichnen und Bearbeiten von Wänden ergänzen
Beim Zeichnen und Grip-Editieren einer Wand wird zusätzlich zur Achse die tatsächliche Außenkontur live gerendert.
- Hilfsfunktion zur reinen Außenkontur-Berechnung (Gesamtdicke aus `resolved_layers`) in `src/modules/aec/engine/contour.rs` ergänzen, basierend auf der bestehenden `layer_contours`-Logik.
- `WallCommand::sync_live`/`build_entity` in `src/modules/aec/commands.rs` erweitert um eine zusätzliche Vorschau-Entity (Außenkontur), die parallel zur Achse als Live-Entity gesendet wird.
- Grip-/Move-Editing-Pfad (`resolve_wall_package`/`regenerate_wall_representation`-Aufrufstellen) prüft, ob während einer laufenden Bearbeitung ebenfalls eine Live-Konturvorschau statt sofortiger voller Regenerierung sinnvoll ist, oder ob die bestehende Delete-and-Recreate-Regenerierung für diesen Fall ausreicht.
- Unit-Tests für die neue Außenkontur-Berechnungsfunktion (einfaches Rechteck-Segment, mehrschichtiger Stil).

###   Step 2: Frühe Stilwarnung und Live-Ausrichtungs-Reflektion in der Vorschau
Der Hinweis auf einen fehlenden Wandstil erscheint bereits nach dem ersten Punkt, und ein Ausrichtungswechsel während des Zeichnens aktualisiert sofort die Kontur-Vorschau.
- `WallCommand::on_point`/`no_style_warning`-Logik in `src/modules/aec/commands.rs` so angepasst, dass die Warnung bereits nach dem ersten gesetzten Punkt (statt erst bei `on_enter`) aktiv wird, sofern `requires_style_selection()` zutrifft.
- Justification-Ctrl-Toggle löst eine Neuberechnung der Live-Kontur-Vorschau (aus Schritt 1) mit der neuen Ausrichtung aus.
- Tests: Warnung erscheint bereits direkt nach dem ersten Punkt bei vorhandener Stilbibliothek; Kontur-Vorschau ändert sich nachweislich bei Ausrichtungswechsel (z.B. Versatz der Randlinien relativ zur Klickpunktkette).

###   Step 3: AEC_WALLJOIN implementieren (L-/T-Verbindung zweier bestehender Wände)
Zwei ausgewählte Wände können per Kommando sauber an einer Ecke (L) oder einem Stoß (T) verbunden werden.
- Neues Modul `src/modules/aec/engine/join.rs`: reine Geometriefunktion `join_wall_axes(axis_a, axis_b) -> Result<(Vec<DVec3>, Vec<DVec3>, JoinKind), JoinError>` (Schnittpunkt-/Trimm-Berechnung, erkennt L- vs. T-Konfiguration).
- Neues `CadCommand` `WallJoinCommand` in `src/modules/aec/commands.rs`, das zwei Wand-Achsen (per Selektion oder Klick, unter Nutzung von `resolve_wall_package`) entgegennimmt, die getrimmten Achsen zurückschreibt und `regenerate_wall_representation` für beide Wände aufruft.
- Neues Kommando `AEC_WALLJOIN` registriert (`src/app/commands/draw.rs`), Ribbon-Button in der bestehenden Architecture-Gruppe.
- Unit-Tests für `join_wall_axes` (L-Konfiguration, T-Konfiguration, parallele/degenerierte Wände liefern kontrollierten Fehler statt Panic).

###   Step 4: AEC_WALLEXTEND und nachträgliche Ausrichtungsänderung implementieren
Eine bestehende Wand kann bis zu einem Zielpunkt verlängert werden, und ihre Ausrichtung (Interior/Center/Exterior) kann nachträglich geändert werden.
- Neues `CadCommand` `WallExtendCommand`: verlängert die Achse der selektierten Wand bis zu einem gepickten/projizierten Punkt (Wiederverwendung der Schnittpunktlogik aus `join.rs`, falls Zielpunkt auf einer anderen Wand/Linie liegt), ruft `regenerate_wall_representation` auf.
- Neues Kommando `AEC_WALLEXTEND` registriert, Ribbon-Button ergänzt.
- Neue Properties-Panel-Aktion zur nachträglichen Änderung der Justification einer bestehenden Wand: berechnet die Achse gemäß gewählter Ausrichtung neu (Wiederverwendung der in `WallCommand` vorhandenen Umrechnungslogik) und regeneriert die Wand.
- Tests: Verlängerung bis zu einem expliziten Punkt sowie bis zum Schnittpunkt mit einer zweiten Wand liefert die erwartete neue Achse; nachträgliche Ausrichtungsänderung verschiebt die Achse um die erwartete Distanz.

###   Step 5: Automatisches Verschneiden beim Zeichnen (Snap-basiert) ergänzen
Wird beim Zeichnen einer neuen Wand ein Punkt in der Nähe einer bestehenden Wand gesetzt, wird automatisch eine Verbindung erzeugt.
- `WallCommand::on_point` in `src/modules/aec/commands.rs` erweitert um eine Prüfung gegen alle vorhandenen Wandachsen im Dokument innerhalb eines konfigurierbaren Snap-/Verschneidungs-Radius.
- Bei Treffer wird der geklickte Punkt auf die bestehende Wandachse gefangen und beim Finalisieren derselbe Join-Mechanismus (`join_wall_axes` aus Schritt 3) angewendet.
- Visuelles Feedback (Snap-Marker/Hervorhebung) für den erkannten Verschneidungspunkt während des Zeichnens.
- Tests: Punkt innerhalb des Radius löst automatische Verschneidung mit der erwarteten Wand aus; Punkt außerhalb des Radius verhält sich wie bisher (kein automatischer Join).

###   Step 6: Dynamische Längen-/Höhen-Attribute und Wand-Kontextmenü ergänzen
Länge und Höhe einer Wand können, soweit von der bestehenden Host-Infrastruktur unterstützt, direkt am Objekt editiert werden, und ein Rechtsklick auf Wände bietet die neuen Wandfunktionen an.
- Prüfung/Erweiterung der bestehenden dynamischen-Input-Infrastruktur (siehe `src/app/commands/mod.rs`) um ein Längenfeld für die aktuelle Wandachse im 2D-Viewport; Höhe im 3D-Viewport wird umgesetzt, sofern die Infrastruktur das ohne größeren Host-Umbau zulässt, andernfalls dokumentiert als bekannte Einschränkung.
- `src/app/view/overlay.rs::viewport_context_menu_overlay` erhält einen neuen bedingten Zweig (aktiv, wenn alle selektierten Handles über `wall_v2_from_entity` als Wände erkannt werden) mit Einträgen "Join", "Extend" und "Change Justification" (Untermenü Interior/Center/Exterior, analog zum bestehenden Draw-Order-Untermenü-Muster).
- Tests: Kontextmenü zeigt die neuen Einträge nur bei einer reinen Wand-Selektion, nicht bei gemischter oder Nicht-Wand-Selektion; dynamische Längeneingabe aktualisiert die Live-Vorschau/das Ergebnis korrekt.