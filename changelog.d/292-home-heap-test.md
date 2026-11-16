### Fixed

- **The Home heap test no longer fails in parallel runs** (#292). `repeated_returns_do_not_grow_the_heap` shares the process's heap counter with the other tests of `home_return`, which ran in parallel threads and moved it; the tests of that binary now run one at a time. Test only.
