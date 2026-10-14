# Houdini

Export `flint-houdini.zip` with `flint bridge export houdini`. Extract `flint.json` and the adjacent `flint` directory into `$HOUDINI_USER_PREF_DIR/packages`, then restart Houdini. The package connects when the UI is ready. Open **Flint > Connection Settings** for the shared [connection controls](../../README.md#connection-settings).

Keep Houdini open and responsive while running Python code through Flint. Use `hou` to work with the current scene. Connecting through `hython` or Houdini Engine is not supported.

To connect without installing the package, use `flint hosts` to find Houdini's PID, then run `flint attach --pid <pid>`.

For direct control from Houdini's Python shell:

```python
from flint_bridge.houdini import manager

manager.connect(address="127.0.0.1", port=6321, name="Houdini")
manager.disconnect()
```
