# Launcher mocks (#236)

Mock screens for a simple launcher plugin: four concepts (A words, B tiles,
C one book, D panel) with three screens each, drawn with the SDK's blocks
(`paperpaw-ui-kit`) by a small interface-7 plugin for the emulator. The
content is made up (`src/data.rs`); left and right move the focus, nothing
opens. Not a product and not part of any build: it is its own workspace.

    ./shots.sh            # builds the plugin, takes the screenshots into out/
    python3 sheets.py     # one sheet per concept (needs Pillow)

`shots.sh` needs a `paperpaw-emu` of the same tree (default
`../../target-qc-lane5/debug/paperpaw-emu`; or pass its path). A screen is
chosen as the home screen with `--set slots.core.home=launcher-mocks.<screen>`.

What the mocks showed about the X4:

- Text and icons end near 48 pixels (font packs), so the clock of concept D
  is drawn from round strokes, and icons larger than about 40 pixels would be
  pictures of the plugin's own.
- A screen's part of the display is `frame.rect`: the row of hints below it
  is the host's.
- The library's cover of a book stays `Busy` for a plugin that only asks
  (nothing here makes it), so the covers are drawn ones.
