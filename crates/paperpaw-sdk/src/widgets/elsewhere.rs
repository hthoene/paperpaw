//! The widgets declared to the kernel that are not registered here: those
//! of native plugins, which the kernel keeps as records, not as
//! [`WidgetSpec`](super::WidgetSpec)s. The kernel's own screens that name
//! widgets (the editor of lists of widgets) ask for them through a lookup
//! the kernel sets; without one they know the registered widgets only.
