### Changed

- **The Wi-Fi plugin's radio policy on interface 7, first part** (#228). The decisions of when the
  radio is on and which network is joined, and the plugin's declarations, are in place without the
  host and refuse cleanly when memory is short; a join that another request replaced no longer
  counts as a failure. The pages follow.
