### Fixed

- **`paperpaw-dev metadata` refuses a list that is cut off or broken** (#244): a `native/metadata.json` that is not one complete JSON value ended in a shorter record without a word; now the command says so and fails.
