# Contracts between modules

| File | Written by | Read by |
|---|---|---|
| `job.json` | `evaluate` | `upgrade_` |
| `outcome.json` | `upgrade_` | `settle-in` |

These are the only interfaces between modules. Everything else is internal.

Both are **versioned**. A USB written by one release will eventually be read by
another — a module that meets a version it does not understand must **refuse,
not guess**. Guessing here means acting on a misunderstood instruction while
holding a partitioning tool.

Change these carefully. `data/` is meant to churn via drive-by PRs; this
directory is not.

Status: not yet written. First item in the build order.
