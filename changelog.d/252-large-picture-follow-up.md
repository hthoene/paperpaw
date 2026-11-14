### Fixed

- **A page's large pictures, drawn right after its text, no longer count as a page turned**
  (#252). Their frame could be the one made the periodic full refresh: the whole screen
  flashed for the picture, and again on the next page turn. Now it refreshes the pictures'
  box only, however large, as small pictures always did.
