### Changed

- **Asking the registry for one setting no longer reads every declared extra of the device** (#248, #228). Whether another setting's showing depends on the one asked for was answered by decoding every `setting.extra` record of every native plugin. For plugins whose declarations stay in their image, only the records whose bytes hold the setting's id are decoded now, and each image is read once.
