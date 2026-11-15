### Fixed

- **The power menu does what it says again** (#266). Choosing *Sleep*, *Switch off* or *Restart* in the menu the held power button opens did nothing: the power plugin ignored the dialog's answer (it came under a token the plugin did not look at), and *Switch off* then failed on its second opening of the interface service. Sleep and restart work after the first fix, switch off after the second. The power settings open after the menu was used. The emulator's X4 profiles offer *Switch off* (and show it as a switch-off) as the real X4 does.
