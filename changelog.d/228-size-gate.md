### Changed

- **CI holds the firmware's size** (#258). The firmware job builds each device's install image as the release does and fails when it is over its line in `shells/xteink/size-budget.txt`, or when the image or the line is missing (`tools/size-report.py`).
