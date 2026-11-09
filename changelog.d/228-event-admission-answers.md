### Added

- **A native plugin can register for the answers of its own requests and for work** (#228). The host admits the registrations of `event.wifi_done` (93), `event.work_due` (103), `event.work_refused` (104) and `event.work_allowed` (105); until now a plugin that named one of them was refused at `make` with `Unsupported`. The work events are delivered as before; the host now delivers `event.wifi_done` too, once per request and only to the plugin that asked.
