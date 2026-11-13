### Added

- **Interface 7, lookup: answers that come later and entries that open more** (#244, dictionary parity). A lookup entry (`lookup_entry`, the stage reply `lookup.entry`) gains `opens`, the screen that shows more of it (a whole article; Beta 3's *Open Wikipedia ›*), which the asker opens with a hand-over of the word, the language and the entry's text. The SDK's `lookup::Entry` gains `pending()` for a provider that fetches and answers after `lookup_ready`, and `opens()`; `ProviderEntry::opens()` reads it. The SDK's docs no longer say that a provider that fetches is refused.
