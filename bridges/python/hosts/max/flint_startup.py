"""Start the bundled Bridge from a 3ds Max post-startup script."""
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from flint_bridge import connect

port = int(os.environ.get("FLINT_MAX_REGISTRY_PORT", "6321"))
connect(host="max", name="3ds Max", port=port)
