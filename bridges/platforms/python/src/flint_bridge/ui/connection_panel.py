"""The common Qt connection panel used by the Maya and 3ds Max packages.

The status area shows the Bridge's own snapshot, including why its connection
is retrying. A refused Apply or Reconnect is that action's result: it appears
beside the buttons until the next action or edit, and polling never replaces it.
"""
from ..qt import resolve_qt

_qt = resolve_qt()
QtCore, QtWidgets = _qt.QtCore, _qt.QtWidgets


def connection_label(snapshot):
    return snapshot["connection"]["state"].replace("_", " ").title() if snapshot else "Stopped"


def connection_obstacle(snapshot):
    obstacle = snapshot["connection"].get("obstacle") if snapshot else None
    return obstacle["message"] if obstacle else None


class ConnectionPanel(QtWidgets.QDialog):
    def __init__(self, settings, snapshot, apply_settings, reconnect, parent=None):
        super().__init__(parent)
        self._snapshot = snapshot
        self._apply_settings = apply_settings
        self._reconnect = reconnect
        self._action_error = None
        self.setWindowTitle("Flint Bridge")
        self.setMinimumWidth(480)

        layout = QtWidgets.QVBoxLayout(self)
        connection = QtWidgets.QGroupBox("Connection")
        connection_layout = QtWidgets.QFormLayout(connection)
        connection_layout.setLabelAlignment(QtCore.Qt.AlignLeft | QtCore.Qt.AlignVCenter)
        connection_layout.setHorizontalSpacing(12)
        self.status = QtWidgets.QLabel()
        self.active = QtWidgets.QLabel()
        connection_layout.addRow("Status", self.status)
        connection_layout.addRow("Active settings", self.active)
        layout.addWidget(connection)

        self.warning = QtWidgets.QFrame()
        self.warning.setFrameShape(QtWidgets.QFrame.StyledPanel)
        warning_layout = QtWidgets.QHBoxLayout(self.warning)
        warning_icon = QtWidgets.QLabel()
        icon = self.style().standardIcon(QtWidgets.QStyle.SP_MessageBoxWarning)
        warning_icon.setPixmap(icon.pixmap(20, 20))
        warning_layout.addWidget(warning_icon)
        self.warning_text = QtWidgets.QLabel()
        self.warning_text.setWordWrap(True)
        warning_layout.addWidget(self.warning_text, 1)
        layout.addWidget(self.warning)

        form = QtWidgets.QGroupBox("Settings")
        form_layout = QtWidgets.QFormLayout(form)
        form_layout.setLabelAlignment(QtCore.Qt.AlignLeft | QtCore.Qt.AlignVCenter)
        form_layout.setHorizontalSpacing(12)
        self.address = QtWidgets.QLineEdit(settings["address"])
        self.port = QtWidgets.QSpinBox()
        self.port.setRange(1, 65535)
        self.port.setValue(settings["port"])
        self.name = QtWidgets.QLineEdit(settings["name"])
        self.enabled = QtWidgets.QCheckBox()
        self.enabled.setChecked(settings["enabled"])
        form_layout.addRow("Bridge address", self.address)
        form_layout.addRow("Bridge port", self.port)
        form_layout.addRow("Instance name", self.name)
        form_layout.addRow("Connect to Flint", self.enabled)
        layout.addWidget(form)

        buttons = QtWidgets.QHBoxLayout()
        self.apply_button = QtWidgets.QPushButton("Apply")
        self.retry_button = QtWidgets.QPushButton("Reconnect")
        for button in (self.apply_button, self.retry_button):
            button.setSizePolicy(QtWidgets.QSizePolicy.Expanding, QtWidgets.QSizePolicy.Fixed)
            buttons.addWidget(button, 1)
        layout.addLayout(buttons)
        self.action_result = QtWidgets.QLabel()
        self.action_result.setWordWrap(True)
        self.action_result.setStyleSheet("color: #d9534f;")
        layout.addWidget(self.action_result)

        self.apply_button.clicked.connect(self.apply)
        self.retry_button.clicked.connect(self.retry)
        for signal in (self.address.textEdited, self.port.valueChanged,
                       self.name.textEdited, self.enabled.toggled):
            signal.connect(self._clear_action_error)
        self.timer = QtCore.QTimer(self)
        self.timer.timeout.connect(self.refresh)
        self.timer.start(1000)
        self.refresh()

    def refresh(self):
        snapshot = self._snapshot()
        active = snapshot["settings"] if snapshot else None
        self.status.setText(connection_label(snapshot))
        self.active.setText("{}:{} · {}".format(
            active["address"], active["port"], active["name"]) if active else "—")
        obstacle = connection_obstacle(snapshot)
        self.warning_text.setText(obstacle or "")
        self.warning.setVisible(bool(obstacle))
        self.action_result.setText(self._action_error or "")
        self.action_result.setVisible(bool(self._action_error))
        busy = bool(snapshot and snapshot["busy"])
        # Without a Bridge, Apply starts one from these settings.
        self.apply_button.setEnabled(not busy)
        self.retry_button.setEnabled(bool(snapshot and active["enabled"] and not busy))

    def _clear_action_error(self, *_):
        if self._action_error:
            self._action_error = None
            self.refresh()

    def apply(self):
        try:
            self._apply_settings(
                self.address.text(), self.port.value(), self.name.text(), self.enabled.isChecked())
            self._action_error = None
        except Exception as problem:
            self._action_error = str(problem)
        self.refresh()

    def retry(self):
        try:
            self._reconnect()
            self._action_error = None
        except Exception as problem:
            self._action_error = str(problem)
        self.refresh()
