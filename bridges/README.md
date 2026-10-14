# Bridges

For installation and host behavior, follow the guides for [Maya](hosts/maya/README.md), [3ds Max](hosts/max/README.md), [Blender](hosts/blender/README.md), [Houdini](hosts/houdini/README.md), [Unity](hosts/unity/README.md), or [standalone Python](hosts/standalone_python/README.md).

Start the backend with `flint start` before connecting a Bridge. Confirm registration with `flint instances --json`, then follow the [execution guide](../README.md#execute-and-inspect) to submit code and inspect results. For command options, use `flint bridge export --help` or the relevant command's help.

## Connection settings

Installed host plugins connect when loaded. Their connection panels edit the backend address, port, and instance name. The address can be an IP address or domain name, including a remote backend.

Fields are drafts. **Apply** saves them for the next connection without changing the active one; **Connect** uses the saved settings. When saved and active settings differ, the panel notes that the changes take effect on the next connection. **Disconnect** stops the connection and automatic retries. The host guides show where to open each panel and how its plugin loads or unloads.
