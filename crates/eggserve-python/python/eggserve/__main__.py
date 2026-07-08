"""Allow running eggserve as a module: python -m eggserve."""

from eggserve._bin import main

raise SystemExit(main())
