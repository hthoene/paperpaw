### Fixed

- **"Some saved passwords … cannot be opened here" no longer shows at every start** (#466). Passwords and keys saved on another device, or before the device's key was erased (a full re-flash), are now told once: the device keeps a small list of what it told (`.paperpaw/secrets-told.txt`, written only when it tells) and says it again only when a new one comes to the card. Entering such a password again where its plugin asks replaces the old entry; deleting a plugin's data removes all of its entries. Passwords that open here are not touched.
