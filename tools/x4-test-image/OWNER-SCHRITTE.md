# X4-Testimage: Schritte für den Owner

Das ist das **Testimage** (#243): Die nativen Plugins sind an, und Passwörter (WLAN, Sperre, Konten) werden mit dem Schlüssel des Geräts aufbewahrt (der echte Sealer, #1224). Es ist kein Release und nichts für andere Leute. **Du flashst selbst; kein Agent tut das.** Das wurde auf keinem Gerät ausprobiert.

> **Passwörter bleiben erhalten; eine Sicherung des Flashs bleibt trotzdem ratsam** (Schritt 2).

Du brauchst: den X4, ein USB-Kabel, Akku über 50 %, `espflash`
(`cargo install espflash --version 4.6.0 --locked`) und diesen Ordner.

## 1. Bundle prüfen

```sh
sha256sum -c SHA256SUMS            # jede Zeile muss OK sagen
# Windows PowerShell:  Get-FileHash <IMAGE> -Algorithm SHA256   (mit SHA256SUMS vergleichen)
```

Bei FAILED anhalten. `BUILD-INFO.txt` sagt auch, ob die App in den kleineren Slot von CrossPoint passt;
wenn nicht, geht dieses Image nur per USB, nie aus dem CrossPoint-Menü. Commit und Features stehen in `BUILD-INFO.txt`
(`test-image` ist dabei, `release` nie).

## 2. Sichern, was der X4 jetzt hat (nicht auslassen)

X4 per USB anschließen. Die Datei (16 MB) gut aufheben:

```sh
espflash read-flash 0x0 0x1000000 x4-before-test-image.bin
```

Die microSD-Karte wird nie angefasst: Bücher und Einstellungen bleiben.

## 3. Flashen

```sh
espflash erase-parts otadata --partition-table partitions.csv   # damit das neue Image startet
espflash write-bin 0x0 <IMAGE>
```

Danach Reset drücken (oder USB abziehen und wieder anstecken).

## 4. Was du sehen solltest

- PaperPaw startet und zeigt den Startbildschirm.
- Im Start-Log (`espflash monitor`, dann X4 zurücksetzen; das Testimage hat keine USB-Fernsteuerung) siehst du den Start; bei Auffälligkeiten aufheben.
- Alle 30 s steht eine `high-water:`-Zeile im Log (Stack frei, Heap frei, größter
  Block); nach einem Neustart sagen die ersten Zeilen `high-water: the run before …`.
  Bei Auffälligkeiten diese Zeilen kopieren.
- Ein Buch öffnen, ein paar Seiten blättern, den X4 schlafen legen und wecken.
- WLAN: ein WLAN-Passwort speichern, X4 neu starten: er verbindet sich ohne Nachfrage wieder
  (das Passwort ist nur über den Sealer lesbar). Hatte Beta 3 schon Passwörter gespeichert,
  funktionieren sie weiter, und das Log sagt einmal `secrets: veiled values sealed`, nachdem
  WLAN zum ersten Mal an ist (nicht beim Einschalten). Ein sauberes Gerät schreibt gar keine
  secrets-Zeile: das ist normal, kein Fehler. Die Zeilen `secrets: no place for the device key`
  und `secrets: veiled values not sealed (...)` sind Probleme: Log aufheben.

Bitte alles notieren, was falsch aussieht, möglichst mit den Log-Zeilen.

## 5. Zurück

```sh
espflash write-bin 0x0 x4-before-test-image.bin
```

Das schreibt den ganzen Flash von Schritt 2 zurück. Startet er danach nicht, einmal
`espflash erase-parts otadata --partition-table partitions.csv` ausführen. Die Karte
bleibt unberührt. Reagiert der X4 gar nicht auf
das Kabel, beim Verbinden die Power-Taste gedrückt halten.

## Wenn er nicht startet

- Bild ändert sich nie oder Neustart-Schleife: USB abziehen, Power 10 s halten,
  nochmal versuchen; wieder Schleife → Schritt 5 und das Log schicken
  (`espflash monitor` direkt nach dem Neustart). Die Firmware hat einen Schutz: nach
  zwei fehlgeschlagenen Starts der nativen Plugins startet sie **ohne** sie (steht im
  Log); eine Schleife, die trotzdem weitergeht, ist ein anderes Problem: Log schicken.
- Ein Stack-Überlauf-Test gehört nicht zu diesem Image; er braucht dein eigenes Okay.
