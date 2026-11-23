### Fixed

- **Screens that end by themselves stay until you go back** (#486). The Wi-Fi
  phone setup, the Wi-Fi transfer's own network and the USB transfer tried to
  close their screen from a timer, which the reader does not allow there. The
  screen stayed, and after ten minutes without the phone the reader's own
  network could stay open with Wi-Fi on. Now the network closes, Wi-Fi goes
  back to how you left it, and the screen says the setup or the transfer ended
  until you press Back. The USB transfer's permission now really ends after
  five minutes without a request, as its screen says.
- **Leaving a screen early closes the reader's own network** (#486). Going
  back from the Wi-Fi phone setup or the Wi-Fi transfer before the reader's own
  network was up left it open once it came up. Now it is closed.
- **Saved networks and paired devices can be forgotten with the buttons**
  (#486). Only a long press on a touch screen forgot one. As in Beta 3, holding
  Select or the menu button forgets the saved Wi-Fi network or the Bluetooth
  device that has the focus, and the key bar says *Forget* there.
- **A Wi-Fi password that is too long is refused on the phone** (#486). A
  password of more than 63 characters typed on the phone left the setup at
  *Connecting…* for good. Now the phone's page asks for it again and says why.
- **The networks list stays open when Wi-Fi goes off** (#486). It tried to
  close itself, which failed. Now it says that Wi-Fi is off.
- **Finding networks reads saved passwords only when they are needed** (#486).
  Every scan read every saved Wi-Fi password, even when no network was to be
  joined. Now they are read only while the reader joins a saved network by
  itself, and the networks in reach are read once per scan.
- **Typing on the phone** (#486). Without Wi-Fi the phone's typing screen
  showed *Without a router*, which did nothing there; now it says that the
  device needs a Wi-Fi network, and only Back is on the key bar. Once the text
  has come from the phone, the screen says so until Back, instead of trying to
  close itself.
- **The USB transfer's key bar names only Back** (#486), as in Beta 3.
- **The Bluetooth sign is in the status line again** (#486). As in Beta 3, the
  Bluetooth status item shows while a device is connected, and a tap opens the
  Bluetooth page.
- **Going back from adding a Bluetooth device stops the search** (#486). The
  search went on for up to 30 seconds, and a device still being paired stayed
  connected without its keys switched on. Now the search stops and the device
  is let go.
- **The transfer's reasons are in your language** (#486). *The transfer could
  not start* and *The transfer stopped* gave the reason in English words in
  every language.
- **Large folders list quickly in the Wi-Fi transfer** (#486). Each part of a
  folder's list read the folder from its start again, so a folder of a
  thousand books took many card reads. Now each part goes on where the one
  before ended.
- **The own network's password is wiped** (#486). The Wi-Fi transfer's own
  network kept its password in the join code and in the text below it after
  the screen closed; both are wiped now.
