### Fixed

- **Calendar: repeats from another time zone fall on the right days** (#491).
  An event that repeats on Tuesdays at 18:00 in Los Angeles showed on
  Tuesdays at 03:00 in Berlin; it now shows on Wednesdays at 03:00, as it
  happens there.
- **Mail: a folder made anew on the server shows its own mails** (#491).
  When a folder was deleted and made again on the server, a mail read before
  could be shown in place of a new one with the same number. A mail kept
  from the old folder is now fetched again.
- **Anki: cards of note types with long styles come over** (#491). Cards
  whose note type has more than 4 KB of CSS were left out of every sync
  without a word; they now come over whole.
- **Anki: an answer never reaches Anki twice** (#491). When Anki's reply to
  the answers sent did not come back in time, the next sync sent them again
  and Anki counted them twice. Answers whose reply was lost are no longer
  sent again; a card Anki did not get stays due and comes with the next
  cards.
- **Translate: a key that could not be saved is not asked for again at
  once** (#491). On a device that cannot keep passwords, the key typed was
  asked for again after *Go online*; it is now used as long as the plugin
  runs.
