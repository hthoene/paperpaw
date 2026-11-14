### Changed

- **`paperpaw-dev` lists the widgets of native plugins.** Its gallery, lint and text checks used the interface-6 widget list, which no longer holds first-party widgets. They now ask `Widgets::everywhere()`. Measuring a widget's grid cell still used an interface-6 probe widget; four tool tests wait for a native probe.
