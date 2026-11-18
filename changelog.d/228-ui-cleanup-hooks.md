### Changed

- **Cleanup of widgets and status values** (#228). When a screen closes, the widget instances it made are freed. A status handle ends with the callback that got it. The cached status values end on the status-changed event and each minute, and a lock screen starting drops the cached private ones.
