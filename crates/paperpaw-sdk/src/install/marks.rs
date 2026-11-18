//! What an installer keeps for a plugin once it is in place: its code
//! named as installed ([`native::remember_code`]), its author, where it
//! came from ([`origin`](super::origin)). Kept before the kernel installs
//! the plugin, each would be wrong where the install fails or the power
//! is cut first: the version before would be refused at the next start,
//! or lose its store. So the installer writes them into the folder it
//! hands over ([`write()`]), and the kernel keeps them once the folder is
//! the plugin's ([`settle`]): right after the install, or at the next
//! start where that was cut short. The file is hidden: neither the Wi-Fi
//! transfer nor the cable reach it.
