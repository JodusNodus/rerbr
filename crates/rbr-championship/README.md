# rbr-championship

The 1.02 championship (flow mode 3) around the stage loop of [`rbr-gameplay`](../rbr-gameplay), pure. The launcher
owns the season and runs each stage through `rbr-standalone --championship`. Behaviour and evidence:
[notes/gameplay.md](../../notes/gameplay.md#championship-and-service). Each module's doc comment names the functions
it ports.

| Module | What it does |
| --- | --- |
| `rally` | A rally's stages, end-of-stage handling, carried damage and tyres; the stage report `rbr-standalone` leaves |
| `service/` | Service park: budget, part tree, orders, repairs, late penalty |
| `season/` | Season creation, teams, forecast weather, points |
| `season/layout` | Named view over the season save: position, clock, ranks, results, orders; the save-layout adapter |
| `events` | Season clock and event queue, AI stages and services, player turns |
| `store` | Season in the profile's `RBR7` block |
| `ai/` | AI driver's stage and service on `rand()` |

## Tests

Unit tests in each module; the retail ones need `RBR_DATA` (and `RBR_PATCH`, `RBR_NGP_PHYSICS`, `RBR_SAVES` where
named) and print "skipping" without them.
