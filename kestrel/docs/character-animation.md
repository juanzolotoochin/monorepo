# Character animation in Kestrel

`app/src/character.rs` owns the render rig and poses. `combat.rs` owns authoritative attack timing and collision. `gameplay.rs` owns input and navigation. The simulation remains the source of position, heading, and hit results; animation only changes child transforms.

## Rig contract

A character has a root transform and optional `Part` joints. Joints store their rest transform and a `PartKind`. The player rig uses Body -> Chest -> Head/Arm -> Forearm -> Hand -> WeaponGrip, with legs, knee skins, shins, and feet below the root. Feet counter-rotate during stance to keep contact readable. A skin can supply voxel or smooth meshes to any joint. Missing skin parts use procedural geometry. Weapons attach under `WeaponGrip`, so motion at every ancestor carries them.

`ActorPart::Body` and `ActorPart::Chest` are separate skin slots. Existing skins that only provide Body still load. A skin may add Chest when it needs a deforming upper torso.

## Motion and actions

`GaitClock` advances pose phase and foot contacts from one style-defined step distance, keeping cadence and sound aligned. `MotionState` smooths movement speed and tracks heading with a damped angular spring, carrying turn momentum after the root changes direction. `Pose` uses that state to layer stance and swing, running posture, turning, breathing, airborne posture, digging, and an action family. `combat::Attack::motion()` maps simulation weapons to Sweep, Thrust, Draw, or Cast. Attack timing stays in combat; the character system blends the visual action into and out of locomotion. `blend_pose` interpolates joints in render time for local and replicated avatars.

The `Jump` scene feature maps Space to a discrete simulation command. `JumpPhase` owns anticipation, launch velocity, gravity, and landing recovery in the simulation; the app derives `JumpMotion` from that state for both the local actor and peers. The pose compresses before launch, extends through a distinct foot push-off, uses a shallow asymmetric leg bend through the arc, then absorbs the landing through the torso and legs. Scenes without `Jump` keep their held jetpack control and flight behavior.

Sword swings carry a simulation-owned `SwordStroke`. Repeated presses cycle through forehand, backhand, and overhead, then reset to forehand after a pause. One press can be buffered during a swing. The `SwiftSword` scene feature selects the shorter sword cadence in both simulation and rendering, while other scenes retain their authored timing. The stroke drives combat sweep direction and the character's torso, shoulder, elbow, hand, and weapon grip; it is replicated with the swing state so remote actors show the same stroke. A thin blade afterimage emits from the posed weapon transform after hierarchy propagation, keeping it attached through those joint motions.

The launch menu's Character Studio opens the production rig in an in-place animation loop. It offers idle, run, run with direction changes, jump, run into jump, and each sword stroke, with three fixed camera angles, speed control, pause, and single-frame stepping. It uses the same `Pose` and rig as gameplay, while controlling the pose clock and camera separately for repeatable inspection. See the [studio preview](character-studio-preview.png).

Styles may set `character_motion` to tune arm and elbow drive, leg swing, step lift and distance, run crouch, turn inertia, torso turn, and joint response without editing animation code. The defaults apply to old styles. Add new joint kinds and action families in the engine, then author meshes and motion tuning in styles; scene setup contains only the fallback rig.

Build and test with `bazel test --config=full_local -c opt //kestrel/...`. Use `--scene moonlit-glade --motion-showcase --capture-dir <directory>` on the optimized demo for repeatable running and turning captures. `--scene moonlit-glade --jump-combo-showcase --capture-dir <directory>` captures the jump and three sword strokes. `--character-studio --studio-clip run-jump --studio-angle side --capture-character-studio <path>` captures a controlled studio pose. Add `--studio-capture-push` to capture the takeoff frame. `--showcase` checks the attack.
