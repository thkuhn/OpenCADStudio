# OpenCADStudio AEC — Beispielprojekt: Einfamilienhaus

Dieses Beispielprojekt demonstriert das Zusammenspiel von **Multi-Geschoss-Modellen über XREFs**, **AEC-Bauteilen (Wände, Decken, Öffnungen)** und den neuen **modellassoziierten 2D-Schnitt-, Ansichts- und Grundriss-Funktionen nach DIN 1356**.

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
  Enthält bereits vordefinierte abgeleitete Zeichnungsträger (`DerivedViewCarrier`) und DIN 1356 Markierungen:
  * **Schnitt A–A:** Mehrgeschossiger Gebäudeschnitt mit Materialschraffuren, Deckenauflagern und assoziativer DIN 1356 Schnittmarkierung.
  * **Südfassade:** Fassadenansicht mit Öffnungsprofilen, DIN-Flügelaufschlägen und Ansichtsmarkierung.
  * **EG Grundriss:** Assoziativer Horizontalschnitt mit lokalisierter Verortung und Türöffnungssymbolen.

---

## Interaktive Griffe & Assoziative Modellbearbeitung

1. **DIN 1356 Schnittmarkierungen im Grundriss:**
   * **Start- und Endgriff (Quadrat):** Verschieben oder Drehen der Schnittlinie. Der assoziierte Schnittträger wird bei Fertigstellung automatisch synchron regeneriert.
   * **Tiefengriff (Rechteck):** Blicktiefe interaktiv per Maus vergrößern oder verkleinern.
   * **Flip-Griff (Dreieck):** Ein Klick kehrt die Blickrichtung der Schnittpfeile und des Schnittträgers normgerecht um.
   * **Jog-Punkte:** Bei abgetreppten Schnittführungen können Zwischensegmente individuell verschoben werden.

2. **Semantische Modellgriffe im Schnitt (`resolve_handle_drag`):**
   * Beim Anklicken abgeleiteter Kanten im Schnitt (z. B. Wandoberkanten, Deckenoberkanten, Fensterbrüstungen oder Stürzen) erscheinen interaktive Modellgriffe.
   * Das Ziehen an einem Wandoberkantengriff hebt oder senkt die 3D-Wandhöhe im BIM-Modell.
   * Beim Loslassen des Griffs wird eine atomare BIM-Transaktion ausgeführt: Die Wandhöhe wird im Modell aktualisiert und alle abhängigen Ansichten werden synchron neu berechnet.

3. **Lokalisierte Grundriss-Engine (`AEC_FLOORPLAN` / `AEC_RCP`):**
   * Grundrisse werden relativ zum Modellursprung projiziert und erscheinen exakt am gewählten Klickpunkt.
   * **Einfügegriff:** Verschieben der gesamten platzierten 2D-Zeichnung im Modellbereich.
   * **Schnitthöhen-Griff:** Interaktive Justierung der horizontalen Schnittebene (`z_cut`) direkt am Plan oder über das Eigenschaften-Panel (`carrier_cut_height`).

4. **Eigenschaften-Panel & Synchronisation:**
   * Alle Parameter (Kennung, Blicktiefe, Z-Bereich, Schnitthöhe, Maßstab 1:50/1:100 und Aktualisierungsmodus) können im Eigenschaften-Panel angepasst werden.
   * Mit dem Befehl `AEC_VIEWUPDATE` können alle manuell verwalteten Ansichten auf Tastendruck auf den neuesten Modellstand gebracht werden.
