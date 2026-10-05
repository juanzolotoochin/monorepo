"""Report the actual runtime and a declared wheel import for launcher tests."""
import json
from pathlib import Path
import sys

import fontTools

print(json.dumps({
    'version': list(sys.version_info[:2]),
    'interpreter': str(Path(sys.executable).resolve()),
    'fonttools': fontTools.__version__,
}))
