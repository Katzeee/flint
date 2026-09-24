"""The common Qt connection panel used by the Maya and 3ds Max packages."""
from PySide2 import QtCore, QtWidgets


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
        form_layout.addRow("Registry address", self.address)
        form_layout.addRow("Registry port", self.port)
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

        self.apply_button.clicked.connect(self.apply)
        self.retry_button.clicked.connect(self.retry)
        self.timer = QtCore.QTimer(self)
        self.timer.timeout.connect(self.refresh)
        self.timer.start(1000)
        self.refresh()

    def refresh(self):
        snapshot = self._snapshot()
        state = snapshot["connection"].replace("_", " ").title() if snapshot else "Stopped"
        active = snapshot["settings"] if snapshot else None
        self.status.setText(state)
        self.active.setText("{}:{} · {}".format(
            active["address"], active["port"], active["name"]) if active else "—")
        error = self._action_error or (snapshot["last_error"] if snapshot else None)
        self.warning_text.setText(error or "")
        self.warning.setVisible(bool(error))
        busy = bool(snapshot and snapshot["busy"])
        self.apply_button.setEnabled(bool(snapshot and not busy))
        self.retry_button.setEnabled(bool(snapshot and active["enabled"] and not busy))

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
