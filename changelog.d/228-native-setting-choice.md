### Fixed

- **A linked native plugin can add a choice to another plugin's setting** (#228). A `setting.choice` record was refused at admission with `ERR_UNSUPPORTED`, so a plugin that declared one (vocabulary's sleep card for the sleep screen) was not registered at all. The host now offers it as interface 6's `add_choice` does; one for a setting that does not exist is dropped as before.
