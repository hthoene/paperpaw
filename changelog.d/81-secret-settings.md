### Added

- Plugins on interface 7 can declare secret settings, such as a mail password (#81). The value is kept with the encrypted secrets, never in `paperpaw.ini` or the plain settings: no plugin reads it through the settings, not even its own (it asks for its secret), and no other plugin is told when it changes. A value an older card still keeps in `paperpaw.ini` moves into the secrets at the start and is then taken out of the file. The settings editor can enter, change or clear it.
