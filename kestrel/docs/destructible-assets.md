# Destructible cover

The laser arena uses cover made from independently breakable pieces. A laser or projectile stops at the first solid piece it reaches. Each hit reduces that piece's health by one and darkens it; when health reaches zero, its geometry and collision disappear. Other pieces in the same bunker remain, so the opening follows the shot location. Broken pieces stay gone in saves and lockstep replays.

## Authoring a bunker

Add a `destructibles` entry to a `Scene(...)` RON file:

```ron
destructibles: [
    (position: (0.0, 0.75, -0.4), size: (1.8, 1.5, 1.8),
     material: "shadow", trim: Some("trim"), pieces: (2, 2), health: 3),
],
```

`position` is the center of the box, in scene meters. `size` gives its width, height, and depth. `material` and optional `trim` name materials in the scene style; the trim is a thin cap on top. `pieces` splits the box across its width and depth, so `(2, 2)` creates four independent pieces. `health` is the number of hits each piece survives. The laser arena gives some bunkers two hits per piece and others three.

The asset compiler bakes each piece as its own mesh and supplies a matching collision disc. The simulation keeps the pieces in a stable order and owns their health. Movement, navigation, projectiles, and beams all check only surviving pieces. The view reads that state to show or hide each mesh and its local hit target. This same piece and health infrastructure can serve other destructible assets; their geometry and damage response can be authored or extended separately.

The current recipe describes box cover. It does not fracture arbitrary meshes or create falling debris.
