### Changed

- Wi-Fi: after a search the Wi-Fi page and the list of networks draw only the rows that say something else (a signal that moved, a network that is kept now) and refresh just those; nothing is drawn where no visible row changed. Before, every such result drew and refreshed the whole list (a broad frame of about 1.6 s on the X4). The whole list is still drawn when the number of networks changes. (#223)
