# QML UI regressions

Run with Python and PySide6 matching the application's Qt version (currently 6.12):

```sh
python tests/qml/queue_view_regression.py
python tests/qml/player_motion_regression.py
python tests/qml/cover_transition_regression.py
python tests/qml/font_scale_regression.py
python tests/qml/accessibility_activation_regression.py
```

The queue test loads the real `QueueView.qml` in an offscreen Qt window. Player/catalog services and visual leaf components are stubs; no Spotify account, Rust build or Android device is required.

It checks header order through 80 queue resets/inserts/removals, manual-row reordering, a stationary header while dragging, and drop positions with a context card above the list. It does not test Spotify commands, platform accessibility or rendering performance; those require the Android device pass.

The motion tests use the actual player lifecycle blocks, SheetMenu and NowPlayingCover with lightweight visual/service stubs. They cover reopening during dismissal, drag-release continuity, short-drag recovery, rapid cover replacement, cancellation of a pending old cover after hiding the player, and reduced motion. A real device pass is still required to judge frame pacing and visual feel.

The font test loads the real Theme, labels, fields, buttons, chips and mobile navigation against a nonlinear platform converter stub. It checks live scale changes, line metrics, input bounds, navigation labels and unchanged desktop typography. The activation reproducer isolates a Qt thread-boundary failure with a TextInput and Popup: resolving their Accessible attached properties on the GUI thread prevents foreign-thread object creation on first accessibility activation. Android JNI conversion and TalkBack still require a device pass.
