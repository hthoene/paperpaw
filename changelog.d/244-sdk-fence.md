### Changed

- **SDK: the old plugin API is public only with the `kernel` feature** (#244). `Plugin`, `Registrar`, `Screen`, `Cx`, `Env`, `Command`, `Commands`, the events, actions, menus, fonts, formats, plugins and status stores, and the modules `ask`, `bluetooth`, `chrome`, `collections`, `contribution`, `developer`, `event`, `host`, `layers`, `power`, `presets`, `render`, `replies`, `services`, `setup`, `sound`, `voices`, `words` and `work` are the crate's own in a build without it, so a plugin cannot name them; the kernel, the app and the shells build with the feature and see everything as before. A plugin is written against `paperpaw_sdk::native`. Still public beside it: ids, storage, the setting specs, `MenuNeed`, `Handled`, and the modules interface-7 plugins use (their functions that take an `Env` or give a `Command` follow in a second step). CI builds every plugin's library and the UI kit alone (`tools/ci/plugin-libs-alone.sh`), since the workspace's build turns the feature on for everyone.

### Added

- **`paperpaw_sdk::data::plugin_dir` and `paperpaw_sdk::numbers::parse_f32`** (#244): a plugin's data folder and the SDK's small parser of floats, which were reachable only through `Env::plugin_dir` and the events' `ParamValue`.
