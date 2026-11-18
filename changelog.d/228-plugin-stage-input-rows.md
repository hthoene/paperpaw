### Fixed

- **A plugin-defined stage takes several input records** (#228). The host cannot
  see a stage's own `input_rows`, so the stage broker's input check for
  plugin-defined stages now accepts any canonical batch of up to 64 records
  within `max_input`; 65 records, trailing bytes and truncated frames are
  refused.
