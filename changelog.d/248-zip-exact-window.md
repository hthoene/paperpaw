### Changed

- **A deflated ZIP entry of up to 32 KiB is inflated into a window of exactly its size** (#248), no longer the next power of two: an EPUB's package document or chapter opened for its details or cover takes less heap (Alice's OPF window 8 → its size). Longer entries wrap through the 32 KiB history as before. An entry that inflates past its stated size is still refused.
