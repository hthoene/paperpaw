### Fixed

- **Opening Settings no longer slows with the square of the number of settings** (#228, #248). The settings list finds which settings another's showing depends on in one pass per list, not one pass over every setting per row; on the X4 emulator this was about three quarters of a Settings open. The flag each row carries is unchanged.
