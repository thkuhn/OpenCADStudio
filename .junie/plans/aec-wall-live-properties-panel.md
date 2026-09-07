---
sessionId: session-260812-011701-iuuy
---

# Requirements

### Overview & Goals
Beim Zeichnen einer Wand (`AEC_WALL`, `src/modules/aec/commands.rs::WallCommand`) werden Wandstil und Höhe aktuell ausschließlich über textbasierte Kommandozeilen-Prompts (`WallPhase::AskStyle`/`AskHeight`/`AskThickness`) abgefragt, *nachdem* die komplette Punktkette gezeichnet wurde. Die Stilauswahl listet dabei alle Wandstile als Klick-Buttons im Befehlseingabe-Fenster (`options()` → `CmdOption`) auf — bei vielen Wandstilen unpraktikabel (siehe frühere Sessions: genau deshalb wurde bereits der hierarchische, durchsuchbare **AEC Style Picker**-Modal-Dialog (`src/ui/window/aec_style_picker.rs`) für den Style-Manager und das Properties-Panel gebaut).

Ziel: Sobald `AEC_WALL` gestartet wird (bzw. spätestens nach dem ersten Klickpunkt), erscheint automatisch das Properties-Panel mit editierbaren Wand-Eigenschaften (Wandstil, Höhe) für die *gerade zu zeichnende* Wand — analog zu Autodesk AutoCAD Architecture, wo die Properties-Palette beim Aktivieren eines Wand-Werkzeugs sofort die aktuellen Default-Eigenschaften des Werkzeugs zeigt und der Wandstil über einen durchsuchbaren/hierarchischen Style-Browser statt einer Dropdown-/Button-Liste gewählt wird.

### Scope
**In Scope:**
- Automatisches Öffnen/Umschalten des Properties-Panels auf eine neue "Wall (drawing)"-Sektion, sobald `AEC_WALL` aktiv wird (spätestens ab dem ersten Punkt).
- Editierbare Felder in dieser Sektion: aktueller Wandstil (als Button/Anzeige, öffnet den bestehenden **AEC Style Picker**-Modal statt einer Text-Eingabe/Button-Liste) und Höhe (Zahlenfeld).
- Auswahl im Style Picker schreibt sofort in die laufende `WallCommand`-Instanz (nicht in eine XDATA, da noch keine fertige Wand-Entity existiert) und aktualisiert die Live-Vorschau der gezeichneten Wand entsprechend.
- Die bisherigen Kommandozeilen-Prompts `AskStyle`/`AskHeight`/`AskThickness` entfallen für den normalen (Maus-/Panel-)Workflow; ein rein tastaturbasierter Fallback (Eingabe von Höhe/Stilname direkt in der Kommandozeile) bleibt bestehen, für Nutzer, die weiterhin ohne Panel arbeiten.
- Der zuletzt verwendete Wandstil und die zuletzt verwendete Höhe werden als Sitzungs-Default für den nächsten `AEC_WALL`-Aufruf vorbelegt (wie AutoCAD Architectures "aktueller Stil").

**Out of Scope:**
- Änderungen an der Stil-/Material-Datenmodell-Logik selbst (`StyleLibrary`, `effective_layers`, `resolve_chain`).
- Neue Fähigkeiten des AEC Style Picker Modals über die bereits vorhandene Baum-/Such-/Vorschau-Funktionalität hinaus.
- Analoge "Live-Properties-Panel beim Zeichnen"-Unterstützung für andere Zeichenkommandos (Fenster/Türen existieren noch nicht) — der neue Mechanismus wird aber so entworfen, dass er dafür wiederverwendbar ist.

### User Stories
- Als Planer möchte ich beim Aufruf von "Wand zeichnen" sofort die Wand-Eigenschaften (Stil, Höhe) im Properties-Panel sehen, damit ich sie vor oder während des Zeichnens anpassen kann, ohne die Kommandozeile zu benutzen.
- Als Nutzer mit vielen Wandstilen möchte ich den gewünschten Stil über denselben durchsuchbaren/hierarchischen Dialog wählen, den ich bereits aus dem Style Manager kenne, statt eine lange Button-Liste im Befehlseingabe-Fenster durchsuchen zu müssen.
- Als Nutzer möchte ich, dass die zuletzt gewählten Wand-Eigenschaften beim nächsten Wandzeichnen als Vorschlag erscheinen, damit ich nicht bei jeder Wand von vorne wählen muss.

### Functional Requirements
- Beim Start von `AEC_WALL` (vor dem ersten Klick) wird das Properties-Panel auf eine "Wall"-Sektion mit Stil-Button (aktueller Stilname + "Ändern...") und Höhen-Eingabefeld umgeschaltet.
- Klick auf den Stil-Button öffnet den bestehenden `ModalKind::AecStylePicker`-Dialog mit einem neuen Ziel-Kontext für "laufendes Zeichenkommando" statt einer Wand-Entity.
- Bestätigte Auswahl im Picker aktualisiert `WallCommand`s internen Stil/Layer-Zustand und die Live-Vorschau-Geometrie sofort (Kontur-Dicke ändert sich ggf. je nach gewähltem Stil).
- Änderung des Höhenfeldes aktualisiert `WallCommand.wall.height` sofort, ohne dass die Punktkette unterbrochen wird.
- Beim Abschluss der Punktkette (Enter/Escape) wird die Wand mit den im Panel eingestellten Werten sofort finalisiert — kein zusätzlicher Kommandozeilen-Umweg über `AskStyle`/`AskHeight`/`AskThickness` mehr nötig, sofern das Panel genutzt wurde.
- Schließt der Nutzer `AEC_WALL` ab, werden Stil-ID und Höhe als Sitzungs-Default für den nächsten Aufruf von `WallCommand::new()` übernommen.

### Non-Functional Requirements
- Der neue Mechanismus ("Properties-Panel zeigt Live-Eigenschaften eines aktiven Zeichenkommandos") wird als generische Erweiterung des `CadCommand`-Traits entworfen, nicht als AEC-spezifischer Sonderfall, damit er später für weitere interaktive Kommandos wiederverwendbar ist.
- Bestehende Tests (`cargo test --lib aec`, `polyline`, `properties`) bleiben grün; neue Tests decken die Stil-/Höhen-Übernahme in die laufende `WallCommand` sowie die Session-Default-Übernahme ab.

# Technical Design

### Current Implementation
- `src/command.rs::CadCommand` ist das zentrale Trait aller interaktiven Zeichenkommandos (`on_point`, `on_enter`, `options()` für Kommandozeilen-Buttons, `set_ctrl`/`set_shift` für Modifikator-Zustand) — es gibt aktuell **keinen** Hook, über den ein Kommando eigene, live editierbare Eigenschaften im Properties-Panel anzeigen könnte; das Properties-Panel (`src/app/properties.rs`) rendert ausschließlich Eigenschaften der aktuell **selektierten Entities**, nicht eines laufenden Kommandos.
- `src/modules/aec/commands.rs::WallCommand`: Phasen `Drawing → AskStyle → AskHeight → AskThickness` laufen rein über Kommandozeilen-Prompts (`prompt()`/`options()`); `options()` bei `AskStyle` erzeugt für **jeden** Wandstil einen eigenen `CmdOption`-Button (Zeile ~1064-1070) — das ist die konkrete Stelle, die bei vielen Stilen unpraktikabel wird. `build_entity()` (Zeile ~932) baut die Live-Vorschau-Geometrie aus `self.wall`/`self.style_id`/`self.resolved_layers`/`self.justification`; `sync_live()` (Zeile ~993) sendet `CmdResult::UpdateLiveEntity`/`CommitLiveEntity` an den Host.
- `src/ui/window/aec_style_picker.rs` + `ModalKind::AecStylePicker { target: StylePickerTarget }` (`src/app/mod.rs`) ist der bereits produktiv genutzte, hierarchische/durchsuchbare Stil-Auswahl-Dialog (Baum via `StyleLibrary::wall_style_tree()`, Live-Suche, Schichtaufbau-Vorschau) — aktuell mit Zielen `WallStyleParent`, `LayerMaterial(usize)`, `LayerOverride(usize)`, `WallPropertiesStyle` (siehe jüngste Session-Änderung: `WallPropertiesStyle` trägt keine Handle mehr direkt, sondern `App::aec_style_picker_wall_handles: Vec<Handle>` wird separat gesetzt, um Mehrfachauswahl-Bearbeitung im Properties-Panel zu ermöglichen) — dieses exakte Muster (Ziel-Enum + separates `App`-Feld für den Schreib-Kontext) ist die Vorlage für ein neues Ziel "aktives Zeichenkommando".
- `src/app/update/command.rs`/`src/app/command_driver.rs`: Host-seitige Schleife, die `CadCommand`-Instanzen hält (`self.active_command: Option<Box<dyn CadCommand>>` o. ä.) und Viewport-/Kommandozeilen-Events an sie weiterleitet; Kommandostart erfolgt in `src/app/commands/draw.rs` (Zeile ~767, `"AEC_WALL" => { ... }`).
- `src/app/properties.rs::wall_prop_section` (aus jüngster Session) zeigt bereits Stil (als `PropValue::Picker`) und Höhe (editierbar) für eine **existierende** `WALL_V2`-Wand — dasselbe visuelle Muster (Stil-Button öffnet Picker, Höhe editierbar) soll für die *im Entstehen begriffene* Wand während `AEC_WALL` wiederverwendet werden, nur mit anderem Schreibziel (laufendes Kommando statt XDATA einer fertigen Entity).

### Key Decisions
- **Generischer `CadCommand`-Hook statt AEC-Spezialcode im Host**: Neue optionale Trait-Methode(n) auf `CadCommand` (Default: `None`/no-op), über die ein Kommando eine Liste editierbarer "Live-Properties" (Label, aktueller Wert, Feldtyp: Text/Zahl/Picker-Button) zurückgibt, plus eine Methode, mit der der Host eine Änderung an einem benannten Feld zurück ins Kommando schreibt. Das hält die Architektur erweiterbar für zukünftige Kommandos (Fenster/Türen) statt einer Wand-spezifischen Ad-hoc-Lösung im `App`-Update-Code.
- **Wiederverwendung des bestehenden AEC Style Pickers statt eines neuen Dialogs**: Es wird kein neues UI-Widget erfunden — `ModalKind::AecStylePicker` bekommt lediglich ein zusätzliches `StylePickerTarget`-Mitglied (z. B. `ActiveCommand`), dessen Bestätigung statt XDATA zu schreiben eine neue `Message` an das laufende Kommando über den neuen generischen Hook weiterreicht.
- **Kommandozeilen-Fallback bleibt erhalten, aber nachrangig**: `AskStyle`/`AskHeight`/`AskThickness` werden nicht ersatzlos gestrichen (reiner Tastatur-Workflow ohne Maus/Panel muss weiter funktionierbar bleiben), aber sie werden nur noch durchlaufen, wenn der Nutzer die Panel-Felder nicht angefasst hat, bzw. das Panel wird zur primären, bevorzugten Eingabemethode.
- **Session-Default statt Persistenz in der Bibliotheksdatei**: zuletzt genutzter Stil/Höhe werden nur als In-Memory-Default (`App`-Feld, analog zu bereits bestehenden Session-State-Mustern wie `aec_style_manager_wall_style_sort`) gehalten, nicht in der `StyleLibrary`-Datei gespeichert — das entspricht dem Verhalten "aktueller Zeichenstil" in AutoCAD, das ebenfalls nur pro Sitzung/Zeichnung gilt.

### Proposed Changes
1. **`CadCommand`-Trait-Erweiterung** (`src/command.rs`): neue Default-Methoden `fn live_properties(&self) -> Option<LiveCommandProperties>` (liefert z. B. `Vec<LiveCommandField>` mit `{ label, field_id, value: LiveFieldValue::{Text(String), Number(f64), Picker(String)} }`) und `fn apply_live_property(&mut self, field_id: &str, value: LiveFieldValue)`. `WallCommand` implementiert beide, alle anderen Kommandos nutzen den Default (`None`/no-op).
2. **Host-Anbindung**: `src/app/properties.rs`/`src/app/view/`-Rendering-Pfad prüft, ob ein `active_command` gesetzt ist und `live_properties()` `Some` liefert; falls ja, rendert es diese Sektion **anstelle** der normalen Selektions-Properties (analog zum bestehenden `wall_prop_section`-Rendering-Muster, aber gespeist aus dem Kommando statt aus einer Entity).
3. **Neues Picker-Ziel** `StylePickerTarget::ActiveCommand` (`src/app/mod.rs`) + neue `Message` (z. B. `AecStylePickerOpenForActiveCommand`), analog zum bestehenden `AecStylePickerOpenForWallProperties`-Muster; Bestätigung ruft `active_command.apply_live_property("wall_style", LiveFieldValue::Picker(style_id))` auf statt XDATA zu schreiben.
4. **`WallCommand`-Anpassung** (`src/modules/aec/commands.rs`): `live_properties()` liefert Stilname (aktuelle `self.style_id`/`self.resolved_layers`) und Höhe (`self.wall.height`); `apply_live_property("wall_style", ...)` löst `effective_layers()` für den gewählten Stil auf und setzt `self.style_id`/`self.resolved_layers`, `apply_live_property("wall_height", ...)` setzt `self.wall.height`; nach jeder Änderung wird intern `sync_live()` erneut aufgerufen, damit die Live-Vorschau sofort reagiert.
5. **Kommandostart-Trigger**: in `src/app/commands/draw.rs`s `"AEC_WALL"`-Zweig wird nach dem Setzen von `active_command` das Properties-Panel aktiv geschaltet (z. B. `self.properties_panel_mode = PropertiesPanelMode::ActiveCommand` o. ä., falls ein solcher Anzeige-Modus-Schalter noch nicht existiert, wird er als kleines neues Enum-Feld auf `App` ergänzt).
6. **Session-Defaults**: neue `App`-Felder (z. B. `aec_last_wall_style_id: Option<String>`, `aec_last_wall_height: Option<f64>`), gesetzt beim Finalisieren einer Wand, gelesen in `WallCommand::new()`/`new_with_library()` als Startwert statt der fest kodierten `DEFAULT_WALL_HEIGHT`/keinem Stil.
7. **Vereinfachung/Rückbau der `AskStyle`-Button-Liste**: `options()` für `WallPhase::AskStyle` bleibt als reiner Tastatur-Fallback bestehen, wird aber nicht mehr die primäre Interaktion, sobald das Panel genutzt wurde (Phase wird übersprungen, wenn `self.style_id` bereits über das Panel gesetzt wurde, analog zur bestehenden Logik, die bei leerer Bibliothek direkt zu `AskHeight` springt).

### Data Models / Contracts
```rust
// command.rs — new generic hook, default no-op
pub enum LiveFieldValue { Text(String), Number(f64), Picker(String) }
pub struct LiveCommandField { pub label: String, pub field_id: &'static str, pub value: LiveFieldValue }
pub struct LiveCommandProperties { pub title: String, pub fields: Vec<LiveCommandField> }

pub trait CadCommand: Send {
    // ... existing methods ...
    fn live_properties(&self) -> Option<LiveCommandProperties> { None }
    fn apply_live_property(&mut self, _field_id: &str, _value: LiveFieldValue) {}
}
```

### Components
- `src/command.rs` (geändert): neue Trait-Default-Methoden + `LiveCommandProperties`/`LiveFieldValue`-Typen.
- `src/modules/aec/commands.rs` (geändert): `WallCommand` implementiert die neuen Methoden; Session-Default-Übernahme beim Finalisieren.
- `src/app/mod.rs` (geändert): neues `StylePickerTarget::ActiveCommand`, neue Session-Default-Felder, neuer Properties-Panel-Anzeige-Modus falls nötig.
- `src/app/properties.rs`/View-Rendering (geändert): rendert `live_properties()` des aktiven Kommandos anstelle der Selektions-Properties, solange ein Kommando aktiv ist und welche liefert.
- `src/ui/window/aec_style_picker.rs` (geändert): zusätzlicher Zweig für `ActiveCommand`-Ziel (visuell identisch zu den bestehenden Zielen).
- `src/app/commands/draw.rs` (geändert): Kommandostart schaltet das Properties-Panel auf den neuen Live-Modus um.

### Risks
- **Konflikt mit normaler Selektions-Properties-Anzeige**: während ein Kommando läuft, darf die normale Entity-Properties-Anzeige nicht gleichzeitig aktiv sein/überschrieben werden — sauberer Zustandswechsel beim Kommandostart/-ende nötig (Rückkehr zur normalen Anzeige nach Abschluss/Abbruch von `AEC_WALL`).
- **Generizität des Hooks**: der neue `CadCommand`-Hook muss so gestaltet sein, dass er nicht AEC-spezifisch fest verdrahtet wirkt, sonst entsteht technische Schuld für zukünftige Kommandos — Umsetzung sollte in Review besonders auf diese Generalität achten.
- **Rückwärtskompatibilität des Kommandozeilen-Workflows**: reine Tastaturnutzer dürfen durch die neue Panel-Priorität nicht ausgeschlossen werden — `AskStyle`/`AskHeight`/`AskThickness` müssen als Fallback vollständig funktionsfähig bleiben.

# Delivery Steps

### ✓ Step 1: Generischen Live-Properties-Hook im CadCommand-Trait einführen
Interaktive Kommandos können optional editierbare Live-Eigenschaften deklarieren, ohne dass bestehende Kommandos betroffen sind.
- Neue Typen `LiveFieldValue`, `LiveCommandField`, `LiveCommandProperties` in `src/command.rs`.
- Neue Default-Methoden `live_properties(&self) -> Option<LiveCommandProperties>` und `apply_live_property(&mut self, field_id, value)` auf `CadCommand` (Default: `None`/no-op), damit alle bestehenden Kommandos unverändert kompilieren.
- Unit-Test: ein Dummy-Testkommando ohne Override liefert `None`/ignoriert `apply_live_property` klaglos.

### ✓ Step 2: Properties-Panel rendert Live-Eigenschaften des aktiven Kommandos
Sobald ein Kommando läuft und Live-Eigenschaften liefert, zeigt das Properties-Panel diese anstelle der normalen Selektionsanzeige.
- Neuer Anzeige-Zustand in `App` (z.B. Enum-Feld), der beim Start/Ende eines Kommandos gesetzt/zurückgesetzt wird.
- Rendering-Pfad in `src/app/properties.rs`/View-Code prüft diesen Zustand und rendert `live_properties()`-Felder (Text/Zahl editierbar, Picker als Button) statt der Entity-Properties.
- Änderungen an Text-/Zahlenfeldern rufen `apply_live_property` auf dem aktiven Kommando auf.
- Nach Abschluss/Abbruch des Kommandos kehrt das Panel zur normalen Selektionsanzeige zurück.

### ✓ Step 3: WallCommand implementiert Live-Properties für Stil und Höhe
AEC_WALL zeigt sofort beim Start ein Wand-Panel mit Stilname und Höhe, editierbar während des Zeichnens.
- `WallCommand::live_properties()` liefert Stilname (aufgelöst über die geladene `StyleLibrary`) und aktuelle Höhe.
- `WallCommand::apply_live_property("wall_height", ...)` aktualisiert `self.wall.height` und triggert `sync_live()` für sofortige Vorschau-Aktualisierung.
- `WallCommand::apply_live_property("wall_style", ...)` löst `effective_layers()` für den neuen Stil auf, setzt `self.style_id`/`self.resolved_layers`, triggert `sync_live()`.
- Kommandostart in `src/app/commands/draw.rs`s `AEC_WALL`-Zweig schaltet das Properties-Panel auf den neuen Live-Modus.
- Tests: Höhenänderung während der Punktkette ändert die Live-Vorschau-Geometrie korrekt; Stiländerung ändert die resultierende Gesamtdicke korrekt.

### ✓ Step 4: AEC Style Picker um Ziel für das laufende Zeichenkommando erweitern
Der Stil-Button im Wand-Live-Panel öffnet denselben hierarchischen, durchsuchbaren Style-Picker-Dialog wie im Style Manager, statt einer Button-Liste im Befehlseingabe-Fenster.
- Neues `StylePickerTarget::ActiveCommand`-Mitglied in `src/app/mod.rs`, analog zum bestehenden `WallPropertiesStyle`-Muster.
- Neue `Message` zum Öffnen des Pickers für das aktive Kommando; Bestätigung ruft `apply_live_property("wall_style", ...)` auf dem aktiven Kommando auf statt XDATA zu schreiben.
- `src/ui/window/aec_style_picker.rs` behandelt das neue Ziel visuell identisch zu den bestehenden Wandstil-Zielen (Baum, Suche, Schichtaufbau-Vorschau).
- Tests: simulierte Bestätigung im Picker für `ActiveCommand`-Ziel aktualisiert den erwarteten Feldwert des Test-Kommandos korrekt.

### ✓ Step 5: Kommandozeilen-Fallback anpassen und Sitzungs-Defaults für Stil/Höhe ergänzen
Reine Tastaturnutzung bleibt möglich, und die zuletzt verwendeten Wand-Eigenschaften werden beim nächsten Wandzeichnen vorbelegt.
- `WallPhase::AskStyle`/`AskHeight`/`AskThickness` werden übersprungen, sobald der Nutzer die entsprechenden Werte bereits über das Live-Panel gesetzt hat; als reiner Tastatur-Fallback bleiben sie für Nutzer ohne Panel-Interaktion vollständig erhalten.
- Neue `App`-Felder `aec_last_wall_style_id`/`aec_last_wall_height`, geschrieben beim erfolgreichen Finalisieren einer Wand, gelesen als Startwert in `WallCommand::new()`.
- Tests: zweiter `AEC_WALL`-Aufruf nach einem finalisierten Wandzeichnen startet mit dem zuvor verwendeten Stil/Höhe vorbelegt; ein Ablauf rein über Kommandozeilen-Prompts (ohne Panel-Interaktion) funktioniert weiterhin unverändert.