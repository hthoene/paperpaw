### Fixed

- **paperpaw-dev: the pseudo-language has every text of the app again** (#4511). It now comes as one compiled catalog (`i18n/pseudo.ppcat`), as a language pack does: its split text catalogs had grown past what a data plugin's text catalogs may take together, so the last ones were left out.
