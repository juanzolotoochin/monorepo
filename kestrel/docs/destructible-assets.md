# Destructible cover

The laser arena uses cover made from independently breakable pieces. A laser or projectile stops at the first solid piece it reaches. Each hit reduces that piece's health by one and darkens it. On the final hit, the collision opens immediately while the piece tumbles or shatters into fragments and a dust ring spreads from the break. Fragments bounce, rest on the floor for a few seconds, then fade away. Other pieces in the same bunker remain, so the opening follows the shot location. Broken pieces stay gone in saves and lockstep replays.

## Authoring a bunker

Add a `destructibles` entry to a `Scene(...)` RON file:

```ron
destructibles: [
    (position: (0.0, 0.75, -0.4), size: (1.8, 1.5, 1.8),
     material: "shadow", trim: Some("trim"), pieces: (2, 2), health: 3,
     form: Stepped, break_style: Shatter),
],
```

`position` is the center of the cover, in scene meters. `size` gives its width, height, and depth. `material` and optional `trim` name materials in the scene style. `pieces` splits the cover across its width and depth, so `(2, 2)` creates four independent pieces. `health` is the number of hits each piece survives. The laser arena gives some bunkers two hits per piece and others three.

`form` controls the authored shape of each piece. `Armored` has a narrow shell between an overhanging plinth and lid, with a thin face band; `Pylon` uses a round base and shaft under a tapered crown; `Stepped` has a lower base and narrower upper tier. `break_style` controls the visual response: `Topple` launches the intact piece and several chips, while `Shatter` breaks the piece down as more fragments scatter. Both choices share collision, health, save, and replay behavior. The laser arena combines metal bunkers and pylons with stepped dark cover.

The asset compiler bakes each piece as its own mesh and supplies a matching collision disc. The simulation keeps the pieces in a stable order and owns their health. Movement, navigation, projectiles, and beams all check only surviving pieces. The view reads that state to launch visual fragments; the fragments have no collision. A loaded save starts with the correct pieces visible, without replaying their old destruction animations. This same piece and health infrastructure can serve other destructible assets; their geometry and damage response can be authored or extended separately.

The current recipe describes segmented cover with three authored forms. It does not yet fracture arbitrary meshes into exact geometric shards.
