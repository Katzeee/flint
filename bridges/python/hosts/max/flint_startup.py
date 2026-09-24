"""Start the bundled Bridge from a 3ds Max post-startup script."""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import flint_max

flint_max.initialize()
