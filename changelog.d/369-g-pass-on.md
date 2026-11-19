### Added

- Native reading apps: `command.pass_on` (SDK `CommandSender::pass_on`) passes a book the app cannot open after all to the next reading app that reads it, in the order of #260 and then any other, leaving out the ones that passed it on; with none left the reader closes with a notice. The reader passes on a book no format of it reads (#369 G, #260).
