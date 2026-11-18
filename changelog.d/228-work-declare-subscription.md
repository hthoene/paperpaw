### Changed

- **`core.work` `declare` needs a `work_due` subscription** (#228). A plugin that did not subscribe to `event.work_due` (103) gets `ERR_STATE` from `declare` (after the form checks), as `core.net` asks for `http_done`: work it could not be told of is never kept.
