"""Run the generated loader against a controlled platform entry and report what it received."""

import json
import sys
from types import ModuleType

print("Python interpreter:", sys.executable, file=sys.stderr, flush=True)

observed = {}
platform = ModuleType("flint_bridge")
entry = ModuleType("flint_bridge.attach")
entry.start = lambda request, error_path: observed.update(
    import_root=sys.path[0], request=json.loads(request), error_path=error_path
)
platform.attach = entry
sys.modules.update({platform.__name__: platform, entry.__name__: entry})
exec(compile(sys.argv[1], "<loader>", "exec"))
print(json.dumps(observed))
