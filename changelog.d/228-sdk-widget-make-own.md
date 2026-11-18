### Fixed

- **`Ui::widget_make` reads the host's answer for the caller's own widget** (#228). The host answers op 4 for an own widget with the own marker and instance 0, like op 15; the SDK refused it as `Invalid`. `widget_make` now returns `MadeWidget::Own` or `MadeWidget::Widget(WidgetHandle)`, an own marker with an instance or an instance without the marker is `Invalid`, and `WidgetHandle::is_own` is gone: a handle is always one the host made. The home, picture-home and sleep plugins follow.
