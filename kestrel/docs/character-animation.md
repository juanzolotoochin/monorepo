# Character animation in Kestrel

`app/src/character.rs` owns the render rig and poses. `combat.rs` owns authoritative attack timing and collision. `gameplay.rs` owns input and navigation. The simulation remains the source of position, heading, and hit results; animation only changes child transforms.

## Rig contract

A character has a root transform and optional `Part` joints. Joints store their rest transform and a `PartKind`. The player rig uses Body -> Chest -> Head/Arm -> Forearm -> Hand -> WeaponGrip, with legs, shins, and feet below the root. Feet counter-rotate during stance to keep contact readable. A skin can supply voxel or smooth meshes to any joint. Missing skin parts use procedural geometry. Weapons attach under `WeaponGrip`, so motion at every ancestor carries them.

`ActorPart::Body` and `ActorPart::Chest` are separate skin slots. Existing skins that only provide Body still load. A skin may add Chest when it needs a deforming upper torso.

## Motion and actions

`GaitClock` advances pose phase and foot contacts from one style-defined step distance, keeping cadence and sound aligned. `MotionState` smooths movement speed and tracks heading with a damped angular spring, carrying turn momentum after the root changes direction. `Pose` uses that state to layer stance and swing, running posture, turning, breathing, airborne posture, digging, and an action family. `combat::Attack::motion()` maps simulation weapons to Sweep, Thrust, Draw, or Cast. Attack timing stays in combat; the character system blends the visual action into and out of locomotion. `blend_pose` interpolates joints in render time for local and replicated avatars.

Styles may set `character_motion` to tune arm and elbow drive, leg swing, step lift and distance, run crouch, turn inertia, torso turn, and joint response without editing animation code. The defaults apply to old styles. Add new joint kinds and action families in the engine, then author meshes and motion tuning in styles; scene setup contains only the fallback rig.

Build and test with `bazel test --config=full_local -c opt //kestrel/...`. Use `--scene moonlit-glade --motion-showcase --capture-dir <directory>` on the optimized demo for repeatable running and turning captures. `--showcase` checks the attack.
