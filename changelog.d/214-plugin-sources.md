### Added

- **Community plugin sources in the plugin store** (#214). *Sources*, the
  last row of the store's start, adds plugin lists of others by their
  address (`https://…/plugins.json`, in the format of PaperPaw's library),
  lists one in place of PaperPaw's and removes it again; PaperPaw's own
  library is always there. Addresses can also be put into
  `.paperpaw/plugins/store/sources.txt` on the card, one on each line. The
  store says before a source is added that PaperPaw does not check it. A
  source signs its list (`plugins.json.sig`: its Ed25519 key and the
  signature over `PaperPaw plugin source\0` and the list's SHA-256, as
  hexadecimal digits); the device keeps the key of the first list it reads
  and from then on refuses a list of that source signed with another key
  or with none, and a code plugin not signed with it. A plugin is updated
  only from the source it was installed from, a plugin's page names its
  source, and a source's list is read up to 512 KiB.
