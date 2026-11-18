### Changed

- **Kernel: status items and lines, item states, setup steps, widgets, decorators and formats hold plain functions (#293, step S3).** What they compute is a function kept in the program (`StatusFn`, `ItemStateFn`, `AppliesFn`, `WidgetFactory`, `DecorateFn`, `FormatOpener` …), no boxed closure each; a data plugin's marks keep their rules as data (`Decorate::Patterns`), a drawn widget its functions (`widgets::Make`). The unused `BookBadge` is gone (kernel side only; interface 7 unchanged).
