# Character animation in Kestrel

`app/src/character.rs` owns the render rig and poses. `combat.rs` owns authoritative attack timing and collision. `gameplay.rs` owns input and navigation. The simulation remains the source of position, heading, and hit results; animation only changes child transforms.

## Rig contract

A character has a root transform and optional `Part` joints. Joints store their rest transform and a `PartKind`. The player rig uses Body -> Chest -> Head/Arm -> Forearm -> Hand -> WeaponGrip, with legs and shins below the root. A skin can supply voxel or smooth meshes to any joint. Missing skin parts use procedural geometry. Weapons attach under `WeaponGrip`, so motion at every ancestor carries them.

`ActorPart::Body` and `ActorPart::Chest` are separate skin slots. Existing skins that only provide Body still load. A skin may add Chest when it needs a deforming upper torso.

## Motion and actions

`MotionState` smooths movement speed and heading response. `Pose` uses that state to layer gait, turning, breathing, airborne posture, digging, and an action family. `combat::Attack::motion()` maps simulation weapons to Sweep, Thrust, Draw, or Cast. Attack timing stays in combat; the character system blends the visual action into and out of locomotion. `blend_pose` interpolates joints in render time for local and replicated avatars.

Styles may set `character_motion` to tune arm swing, leg swing, torso turn, and joint response without editing animation code. The defaults apply to old styles. Add new joint kinds and action families in the engine, then author meshes and motion tuning in styles; scene setup contains only the fallback rig.

Build and test with `bazel test --config=full_local -c opt //kestrel/...`. Use `--scene moonlit-glade --showcase --capture-dir <directory>` on the optimized demo for a repeatable visual check.
