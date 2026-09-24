const { app, BrowserWindow } = require("electron");

app.whenReady().then(() => {
  const window = new BrowserWindow({
    show: false,
    width: 880,
    height: 600,
    webPreferences: { contextIsolation: false, sandbox: false },
  });
  void window.loadURL("about:blank");
});
