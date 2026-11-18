### Added

- **`RegistryWidget::previews()` and `Registry::widgets_previewing`** (#228). The registry reports which settings group a widget previews (`widget.spec.previews`), and `widgets_previewing(group, ...)` lists the widgets that preview a group, in the host's order (a settings page shows the last). `widgets` still leaves them out.
