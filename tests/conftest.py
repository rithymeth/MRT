import os
import sys

# Make `import mrt...` work regardless of the directory pytest is invoked from.
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
