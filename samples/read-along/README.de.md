# Mitlesen: den vorgelesenen Satz markieren

*English: [README.md](README.md).*

Ein PaperPaw-Beispielplugin mit Code für die Plugin-Schnittstelle 7 (#377).
Während ein Buch vorgelesen wird („Vorlesen“ im Reader), zeichnet es in jeder
Zeile des Satzes einen Rahmen um die gelesenen Wörter oder eine dicke Linie
darunter; die Seite wird für jeden Satz in einer schnellen Auffrischung neu
gezeichnet. Solange nicht vorgelesen wird, wird nichts gezeichnet, gefragt
oder gehalten.

## Benutzen

1. Bauen und auf die Karte oder in die Bibliothek des Emulators kopieren
   (siehe [CODE.md der Vorlage](../../templates/plugin-source/CODE.md):
   `paperpaw-dev build`, `paperpaw-dev build --device emulator`).
2. *Einstellungen → Lesehilfen → Vorgelesenen Satz markieren*: *Rahmen*
   (Vorgabe), *Dicke Unterstreichung* oder *Nicht markiert* (dann folgt das
   Plugin dem Vorlesen gar nicht). Die dünne Unterstreichung des Readers und
   sein Umblättern haben ihre eigenen Einstellungen (*Satz unterstreichen*,
   *Umblättern*): das Plugin überlässt beides dem Reader.

## Wie es funktioniert

Siehe [README.md](README.md): Das Plugin hängt sich an die Ebenen-Stufe
`reader.layer` des Readers (`docs/abi/page-layers.md`), meldet mit
`LayerAnswer::uses_reading_aloud`, dass es dem Vorlesen folgt, und bekommt für
jeden vorgelesenen Satz die Seite mit den gelesenen Wörtern
(`LayerPage::spoken`). Es antwortet je Zeile mit einem Rahmen oder einer Linie.
Die Sprachausgabe meldet nur ganze Sätze, daher wird der Satz markiert, nicht
das Wort.
