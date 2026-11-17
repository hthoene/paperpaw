### Fixed

- **The Home heap test no longer fails in parallel runs** (#292). `repeated_returns_do_not_grow_the_heap` counts the process's heap, which the other tests of `home_return` moved while running in parallel; the heap test now runs alone while the others share a lock. Test only.
