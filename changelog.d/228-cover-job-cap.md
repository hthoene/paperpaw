### Changed

- **The cover job goes slower on low power and stops after 10 minutes per book** (#228, #245). Between two steps of the background cover job the device waits 1 s in the power modes Saving and Offline (100 ms at Normal). A book's job may run 10 minutes in all per start of the device (time before a displacement or a sleep counts); after that the book gets no cover until the next start.
