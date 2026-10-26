# Characters

## Done

### Remove the right-click movement leftovers

- **Status:** done (branch `kestrel-ability-prediction`)
- **Why:** right-click path movement was replaced by explicit WASD movement
  in `50cd1a3`, but its drawing and comments remain. GoTo itself stays: the
  depot transfer's walk to the dock, pile jobs, the automation `Walk`
  command and the character preview still use it. Do this first when the
  step 5 ability prediction work starts
  (`kestrel/ABILITY_PREDICTION_HANDOFF.md`).
- **What to do:**
  - Remove `Controls::route` and the waypoint gizmo that draws it.
  - Update the character preview shot that sets it, and the automation's
    movement readout.
  - Fix the comments that say click-to-move "is going away": GoTo stays,
    only the player's right-click input went.
- **Where:** `kestrel/app/src/gameplay.rs` (`Controls::route`, the gizmo
  around line 89); `kestrel/app/src/preview.rs:68`;
  `kestrel/app/src/control/mod.rs:623`; comments at
  `kestrel/app/src/lockstep.rs:1434` and `kestrel/app/src/prediction.rs:230`.
- **Found in:** design session for step 5 ability prediction (2026-10-26).
