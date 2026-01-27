# simple_python fixture

Minimal Python project for context-footprint E2E tests.

## Regenerating the SCIP index

If you have [scip-python](https://github.com/sourcegraph/scip-python) installed:

```bash
cd tests/fixtures/simple_python
scip-python index . --output index.scip
```

Without `index.scip`, the E2E test that uses this fixture will skip (no failure).
