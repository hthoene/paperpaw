# Interlinear: eine Übersetzungszeile unter jedem Satz

*English: [README.md](README.md).*

Ein Beispiel-Plugin mit Code für PaperPaw, Plugin-Schnittstelle 7. Unter
jedem Satz eines Buchs zeigt es seine Übersetzung in kleiner Kursivschrift,
Zeile für Zeile unter dem Satz, oder mit der Einstellung *Übersetzung nach
dem Absatz* alle Übersetzungen eines Absatzes gesammelt nach ihm. Die
Übersetzungen kommen aus einer Textdatei neben dem Buch.

## Benutzen

1. Bauen und auf die Karte oder in die Bibliothek des Emulators kopieren
   (siehe [CODE.md der Vorlage](../../templates/plugin-source/CODE.md):
   `paperpaw-dev build`, `paperpaw-dev build --device emulator`).
2. Die Übersetzungsdatei des Buchs in den Datenordner des Plugins legen,
   `.paperpaw/plugins/interlinear/` auf der Karte, benannt wie das Buch ohne
   Endung und mit `.txt`: zu `alice-in-wonderland.epub` gehört
   `alice-in-wonderland.txt`. [`example/alice-in-wonderland.txt`](example/alice-in-wonderland.txt)
   übersetzt die ersten beiden Absätze des Beispielbuchs ins Deutsche.
3. Das Buch öffnen. Unter *Einstellungen → Lesehilfen* stehen
   *Übersetzungszeilen* (an) und *Übersetzung nach dem Absatz* (aus).

Die Datei hat eine Zeile je Satz: den Satz, wie das Buch ihn hat, einen
Tabulator, seine Übersetzung, in UTF-8. Zeilen, die mit `#` beginnen, sind
Kommentare. Ein Satz wird an seinen ersten 40 Buchstaben und Ziffern
erkannt; Anführungszeichen, Leerzeichen und Groß- und Kleinschreibung müssen
also nicht mit dem Buch übereinstimmen. Genutzt werden höchstens 2.048 Sätze
einer Datei und Übersetzungen bis 1.024 Bytes; ein langer Absatz zeigt die
Übersetzungen, die in eine Antwort passen (3.840 Bytes).

## Wie es funktioniert

Das Plugin abonniert die Text-Stage des Readers, `reader.text`
(`docs/abi/page-layers.md`, SDK `paperpaw_sdk::native::page_layers`):

- Wenn ein Buch aufgeht, fragt der Reader, ob das Plugin etwas zu ihm
  beiträgt (`TextAsk::Book`). Das Plugin sucht die Datei des Buchs, liest sie
  einmal in einen Index von 8 Bytes je Satz und antwortet mit
  `TextAnswer::uses` und einer Signatur aus seinen Einstellungen und der
  Datei. Ohne Datei antwortet es nichts, und der Reader fragt es zum Text
  dieses Buchs nie.
- Bevor ein Absatz gesetzt wird, zeigt der Reader ihm dessen Text, einmal,
  solange das Buch offen ist (`TextAsk::Block`). Das Plugin teilt ihn in
  Sätze, sucht jeden, liest gefundene Übersetzungen aus der Datei und
  antwortet sie einzeln mit `TextAnswer::gloss` (Art `Gloss`, 60 %) oder
  gesammelt mit `TextAnswer::insert`. Das Layout macht unter den Zeilen
  Platz und setzt die Übersetzung dorthin; ein Umblättern fragt nichts.

`src/lib.rs` ist der Teil, der ohne Gerät läuft und am Rechner getestet wird
(`cargo test --lib`); `src/main.rs` ist das Plugin darum.
