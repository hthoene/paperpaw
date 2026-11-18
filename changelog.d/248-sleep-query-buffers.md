### Changed

- **The sleep screen asks the host in less memory** (#248). Reading the screen list and its open parameters, and letting its widgets go, took a 4 KiB request buffer each time; the requests take at most 256 bytes, so they now take 256. What shows is unchanged.
