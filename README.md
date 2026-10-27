# T6 Error Tracker

Tracks Black Ops II Zombies' script variables, animations, entities and server reset progress on Windows.

Start Plutonium T6 Zombies with `-lan` and host a local map, then open `t6-error-tracker.exe`. It attaches automatically. Server values are available when you host the game yourself. Run with `--help` for options.

## Reading the values

| Value | Meaning |
| --- | --- |
| `*_next` / `*_max` | Allocator index / highest index seen on this map. These aren't exact usage counts. |
| `anims_active`, `ents_in_use` | Animation and entity counts currently in use. |
| `ents_free`, `num_entities` | Reusable entity slots / size of the allocated entity pool. |
| `snapshot_ents` | Entities in the latest server snapshot. Compare this with the in-game overlay. |
| `svs.nextSnapshotEntities` | Cumulative server snapshot counter. |
| `svs.numSnapshotEntities` | Snapshot buffer capacity. |
| `snapshot reset remaining` | Allocations left before the snapshot entity reset threshold. Counts down toward zero. |
| `com_frameTime` | Engine clock in milliseconds. |

Numbers after `/` are limits. Values stay at the top; timestamped messages appear below. `--` means unavailable. Check **Status** and **Last sample** to see whether readings are current.

## About this fork

A vibe coded Rust rewrite of [lveez's BO2 Error Tracker](https://github.com/lveez/BO2-ERROR-TRACKER-V2), forked to fix reset tracking that used client-side counts instead of the global server counter.
