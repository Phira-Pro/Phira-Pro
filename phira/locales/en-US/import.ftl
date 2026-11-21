
info-fail = Failed to load info.
invalid-chart = Invalid chart.

importing = Importing...
import-success = Imported successfully.
import-failed = Import failed.
import-respack-success = Imported successfully.
import-respack-failed = Failed to import respack.

batch-import = Batch Import
batch-importing = Batch importing... ({ $current }/{ $total })
batch-import-confirm = Batch import data detected. Import { $count } charts?
batch-import-success = Successfully imported { $count } charts
batch-import-downloaded-skipped = Charts skipped due to already downloaded: { $charts }
batch-import-failed = Batch import failed
batch-import-failed-chart = Batch import failed: { $chart }

warning = Warning
warning-new-speed-event = This chart uses the speed event easing introduced in RPE 1.7.0. For compatibility reasons, Phira does not enable support for this event by default. If you want to enable it, please check "New Speed Tween" in the chart information.
warning-attach-ui = This chart uses UI attachment. Recent versions of Phira introduced a fix for UI attachment that may cause issues with charts that rely on the old behavior. If you encounter any problems, please uncheck "Attach UI Fix" in the chart information.

deeplink-title = Import Chart from Link
deeplink-confirm = Download and import the chart from this link?
deeplink-unofficial = Unofficial source: this file is not provided by Phira's official server ({ $host }). Make sure you trust the source before downloading.
deeplink-download = Download
deeplink-bad-url = Invalid chart file link.
deeplink-downloading = Downloading...
deeplink-dl-failed = Download failed
deeplink-too-large = The file exceeds the maximum size allowed for link import (100 MiB)
deeplink-opening = Opening chart...
deeplink-open-failed = Failed to open the chart

warning-rpen-features = This chart uses new RPEN features. Support is in early testing and may display incorrectly. You can disable "RPEN Features" in chart information.
lane-import-title = Import lane chart
lane-import-positions = Lane X positions
lane-import-spacing = Lane spacing
lane-import-height = Judgement line Y
lane-import-speed = Scroll speed
lane-import-custom-layout = Custom
lane-import-confirm = Convert and import
lane-import-positions-help = Enter { $count } X positions from left to right, separated by commas. Each value must be between -675 and 675.
lane-import-spacing-help = Enter a positive spacing that fits all lanes on screen. This centers the lanes.
lane-import-height-help = Enter a value from -450 (bottom) to 450 (top).
lane-import-speed-help = Enter a speed greater than 0 and at most 1000. The default is 12.
