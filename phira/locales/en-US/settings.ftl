
label = SETTINGS

general = General
audio = Audio
chart = Chart
debug = Debug
about = Info

item-lang = Language
item-appearance = Appearance
item-appearance-sub = Custom icon, background and illustration
item-appearance-open = Open
item-appearance-import = Custom illustration
item-appearance-import-btn = Import
item-appearance-imported = Illustration imported
item-ui-theme = UI theme
item-ui-theme-sub = Accent and surface colours for the app interface
theme-blue = Phira Blue
theme-violet = Violet
theme-emerald = Emerald
theme-sunset = Sunset
theme-rose = Rose
theme-graphite = Graphite
item-limit-perfect-plus = Perfect+ window
item-limit-perfect = Perfect window
item-limit-good = Good window
item-limit-bad = Bad window
item-hp-mode = Health bar
item-hp-mode-sub = The run ends when the bar reaches 0
item-hp-amount = Damage multiplier
item-hp-width = Bar length
item-auto-retry = Auto retries
item-auto-retry-sub = Restart the run automatically after a failure (0 disables)
item-retry-lead = Retry lead-in
item-retry-lead-sub = Restart this many seconds before the failure (0 starts over)
item-practice-ramp = Speed ramp
item-practice-ramp-sub = Raise the speed after each practice loop
item-practice-speed = Start speed
item-practice-step = Step per loop
item-hp-height = Bar thickness
item-fullscreen = Fullscreen Mode
item-offline = Offline Mode
item-offline-sub = Disable all online functionality.
item-server-status = Server Status
item-server-status-sub = Open the server status page in your browser.
check-status = Open
item-mp = Multiplayer
item-mp-sub = Enable multiplayer functionality.
item-mp-addr = Multiplayer Server
item-mp-addr-sub = Connect to a custom multiplayer server.
item-mp-addr-invalid = Invalid server address.
item-lowq = Low Resolution Mode
item-lowq-sub = Lower the quality of the UI, increasing peformance.
item-clear-cache = Clear Cache
item-cache-size-loading = Loading…
item-cache-size = Cache size: { $size }
item-clear-cache-btn = Clear
item-cache-cleared = Cache cleared
item-insecure = Insecure Connection
item-insecure-sub = Enable old devices to use online functionality.
item-enable-anys = Enable Anys
item-enable-anys-sub = Use an Anys gateway to improve network stability.
item-anys-gateway = Anys Gateway
item-anys-gateway-sub = Use a custom Anys gateway address.
item-anys-gateway-invalid = Invalid gateway address.

item-adjust = Automatic Time Adjustment
item-adjust-sub = Adjust the audio and chart offset dynamically.
item-music = Music Volume
item-sfx = SFX Volume
item-bgm = BGM Volume
item-cali = Adjust Offset
item-preferred-sample-rate = Preferred Sample Rate
preferred-sample-rate-default = System Default
item-audio-buffer-size = Audio Buffer Size

item-show-acc = Real-Time Accuracy
item-show-avg-fps = Show AVG FPS
item-show-avg-fps-sub = Display the average FPS on the results screen.
item-ap-fc-indicator = AP/FC Indicator
item-ap-fc-indicator-sub = Use line color to indicate AP/FC status.
item-dc-pause = Double-Tap to Pause
item-dhint = Simultaneous Hint
item-dhint-sub = Highlight notes that are meant to be hit at the same time.
item-opt = Chart Optimization
item-opt-sub = Significantly increase peformance while playing. (If unintended behavior arises, disable this.)
item-use-keyboard = Use Keyboard
item-use-keyboard-sub = Enable keyboard input for gameplay. Scores cannot be uploaded when enabled.
item-prefer-reduced-motion = Prefer Reduced Motion
item-prefer-reduced-motion-sub = Reduce animations and visual effects
item-speed = Speed
item-note-size = Note Size

item-chart-debug = Show Line ID
item-chart-debug-sub = Display the IDs and orientation of lines.
item-show-fps = Show FPS
item-show-fps-sub = Show the current framerate in the bottom-left corner
item-touch-debug = Show Touch Points
item-touch-debug-sub = Display user touch points.

load-cali-failed = Failed to load calibration audio.

about-content =
  Phira Pro v{ $version }

  Phira is a non-commercial community-driven rhythm game inspired by Phigros.

  This is an unofficial player-run project, with no relationship of license, partnership, or operation with Pigeon Games Co., Ltd. or the official Phigros team.

  Phira Pro is a third-party, non-commercial modification based on the official Phira.

  BiliBili Account: @Phira官方
  QQ Guild: r48eajexth
  Discord Server: discord.gg/gqpR3bTSsP

  We recommend joining either the QQ guild or the Discord server to get live updates and receive assistance.

  Staff List (sorted lexicographically)
  Development
  { $development }

  Operations
  { $operations }

  Documentation
  { $documentation }

  Art
  { $art }

  Music
  { $music }

  Audio
  { $audio }

  Community Management
  { $community }

  Localization
  { $localization }

  Phira Pro Revision
  Maintenance
  { $revision }

  And many more voluntary chart reviewers. For a full list please refer to https://phira.moe/staff .

item-drag-protect = Drag Protection
item-drag-protect-sub = A tap is no longer eaten by an overlapping Drag (yellow) note.
item-flick-protect = Flick Protection
item-flick-protect-sub = A tap is no longer eaten by an overlapping Flick (red) note.
item-combo-text = Combo Label
item-combo-text-sub = The text shown under the combo counter in game (up to 16 characters)
combo-text-default = Default
item-late-leniency = Late Leniency
item-late-leniency-sub = Subtracts this much from late hits when judging; 0 = perfectly symmetric with early (upstream silently allowed 70ms, making late hits too forgiving)
item-judge-chart = Judgement Chart
item-judge-chart-sub = Draw a judgement timing distribution chart on the ending screen (early <- -> late)
item-hp-scale = Health Bar Rate
item-hp-color = Health Bar Color
item-upload = Upload Scores
item-upload-sub = [THIS BUILD CANNOT UPLOAD] In this open build the upload channel is disabled at compile time (official upload needs the closed-source score encoder), so this switch currently has no effect and scores are kept only on this device. Use the official client if you want to upload.
item-upload-consent = Upload Consent
item-upload-consent-open = View
upload-consent-title = Score Upload: Informed Consent and Disclaimer
upload-consent-accept = I have read and agree
upload-consent-deny = Disagree
upload-consent-text = When enabled, finishing an official chart uploads this run's score to the Phira official server: the chart ID, the run's score data (score / accuracy / judgement, in the official format) and the chart version timestamp, plus your account credentials in the request header. It does NOT upload device info, location, photos or any other file, collects no extra statistics, sends nothing to third parties, and there is no self-hosted server. Once uploaded the score appears on the Phira cloud leaderboard and your profile; RKS / EXP are settled by the official server. When disabled, scores stay only on this device (the local score history still records them), are not ranked and do not update cloud RKS. Note: in THIS build the upload channel is disabled at compile time (official upload relies on the closed-source score encoder), so this switch currently only records your intent and keeps the pipeline ready — scores are in fact kept locally. Disclaimer: this is an unofficial community build with no affiliation to TeamFlos / Phira. It modifies judging and presentation; options that would clearly affect score fairness are marked UNRATED and are never uploaded, but if you bypass those limits or combine them, your uploaded score may differ from the official client and you bear the consequences alone. This build is provided as is, without warranty of any kind. You can turn this switch off in the settings at any time.
