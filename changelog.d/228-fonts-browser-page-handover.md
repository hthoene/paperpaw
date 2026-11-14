### Fixed

- **Fonts browser on interface 7: a font's page opens on the X4** (#228, #244). The list handed the whole font over to its page, more than the 1,024 bytes a screen may be handed where the library lists the font's packs; now it hands over the font's id and the page finds the font in the catalog it keeps.
