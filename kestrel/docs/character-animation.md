# Character animation in Kestrel

`app/src/character.rs` owns the render rig and poses. `combat.rs` owns authoritative attack timing and collision. `gameplay.rs` owns input and navigation. The simulation remains the source of position, heading, and hit results; animation only changes child transforms.

## Rig contract

A character has a root transform and optional `Part` joints. Joints store their rest transform and a `PartKind`. The player rig uses Body -> Chest -> Head/Arm -> Forearm -> Hand -> WeaponGrip, with legs, knee skins, shins, and feet below the root. Feet counter-rotate during stance to keep contact readable. A skin can supply voxel or smooth meshes to any joint. Missing skin parts use procedural geometry. Weapons attach under `WeaponGrip`, so motion at every ancestor carries them.

`ActorPart::Body` and `ActorPart::Chest` are separate skin slots. Existing skins that only provide Body still load. A skin may add Chest when it needs a deforming upper torso.

## Independent upper and lower motion

The laser fighter shows why the rig has separate branches. The Body and leg joints keep the locomotion pose while the Chest, arms, hands, and weapon turn toward the aim. A laser attack changes the upper branch without restarting or replacing the lower branch's step. Tests cover both aiming without reposing the legs and firing while their walking support pose stays unchanged.

The laser scene's style currently authors its weapon mesh and muzzle, sets natural motion, and enables planted feet. The rest of the player body uses procedural fallback meshes. A style may also author Body, Chest, and leg parts; partial styles are valid because each part can use fallback geometry. The [cube-to-character walkthrough](asset-walkthrough.md) builds those parts from reusable shapes.

## Motion and actions

Kestrel currently has **procedural animation**, but no authored keyframe clip format or playback path. For example, breathing is sampled from a sine wave over time, while walking uses travel distance and stance rules to pose the legs. The Studio's named clips and numbered frames are repeatable samples of these functions, not stored poses. `motion: Biped` and `motion: CubeRoll(edge: ...)` select engine pose rules; they do not point to keyframe files. Supporting authored keyframes would require a clip schema, a sampler for joint transforms, and rules for blending sampled poses with locomotion and combat.

`GaitClock` advances pose phase and foot contacts from one style-defined step distance, keeping cadence and sound aligned. `MotionState` smooths movement speed and tracks heading with a damped angular spring, carrying turn momentum after the root changes direction. `Pose` uses that state to layer stance and swing, running posture, turning, breathing, airborne posture, digging, and an action family. `combat::Attack::motion()` maps simulation weapons to Sweep, Thrust, Draw, or Cast. Attack timing stays in combat; the character system blends the visual action into and out of locomotion. `blend_pose` interpolates joints in render time for local and replicated avatars.

The `Jump` scene feature maps Space to a discrete simulation command. `JumpPhase` owns anticipation, launch velocity, gravity, and landing recovery in the simulation; the app derives `JumpMotion` from that state for both the local actor and peers. The pose compresses before launch, extends through a distinct foot push-off, uses a shallow asymmetric leg bend through the arc, then absorbs the landing through the torso and legs. Scenes without `Jump` keep their held jetpack control and flight behavior.

Sword swings carry a simulation-owned `SwordStroke`. Repeated presses cycle through forehand, backhand, and overhead, then reset to forehand after a pause. One press can be buffered during a swing. The `SwiftSword` scene feature selects the shorter sword cadence in both simulation and rendering, while other scenes retain their authored timing. The stroke drives combat sweep direction and the character's torso, shoulder, elbow, hand, and weapon grip; it is replicated with the swing state so remote actors show the same stroke. A thin blade afterimage emits from the posed weapon transform after hierarchy propagation, keeping it attached through those joint motions.

The launch menu's Asset Studio discovers player actors and animated biped, quadruped, and cube rolling skins from baked scene packs. Each style can name its actor and primary weapon. Player actors keep the complete procedural rig even when a style replaces only some meshes. Animated skins use the shared rig pose path from the settlement view.

Studio lists characters for direct selection, including NPCs such as the ox and ghouls. It offers sword strokes for sword actors and a weapon action clip for other armed actors, using the same attack pose family as gameplay. The pose clock and camera allow repeatable inspection: clips have fixed 60 Hz numbered frames, three camera angles, speed control, pause, and single-frame stepping.

Those frames sample deterministic procedural pose functions; they are not stored authored keyframe assets. The side view shows a blade angle when the selected actor carries a sword. See the [studio preview](character-studio-preview.png).

Styles may set `character_motion` to tune arm and elbow drive, leg swing, step lift and distance, run crouch, turn inertia, torso turn, and joint response without editing animation code. `motion_style` chooses the procedural motion family; `plant_feet` enables planted-foot IK and `leg_reach` defines that rig's reach. Defaults cover styles that omit these fields. Joint kinds and action families are engine code; meshes and motion tuning are style data. Scene setup contains the fallback rig.

Build and test with `bazel test -c opt //kestrel/app:movement_test //kestrel/sim:sim_test //kestrel/tools/assetc/core:core_test`. Use `--scene moonlit-glade --motion-showcase --capture-dir <directory>` on the optimized demo for repeatable running and turning captures. `--scene moonlit-glade --jump-combo-showcase --capture-dir <directory>` captures the jump and three sword strokes. `--character-studio --studio-character <style-id-or-skin:id> --studio-clip run --studio-angle side --capture-character-studio <path>` captures a selected character's controlled pose. For example, use `--studio-character skin:ox` or `--studio-character skin:ghoul`; omit the option to use the first discovered actor. Add `--studio-capture-push` to capture the takeoff frame. `--showcase` checks the attack.
