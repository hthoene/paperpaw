### Changed

- **A plugin install lets its 4 KiB buffer go before the question** (#228, #248). Nothing after the trust rules reads through it, so it is no longer held while the user decides, nor through the marks and the swap.
