### Fixed

- **Recently opened books show at once after a start, also when the library's list was made before they came** (#253). A book opened lately that the list of the library did not know yet (it came after the list was made) showed only when the walk at the start ended. It is now added to the end of the list at the start, a line each, before the walk: the list is not written anew, and a start where nothing is missing reads none of it. A cut off while it is added leaves the list readable.
