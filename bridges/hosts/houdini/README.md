# Houdini

Export `flint-houdini.zip` with `flint bridge export houdini`. Extract `flint.json` and the adjacent `flint` directory into `$HOUDINI_USER_PREF_DIR/packages`, then restart Houdini. The package connects when the UI is ready. Open **Flint > Connection Settings** for the shared [connection controls](../../README.md#connection-settings).

The adapter uses Houdini's Python runtime and runs submitted code on the UI thread. The execution namespace includes `hou`. This integration requires an interactive Houdini session; `hython` and Houdini Engine are outside its scope.

External attach uses the same adapter and requires no package installation. Use `flint hosts --help` and `flint attach --help` to discover and connect a running Houdini process.

For direct control from Houdini's Python shell:

```python
from flint_bridge.houdini import manager

manager.connect(address="127.0.0.1", port=6321, name="Houdini")
manager.disconnect()
```
