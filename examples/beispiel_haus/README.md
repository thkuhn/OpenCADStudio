# OpenCADStudio AEC — Beispielprojekt: Einfamilienhaus

Dieses Beispielprojekt demonstriert das Zusammenspiel von **Multi-Geschoss-Modellen über XREFs**, **AEC-Bauteilen (Wände, Decken, Öffnungen, Räume)** und den **modellassoziierten 2D-Schnitt-, Ansichts- und Grundriss-Ableitungen nach DIN 1356** für Genehmigungs- und Ausführungsplanung.

---

## Projektstruktur und Geschosshierarchie

Das Gebäude ist als typisches deutsches **Einfamilienhaus (EFH)** mit 4 Geschossebenen konzipiert:

* `Projekt_Einfamilienhaus.ocsproj`:
  Die zentrale Projektdatei für den OpenCADStudio Geschoss- und Projekt-Manager. Definiert die Geschosshöhen:
  * **UG (Untergeschoss / Keller):** $Z = -2{,}80\,\text{m}$ (lichte Raumhöhe $2{,}40\,\text{m}$)
  * **EG (Erdgeschoss):** $Z = 0{,}00\,\text{m}$ (lichte Raumhöhe $2{,}60\,\text{m}$)
  * **OG (Obergeschoss):** $Z = 2{,}80\,\text{m}$ (lichte Raumhöhe $2{,}60\,\text{m}$)
  * **DG (Dachgeschoss / Studio):** $Z = 5{,}60\,\text{m}$ (Kniestock $1{,}20\,\text{m}$, Firsthöhe $9{,}10\,\text{m}$)
  Enthält vorkonfigurierte Bibliotheken für **Materialien**, **Wandstile**, **Deckenstile**, **Öffnungsstile** und **Planarten (DisplayConfigs)**.

* `UG.dwg` / `UG.dxf`:
  Untergeschoss mit WU-Beton-Bodenplatte, gedämmten WU-Beton-Kelleraußenwänden (Bitumenabdichtung, Perimeterdämmung), tragender Mittelwand, Kellerfenstern (Lichtschächte), T30-Türen und DIN 277 Räumen (Technik/HTR, Vorrat/Lager, Hobbyraum).
* `EG.dwg` / `EG.dxf`:
  Erdgeschoss mit wärmegedämmten Außenwänden (WDVS 14cm, Kalksandstein 17.5cm), Stahlbetondecke, Haustür, Terrassen-Hebeschiebetür zum Garten, Fenstern, Innentüren und Räumen (Diele/Windfang, Gäste-WC, Küche, Wohnen & Essen).
* `OG.dwg` / `OG.dxf`:
  Obergeschoss mit Geschossdecke, Schlafräumen (Eltern, Kind 1, Kind 2), Familienbad, Flur und Fenstern.
* `DG.dwg` / `DG.dxf`:
  Dachgeschoss mit Holzbalkendecke, Kniestock-/Drempelwänden, Giebelwänden, Giebelfenstern und Studio/Galerie.
* `Haus_Gesamt.dwg` / `Haus_Gesamt.dxf`:
  Das aggregierte Master-Gesamtmodell. Es bindet die Einzelgeschosse `UG`, `EG`, `OG` und `DG` als externe Referenzen (XREFs) mit ihren jeweiligen Höhenversätzen ($Z = -2{,}80\,\text{m}$, $Z = 0{,}00\,\text{m}$, $Z = 2{,}80\,\text{m}$, $Z = 5{,}60\,\text{m}$) ein.
  Enthält bereits vordefinierte abgeleitete Zeichnungsträger (`DerivedViewCarrier`) und DIN 1356 Markierungen:
  * **Schnitt A–A (Ausführungsplanung 1:50):** Mehrgeschossiger Längsschnitt durch alle 4 Geschosse mit Einzelschichten, DIN-Materialschraffuren und assoziativer DIN 1356 Schnittmarkierung.
  * **Schnitt B–B (Genehmigungsplanung 1:100):** Mehrgeschossiger Querschnitt durch Treppen- und Wohnbereich.
  * **Südfassade & Nordfassade (Ansichten 1:100):** Fassadenansichten mit Öffnungsprofilen, Fensterbänken, DIN-Flügelaufschlägen und Ansichtsmarkierungen.
  * **EG Grundriss (Ausführung 1:50 & Genehmigung 1:100):** Assoziative Grundrissableitungen für Werk- und Bauantragspläne.

---

## Planarten und Darstellungskonfigurationen

Die Projektdatei stellt getrennte Konfigurationen für die verschiedenen Leistungsphasen bereit:

1. **Genehmigungsplanung (1:100):**
   * Reduktion auf Tragschichten (`LayerDetail::StructuralOnly`) oder Gesamtkonturen.
   * Normgerechte Öffnungsbeschriftungen nach DIN 1356 ($B/H, BRH$).
   * Raumstempel mit DIN 277 Nutzungsarten und Flächennachweisen.
2. **Ausführungsplanung / Werkplan (1:50):**
   * Volle Schichtdarstellung aller Wände und Deckenaufbauten (`LayerDetail::All`).
   * Materialspezifische DIN-Schraffuren (Mauerwerk, Stahlbeton, Dämmung, Trockenbau, Estrich).
   * Detaillierte Öffnungsdarstellung mit Rahmen, Flügeln, Glasebenen, Schwellen und Fensterbänken.
3. **Statik / Rohbau (1:100 / 1:50):**
   * Fokus auf tragende Bauteile und Durchbruchssymbole.
4. **Deckenspiegel (RCP 1:50):**
   * Deckenuntersichten und Aussparungen.

---

## Interaktive Griffe & Assoziative Modellbearbeitung

1. **DIN 1356 Schnittmarkierungen im Grundriss:**
   * **Start- und Endgriff (Quadrat):** Verschieben oder Drehen der Schnittlinie. Der assoziierte Schnittträger wird bei Fertigstellung automatisch synchron regeneriert.
   * **Tiefengriff (Rechteck):** Blicktiefe interaktiv per Maus vergrößern oder verkleinern.
   * **Flip-Griff (Dreieck):** Ein Klick kehrt die Blickrichtung der Schnittpfeile und des Schnittträgers normgerecht um.

2. **Semantische Modellgriffe im Schnitt (`resolve_handle_drag`):**
   * Beim Anklicken abgeleiteter Kanten im Schnitt (z. B. Wandoberkanten, Deckenoberkanten, Fensterbrüstungen oder Stürzen) erscheinen interaktive Modellgriffe.
   * Das Ziehen an einem Wandoberkantengriff hebt oder senkt die 3D-Wandhöhe im BIM-Modell.
   * Beim Loslassen des Griffs wird eine atomare BIM-Transaktion ausgeführt: Die Wandhöhe wird im Modell aktualisiert und alle abhängigen Ansichten werden synchron neu berechnet.

3. **Eigenschaften-Panel & Synchronisation:**
   * Alle Parameter (Kennung, Blicktiefe, Z-Bereich, Schnitthöhe, Maßstab 1:50/1:100 und Aktualisierungsmodus) können im Eigenschaften-Panel angepasst werden.
   * Mit dem Befehl `AEC_VIEWUPDATE` können alle Ansichten auf Tastendruck auf den neuesten Modellstand gebracht werden.
