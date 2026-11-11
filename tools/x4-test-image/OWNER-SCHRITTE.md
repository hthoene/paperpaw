# X4-Testimage: Schritte für den Owner

Das ist das **Testimage** (Entscheidung 156, #243): Die nativen Plugins sind an,
Passwörter (WLAN, Sperre, Einstellungen) sind nur **verschleiert**, nicht
verschlüsselt. Es ist kein Release und nichts für andere Leute. **Du flashst
selbst; kein Agent tut das.** Das wurde auf keinem Gerät ausprobiert. Schritte
mit *(offen #2462)* werden ergänzt, wenn Seniors Bedingungen fertig sind.

> **Notiere vor dem Flashen deine WLAN- und Konto-Passwörter; du musst sie später
> neu eingeben.** Beim ersten Schreiben eines Geheimnisses verschleiert das
> Testimage *alle* Passwortzeilen von Beta 3 neu. Mit der echten Verschlüsselung
> später oder nach dem Zurück auf Beta 3 sind alle Passwörter auf diesem X4 (WLAN,
> Konten) weg und müssen neu eingegeben werden. Bücher und übrige Einstellungen
> bleiben.

Du brauchst: den X4, ein USB-Kabel, Akku über 50 %, `espflash`
(`cargo install espflash --version 4.6.0 --locked`) und diesen Ordner.

## 1. Bundle prüfen

```sh
sha256sum -c SHA256SUMS            # jede Zeile muss OK sagen
# Windows PowerShell:  Get-FileHash <IMAGE> -Algorithm SHA256   (mit SHA256SUMS vergleichen)
```

Bei FAILED anhalten. Commit und Features stehen in `BUILD-INFO.txt`
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

- PaperPaw startet und zeigt den Startbildschirm. *(offen #2462: erster Start)*
- Einstellungen › Info (oder Diagnose) zeigt **„Secrets: veiled (test build)“**.
- Im Start-Log (`python tools/x4-remote.py --log 30`, aus einem PaperPaw-Checkout)
  steht der Verschleierer.
- Ein Buch öffnen, ein paar Seiten blättern, den X4 schlafen legen und wecken.
- WLAN: einmal verbinden, X4 neu starten, prüfen, dass das Passwort noch da ist.

Bitte alles notieren, was falsch aussieht, möglichst mit den Log-Zeilen.

## 5. Zurück

```sh
espflash write-bin 0x0 x4-before-test-image.bin
```

Das schreibt den ganzen Flash von Schritt 2 zurück. Reagiert der X4 gar nicht auf
das Kabel, beim Verbinden die Power-Taste gedrückt halten.

## Wenn er nicht startet

- Bild ändert sich nie oder Neustart-Schleife: USB abziehen, Power 10 s halten,
  nochmal versuchen; wieder Schleife → Schritt 5 und das Log schicken
  (`python tools/x4-remote.py --log 60` direkt nach dem Neustart).
  *(offen #2462: Schleifenschutz / Safe Mode)*
