### Fixed

- **Search in a book a plugin handed to the reader** (#228). The reader opens its search screen from the answer to the question for the text, an event callback in which no screen instance runs, so the hand-over of the handed file (`root` `5`) was refused. In an event's callback the host now takes the topmost screen of the calling plugin that holds a file of that path (same path, same maker, read only), so a search in a plugin's own text works as in Beta 3.
