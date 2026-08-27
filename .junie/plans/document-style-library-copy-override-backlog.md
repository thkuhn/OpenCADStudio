---
sessionId: session-260826-184400-h1pj
---

# Requirements

### Overview & Goals
Der Nutzer möchte **keine Implementierung jetzt**, sondern nur eine Ergänzung des bestehenden Plan-Dokuments `.junie/plans/aec-plan-view-display-variants.md`: Eine neue, spätere Ausbaustufe soll festgehalten werden, mit der Wand-/Materialstile explizit zwischen der **Projekt-Bibliothek** und der **Standard-(globalen)Bibliothek** kopiert werden können, in beide Richtungen, und optional dabei ein bereits vorhandener Zieleintrag überschrieben werden kann.

### Scope
**In Scope**
- Neuer Backlog-/Later-Stage-Eintrag im Plan-Dokument (analog zum bestehenden "✓ Step 7 (später/optional)"-Muster), der das Feature beschreibt: Stil/Material von Projekt → Standard-Bibliothek kopieren, von Standard-Bibliothek → Projekt kopieren, jeweils mit optionalem Überschreiben eines gleichnamigen/gleich-IDs Zieleintrags.
- Kurze Verknüpfung mit dem bereits behobenen ID-Kollisions-Bug (`unique_id`) und der bestehenden projektweiten Bibliotheks-Persistenz (Step 6), da beide für dieses Feature relevant sind.
- Kein Code wird geändert — reine Plan-Dokumentation.

**Out of Scope**
- Jegliche Implementierung (Datenmodell, Kopier-Logik, UI-Buttons) — ausdrücklich zurückgestellt.
- Auto-Sync/automatischer Abgleich zwischen Bibliotheken — nur explizites, manuelles Kopieren ist Teil dieser späteren Stufe.

### User Stories
- Als Anwender möchte ich einen in einem Projekt neu erstellten Wandstil in die globale Standard-Bibliothek übernehmen können, damit ich ihn in zukünftigen Projekten wiederverwenden kann.
- Als Anwender möchte ich einen bewährten Stil aus der Standard-Bibliothek gezielt in ein einzelnes Projekt kopieren können, ohne dass er automatisch mit der globalen Bibliothek synchron bleibt.
- Als Anwender möchte ich beim Kopieren wählen können, ob ein bereits vorhandener gleichnamiger Zieleintrag überschrieben oder das Kopieren abgebrochen/umbenannt wird, um Datenverlust zu vermeiden.

### Functional Requirements (für die spätere Umsetzung, nicht jetzt)
- Kopieren in beide Richtungen: Projekt → Standard, Standard → Projekt.
- Konfliktbehandlung: existiert im Ziel bereits ein Eintrag mit gleichem Namen/gleicher ID, muss der Nutzer explizit "Überschreiben" bestätigen; ohne Bestätigung wird nicht überschrieben.
- Das Feature betrifft sowohl Wandstile als auch Materialien (beide bereits als eigene Bibliothekstypen vorhanden).
- Muss mit der bereits eingeführten `unique_id`-ID-Vergabe (siehe Geklärte Diskussionspunkte) konsistent bleiben, damit keine erneute ID-Kollision entsteht.

# Technical Design

### Current Implementation (Kontext)
- Wandstil-/Material-Bibliotheken existieren aktuell in zwei Auflösungsebenen: projektweit (`ProjectFile::material_wall_style_library`, `src/modules/aec/engine/project.rs`) und global/Standard (`default_library_path()` / `load_or_seed()`, `src/modules/aec/engine/library.rs`), aufgelöst über `resolve_style_library(project)` ("Projekt überschreibt Global, sonst Fallback").
- Es gibt aktuell **keinen** Mechanismus, um einen Eintrag gezielt von der einen in die andere Bibliothek zu kopieren — die beiden Ebenen sind bislang rein hierarchisch (Fallback), nicht bidirektional kopierbar.
- IDs werden seit dem bereits umgesetzten Bugfix über `unique_id(prefix, name)` vergeben (`src/modules/aec/commands.rs`), was projektübergreifende ID-Kollisionen für **neu angelegte** Einträge verhindert — beim Kopieren eines bestehenden Eintrags muss diese Eindeutigkeit weiterhin beachtet werden (z. B. beim Umbenennen/Konflikt).

### Proposed Changes (nur Dokumentation, keine Implementierung jetzt)
- Im Kapitel "# Delivery Steps" wird ein neuer, klar als **später/optional** markierter Eintrag ergänzt (Nummerierung fortlaufend nach Step 8), der beschreibt:
  - Kopier-Aktion "Projekt → Standard-Bibliothek" für Wandstile/Materialien.
  - Kopier-Aktion "Standard-Bibliothek → Projekt" für Wandstile/Materialien.
  - Konflikt-/Überschreiben-Dialog bei Namens-/ID-Kollision im Zielbestand.
  - Verweis auf `resolve_style_library`/`unique_id` als bereits vorhandene Bausteine, auf die aufgesetzt wird.
- Im Kapitel "# Geklärte Diskussionspunkte" wird ein Satz ergänzt, der festhält: Feature wurde besprochen und bewusst als spätere Ausbaustufe zurückgestellt, nicht Teil der aktuellen Umsetzung.

### File Structure
- Geändert: `.junie/plans/aec-plan-view-display-variants.md` (einzige betroffene Datei).

# Delivery Steps

###   Step 1: Neuen Backlog-Eintrag "Stil-Bibliothek Kopieren & Überschreiben" im Delivery-Steps-Kapitel ergänzen
Das Plan-Dokument enthält einen neuen, klar als später/optional markierten Delivery-Step, der das bidirektionale Kopieren von Wandstilen/Materialien zwischen Projekt- und Standard-Bibliothek beschreibt.

- Neuen Abschnitt nach dem bestehenden "Step 8" in `.junie/plans/aec-plan-view-display-variants.md` einfügen, z.B. "### Step 9 (später/optional): Stil-/Material-Kopie zwischen Projekt- und Standard-Bibliothek".
- Beschreibung enthält: Kopieren Projekt→Standard, Kopieren Standard→Projekt, jeweils für Wandstile UND Materialien.
- Konfliktverhalten dokumentieren: bei existierendem Zieleintrag (gleicher Name/ID) muss der Nutzer das Überschreiben explizit bestätigen.
- Verweis auf bereits bestehende Bausteine (`resolve_style_library`, `unique_id`) als Grundlage für die spätere Umsetzung ergänzen.
- Ausdrücklich vermerken, dass dieser Step aktuell nicht umgesetzt wird (Reihenfolge/Markierung ohne ✓, analog zu unerledigten Steps).

###   Step 2: Zurückstellung im Abschnitt "Geklärte Diskussionspunkte" festhalten
Das Plan-Dokument dokumentiert explizit, dass das Kopier-Feature besprochen, aber bewusst auf später verschoben wurde.

- Neue Bullet-Zeile unter "# Geklärte Diskussionspunkte" in `.junie/plans/aec-plan-view-display-variants.md` ergänzen, im gleichen Stil wie die vorhandenen Einträge.
- Formulierung: Feature "Stile von Projekt- zu Standard-Bibliothek und umgekehrt kopieren, optional überschreiben" wurde diskutiert und ist als spätere Ausbaustufe (siehe neuer Step 9) festgehalten, nicht Teil der aktuellen Umsetzung.
- Keine Code-Änderungen in diesem Schritt — reine Dokumentationsergänzung, konsistent mit den bereits vorhandenen Bugfix-/Entscheidungs-Bullets.