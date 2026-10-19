# Target-app parity: how it is measured

> **Status:** current · **Last updated:** 2026-10-19 · **Target:** Express Scribe Pro (public feature description, v5.x)

- The checklist is `parity/express-scribe-features.toml`, built from NCH's public product page
  and help pages only (ADR 0001). `cargo xtask parity` validates it and prints the numbers;
  `--partial` lists everything not yet shipped.
- `shipped` = works end to end and cites at least one passing test that exists (checked).
  It means "exists and tested", **not** "as good as Express Scribe".
- Weighting for the headline number: P0 counts 3, P1 2, P2 1, P3 0.5.

## Numbers (2026-10-19)
| Tier | Shipped / total |
|---|---|
| P0 | 8 / 16 |
| P1 | 9 / 15 |
| P2 | 0 / 11 |
| P3 | 0 / 3 |
| **All** | **17 / 45** |

Weighted: (8·3 + 9·2) / (16·3 + 15·2 + 11 + 1.5) = 42 / 90.5 ≈ **46 %** — but the missing P0s
(global hotkeys, pedals, DSS) are exactly what makes it usable for professional work, so the
"ready for real work" estimate is lower, about **25 %**.

## Revision history
- 2026-10-19 — first measurement.
