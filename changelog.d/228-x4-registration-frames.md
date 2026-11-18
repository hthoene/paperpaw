### Fixed

- X4: registering a native plugin stays within the 2 KiB stack band its code
  runs over. `bind_native` builds the plugin's dispatch in a small generic
  step and stages and publishes the rest in outlined steps that share one
  borrowed staging list, and a declaration batch is checked one record at a
  time instead of decoding all eight on the stack (the batch is decoded
  twice: once whole, once record by record). On the X4 the largest of
  these frames is now 1,136 B (was 2,944 B for `PendingDeclarations::stage`
  and up to 3,984 B for `bind_native`); statuses and their order are unchanged:
  a decoding error anywhere in a batch still comes before any record's own
  status.
