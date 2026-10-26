# Playable piano

The Asset Guide scene contains an upright piano. Run `bazel run --config=remote //kestrel/app:demo -- --scene asset-guide`, walk up to its keyboard, and play a connected MIDI input device. The prompt appears when the player is close enough. The piano listens to the first readable ALSA sequencer MIDI port on Linux and reconnects when one becomes available. The game still runs when no MIDI device is present.

Add a piano to any scene with `pianos: [(position: (0.0, 0.0, -4.0), yaw: 0.0)]`. Position is the cabinet's floor center in scene coordinates; its keyboard faces local positive Z. The cabinet blocks movement, while its keys and sound are local presentation. MIDI notes do not change the shared simulation.

The model shows two octaves of white and black keys. A MIDI note presses the key with the corresponding pitch class in those two octaves, including when the input is outside the displayed range. Its sound retains the original MIDI pitch and velocity. Note-off releases the voice. The audio is synthesized by the game's embedded sound bank, so the single game binary needs no sample files or runtime asset directory.
