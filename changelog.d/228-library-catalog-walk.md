### Fixed

- **The library is collected with native formats only** (#243, #245). With interface 7 the library stayed at "Preparing the library … 0 books": no native view asked for the catalog, and the walk only knew interface 6 formats. Now the first read of the library (`core.library` state or query) since the start or a wake asks for the catalog once, as Beta 3's library view did when it showed. The files of native format plugins (their declared extensions) are books of the walk and of the book check too, together with the interface 6 formats.
