# OpenCADStudio AEC — Beispielprojekt: Einfamilienhaus

Dieses Beispielprojekt demonstriert das Zusammenspiel von **Multi-Geschoss-Modellen über XREFs**, **AEC-Bauteilen (Wände, Decken, Öffnungen)** und den neuen **modellassoziierten 2D-Schnitt- und Ansichtsfunktionen**.

---

## Projektstruktur

* `Projekt_Einfamilienhaus.ocsproj`:
  Die zentrale Projektdatei für den OpenCADStudio Geschossmanager. Definiert die Geschosshöhen (EG $0{,}00\,\text{m}$, OG $2{,}80\,\text{m}$, DG $5{,}60\,\text{m}$) und verknüpft die Zeichnungsdateien.
* `EG.dwg` / `EG.dxf`:
  Erdgeschoss-Grundriss mit mehrschichtigen Außenwänden (Wärmedämmung, Kalksandstein), tragenden Innenwänden, Fenstern mit Brüstungen und Türen.
* `OG.dwg` / `OG.dxf`:
  Obergeschoss-Grundriss mit Geschossdecke, Schlafräumen, Bad und Fenstern.
* `DG.dwg` / `DG.dxf`:
  Dachgeschoss mit Kniestock-/Drempelwänden und Giebelwänden.
* `Haus_Gesamt.dwg` / `Haus_Gesamt.dxf`:
  Das aggregierte Master-Modell. Es bindet die Einzelgeschosse `EG`, `OG` und `DG` als externe Referenzen (XREFs) mit ihren jeweiligen Höhenversätzen ($Z = 0{,}00\,\text{m}$, $Z = 2{,}80\,\text{m}$, $Z = 5{,}60\,\text{m}$) ein.
  Enthält bereits vordefinierte abgeleitete Zeichnungsträger:
  * **Schnitt A–A:** Mehrgeschossiger Gebäudeschnitt mit Materialschraffuren und Deckenauflagern.
  * **Südfassade:** Fassadenansicht mit Öffnungsprofilen und DIN-Flügelaufschlägen.

---

## Befehle & Arbeitsablauf

1. **Projekt öffnen:**
   * Öffnen Sie `Projekt_Einfamilienhaus.ocsproj` im Projekt-Explorer oder öffnen Sie `Haus_Gesamt.dwg` direkt.
2. **Neuen Schnitt erzeugen (`AEC_SECTION`):**
   * Befehl `AEC_SECTION` in der Befehlszeile eingeben oder in der Ribbon-Gruppe **"Views"** auf **"Section"** klicken.
   * Start- und Endpunkt der Schnittlinie durch das Gebäude wählen (z. B. quer oder gestuft).
   * Blicktiefe und Einfügepunkt für den 2D-Zeichnungsträger im Modellbereich anklicken.
3. **Ansicht erstellen (`AEC_ELEVATION`):**
   * Befehl `AEC_ELEVATION` aufrufen.
   * Fassadenlinie und Blicktiefe angeben.
4. **Ansichten aktualisieren (`AEC_VIEWUPDATE`):**
   * Nach Änderungen an Wänden oder Fenstern in den Geschossen aktualisiert `AEC_VIEWUPDATE` alle veralteten Schnitte synchron.
5. **Layout-Platzierung (`AEC_PLACE_VIEW`):**
   * Im Papierbereich platzieren mit automatischer Ansichtsfenstererstellung (`MVIEW`) und Maßstabsbindung (1:50 / 1:100).
