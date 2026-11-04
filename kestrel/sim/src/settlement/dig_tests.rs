//! Digging in front in sloped scenes, and a town's stone (M6.2 of the
//! mining design): a dig takes the tile the digger faces, at most a layer
//! below its own, never under what stands on the ground; a stone layer drops
//! 3 to 6 stone on a ground pile beside the pit; and no run of allowed digs
//! leaves anyone without a walk back to undug ground. The town tests check
//! every invariant on every tick.

use std::collections::{BTreeSet, VecDeque};

use content::scenario::Scenario;
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::{AgentIx, SceneSetup, Settlement};
use crate::mine::{Veins, STONE_ID};
use crate::nav;
use crate::rng::Rng;
use crate::terrain::{Disc, DigRefusal, Terrain};
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
/// The elder's body radius (the exosuit's).
fn radius() -> Fx {
    Fx::from_ratio(8, 25)
}

/// The Sacred Tree's supply on a sloped, six-layer town mine from `seed`,
/// with no waves: what M6.3 makes the Sacred Tree.
fn town(seed: u64) -> Simulation {
    let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
    scenario.waves.clear();
    let setup = SceneSetup {
        scenario: Some(scenario),
        player: "elder".into(),
        // The tree's trunk, as the scene's architecture blocks it.
        obstacles: vec![Disc { center: FxVec2::ZERO, radius: Fx::ONE }],
        diggable: true,
        sloped: true,
        dig_layers: Some(6),
        mine: Some(seed),
        spawn: [Fx::ZERO, Fx::ZERO, Fx::from_ratio(17, 5)],
        ..SceneSetup::default()
    };
    Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

fn terrain(sim: &Simulation) -> &Terrain {
    sim.world().terrain()
}

/// Steps one tick with `turn`, checking every invariant after it.
fn step(sim: &mut Simulation, turn: &[Command]) {
    sim.step(turn);
    settlement(sim).validate().unwrap();
}

fn command(kind: CommandKind) -> Command {
    Command { player: PlayerId(0), kind }
}

/// The tile `x`, `z` tiles from the tree's.
fn tile(sim: &Simulation, x: i32, z: i32) -> (i32, i32) {
    let tree = Terrain::pit_tile(terrain(sim).center);
    (tree.0 + x, tree.1 + z)
}

/// The heading that faces along `(x, y)`.
fn facing(x: i32, y: i32) -> Fx {
    nav::bearing(FxVec2::ZERO, FxVec2::new(Fx::from_int(x), Fx::from_int(y)))
}

/// Walks the elder to `target` and waits until it stands there.
fn walk_to(sim: &mut Simulation, target: FxVec2) {
    if nav::distance(settlement(sim).agents[ELDER as usize].body.position, target) < Fx::from_ratio(1, 20) {
        return;
    }
    step(sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target, run: false } })]);
    assert!(!settlement(sim).agents[ELDER as usize].body.route.is_empty(), "no route to {target:?}");
    for _ in 0..20 * HZ {
        let a = &settlement(sim).agents[ELDER as usize];
        if a.body.route.is_empty() && a.avatar().unwrap().intent == Some(Intent::Stop) {
            return;
        }
        step(sim, &[]);
    }
    panic!("never got to {target:?}");
}

/// Walks the elder onto the middle of `on`, its last tile `toward` (a
/// step along x or y), so it faces the next tile that way.
fn stand(sim: &mut Simulation, on: (i32, i32), toward: (i32, i32)) {
    walk_to(sim, Terrain::pit_center((on.0 - toward.0, on.1 - toward.1)));
    walk_to(sim, Terrain::pit_center(on));
    assert_eq!(Terrain::pit_tile(settlement(sim).agents[ELDER as usize].body.position), on);
}

/// Sends the dig command, returning the tile the dig started on, and waits
/// for it to finish.
fn dig(sim: &mut Simulation) -> Option<(i32, i32)> {
    step(sim, &[command(CommandKind::DigDown { agent: ELDER })]);
    let dig = settlement(sim).agents[ELDER as usize].avatar().unwrap().dig.clone()?;
    while settlement(sim).agents[ELDER as usize].avatar().unwrap().dig.is_some() {
        step(sim, &[]);
    }
    assert!(dig.until - dig.started <= 2 * HZ + 1, "about 2 s a layer");
    Some(dig.tile)
}

/// The ground piles of stone: position and units.
fn stone_piles(sim: &Simulation) -> Vec<(FxVec2, u32)> {
    let s = settlement(sim);
    let stone = s.item_index(STONE_ID).unwrap();
    s.piles.as_ref().unwrap().piles.values().filter(|p| p.item() == Some(stone)).map(|p| (p.position, p.units())).collect()
}

/// The stone dug out of the ground so far.
fn dug_stone(sim: &Simulation) -> u64 {
    let s = settlement(sim);
    s.accounts[s.item_index(STONE_ID).unwrap() as usize].generated
}

/// Whether the top layer of `tile` is stone.
fn stone_on_top(sim: &Simulation, tile: (i32, i32)) -> bool {
    terrain(sim).mine().unwrap().ore(tile, terrain(sim).pit(tile)).is_some()
}

/// A tile east of the tree whose two tiles east have stone on top, the
/// first of which the elder can dig from it; and the seed it takes.
fn stone_row() -> (u64, (i32, i32)) {
    for seed in 1..64 {
        let sim = town(seed);
        let s = settlement(&sim);
        let view = s.view(ELDER, 0, terrain(&sim));
        for x in 3..5 {
            for z in -5..6 {
                let from = tile(&sim, x, z);
                let row = (1..=2).map(|k| (from.0 + k, from.1));
                if row.clone().all(|t| stone_on_top(&sim, t)) && view.dig_in_front(Terrain::pit_center(from), facing(1, 0), radius()) == Ok((from.0 + 1, from.1)) {
                    return (seed, from);
                }
            }
        }
    }
    panic!("no seed has a row of stone to dig");
}

/// The elder digs the tile it faces, never its own, and only down to a
/// layer below its own; standing in the pit, it digs the next tile a layer
/// deeper. A stone layer drops 3 to 6 stone on a ground pile beside the
/// pit, the next onto the same pile, and every unit is accounted for.
#[test]
fn the_elder_digs_stone_in_front_onto_a_pile() {
    let (seed, from) = stone_row();
    let mut sim = town(seed);
    let (a, b) = ((from.0 + 1, from.1), (from.0 + 2, from.1));
    let mine = terrain(&sim).mine().unwrap();
    let (first, second) = (mine.yield_of(a, 0), mine.yield_of(b, 0));
    assert!((3..=6).contains(&first) && (3..=6).contains(&second));

    stand(&mut sim, from, (1, 0));
    assert_eq!(dig(&mut sim), Some(a), "the tile in front");
    assert_eq!(terrain(&sim).pit(a), 1);
    assert_eq!(terrain(&sim).pit(from), 0, "never its own");
    let piles = stone_piles(&sim);
    assert_eq!(piles.len(), 1);
    let (pile, units) = piles[0];
    assert_eq!(units, first);
    assert_eq!(dug_stone(&sim), first as u64);
    assert!(nav::distance(pile, Terrain::pit_center(a)) < Fx::from_ratio(3, 2), "beside the pit: {pile:?}");
    assert_ne!(Terrain::pit_tile(pile), a);

    // As deep as it goes from here: a layer below the elder's tile.
    assert_eq!(dig(&mut sim), None, "too deep");
    assert_eq!(terrain(&sim).pit(a), 1);
    let s = settlement(&sim);
    let elder = &s.agents[ELDER as usize].body;
    assert_eq!(s.view(ELDER, sim.world().tick(), terrain(&sim)).dig_in_front(elder.position, elder.heading, radius()), Err(DigRefusal::TooDeep));

    // From the pit's floor, the next tile goes a layer deeper, and its stone
    // lands on the same pile.
    stand(&mut sim, a, (1, 0));
    assert_eq!(dig(&mut sim), Some(b));
    assert_eq!(stone_piles(&sim), vec![(pile, first + second)], "onto the pile already there");
    let earth = terrain(&sim).mine().unwrap().ore(b, 1).is_none();
    assert_eq!(dig(&mut sim), Some(b));
    assert_eq!(terrain(&sim).pit(b), 2);
    if earth {
        assert_eq!(dug_stone(&sim), (first + second) as u64, "earth yields nothing");
    }
    assert_eq!(dig(&mut sim), None, "a layer below the pit's floor at most");

    // It walks back up and out, and the balances hold.
    let out = (from.0 - 1, from.1);
    walk_to(&mut sim, Terrain::pit_center(out));
    assert_eq!(Terrain::pit_tile(settlement(&sim).agents[ELDER as usize].body.position), out);
    assert_eq!(terrain(&sim).pit(out), 0);
    let stone = &settlement(&sim).balances()[STONE_ID];
    assert!(stone.conserved && stone.stored >= dug_stone(&sim));
}

/// The dug stone is picked up as a bundle, as any ground pile's units are.
#[test]
fn dug_stone_is_carried_from_its_pile() {
    use super::interact::InteractTarget;
    let (seed, from) = stone_row();
    let mut sim = town(seed);
    stand(&mut sim, from, (1, 0));
    dig(&mut sim).unwrap();
    let (id, pile) = settlement(&sim).piles.as_ref().unwrap().piles.iter().map(|(&id, p)| (id, p.position)).next().unwrap();
    walk_to(&mut sim, pile + (Terrain::pit_center(from) - pile).normalize_or_zero() * Fx::from_ratio(3, 5));
    step(&mut sim, &[command(CommandKind::Interact { agent: ELDER, target: InteractTarget::Pile(id) })]);
    for _ in 0..10 * HZ {
        step(&mut sim, &[]);
    }
    let s = settlement(&sim);
    assert!(s.agents[ELDER as usize].carry.inventory.count(s.item_index(STONE_ID).unwrap()) > 0, "the elder carries stone");
}

/// Digs that the rules refuse: under the tree, a building, a tree pile's
/// dock, a ground pile or undiggable ground, more than a layer below the
/// digger, past the deepest layer, or the digger's own tile.
#[test]
fn digs_under_things_or_too_deep_are_refused() {
    let (seed, from) = stone_row();
    let mut sim = town(seed);
    let at = |sim: &Simulation, x: i32, z: i32| Terrain::pit_center(tile(sim, x, z));
    {
        let s = settlement(&sim);
        let view = s.view(ELDER, 0, terrain(&sim));
        // The trunk, of radius 1 at the tree.
        assert_eq!(view.dig_in_front(at(&sim, 2, 0), facing(-1, 0), radius()), Err(DigRefusal::Blocked));
        // The Wood Gatherer, 0.9 across at (-5.2, 0.6).
        assert_eq!(view.dig_in_front(at(&sim, -3, 1), facing(-1, 0), radius()), Err(DigRefusal::Blocked));
        // The tree's wood pile, its dock at (-1.7, 1.7).
        assert_eq!(view.dig_in_front(at(&sim, -3, 2), facing(1, 0), radius()), Err(DigRefusal::Pile));
        // Open ground east of the tree.
        assert_eq!(view.dig_in_front(at(&sim, 5, 4), facing(1, 0), radius()), Ok(tile(&sim, 6, 4)));
        assert_eq!(view.dig_in_front(at(&sim, 5, 4), facing(0, -1), radius()), Ok(tile(&sim, 5, 3)));

        // Undiggable ground (M6.3's hill).
        let mut hill = terrain(&sim).clone();
        hill.undiggable.push((tile(&sim, 6, 3), tile(&sim, 8, 5)));
        let view = s.view(ELDER, 0, &hill);
        assert_eq!(view.dig_in_front(at(&sim, 5, 4), facing(1, 0), radius()), Err(DigRefusal::Undiggable));
        assert_eq!(view.dig_in_front(at(&sim, 5, 4), facing(0, 1), radius()), Ok(tile(&sim, 5, 5)));

        // A scene that isn't diggable, the last layer, the digger's own tile.
        let mut ground = terrain(&sim).clone();
        let (own, front) = (tile(&sim, 5, 4), tile(&sim, 6, 4));
        assert_eq!(ground.dig_refusal(own, own, radius()), Some(DigRefusal::OwnTile));
        ground.max_layers = 1;
        ground.deepen(front);
        assert_eq!(ground.dig_refusal(own, front, radius()), Some(DigRefusal::Bottom));
        ground.diggable = false;
        assert_eq!(ground.dig_refusal(own, (front.0, front.1 + 1), radius()), Some(DigRefusal::Undiggable));
    }

    // Under a ground pile: the one the dug stone made.
    stand(&mut sim, from, (1, 0));
    dig(&mut sim).unwrap();
    let pile = Terrain::pit_tile(stone_piles(&sim)[0].0);
    let s = settlement(&sim);
    let view = s.view(ELDER, sim.world().tick(), terrain(&sim));
    let mut sides = 0;
    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let beside = (pile.0 - dx, pile.1 - dy);
        if terrain(&sim).pit(beside) == 0 {
            assert_eq!(view.dig_in_front(Terrain::pit_center(beside), facing(dx, dy), radius()), Err(DigRefusal::Pile));
            sides += 1;
        }
    }
    assert!(sides >= 2);
}

/// Whether a body standing at `p` can walk to undug ground: the middle of
/// a tile with no pit, not too steep. A search over a half-tile lattice
/// within `box_` (the first and last tile on both axes), walking only
/// where `Terrain::walkable` allows it.
fn escapes(terrain: &Terrain, p: FxVec2, box_: ((i32, i32), (i32, i32))) -> bool {
    let r = radius();
    let ((x0, y0), (x1, y1)) = box_;
    let (w, h) = ((x1 - x0) * 2 + 1, (y1 - y0) * 2 + 1);
    let point = |i: i32, j: i32| FxVec2::new(Fx::from_int(x0) + Fx::HALF.mul_int(i as i64), Fx::from_int(y0) + Fx::HALF.mul_int(j as i64));
    let undug = |i: i32, j: i32| {
        let tile = (x0 + i / 2, y0 + j / 2);
        i % 2 == 0 && j % 2 == 0 && terrain.pit(tile) == 0 && !terrain.steep(tile)
    };
    let mut seen = BTreeSet::new();
    let mut open = VecDeque::new();
    let near = Terrain::pit_tile(p);
    for i in ((near.0 - x0) * 2 - 2)..=((near.0 - x0) * 2 + 2) {
        for j in ((near.1 - y0) * 2 - 2)..=((near.1 - y0) * 2 + 2) {
            if i >= 0 && j >= 0 && i < w && j < h && terrain.walkable(p, point(i, j), r) && seen.insert((i, j)) {
                open.push_back((i, j));
            }
        }
    }
    while let Some((i, j)) = open.pop_front() {
        if undug(i, j) {
            return true;
        }
        for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let (ni, nj) = (i + di, j + dj);
            if ni >= 0 && nj >= 0 && ni < w && nj < h && !seen.contains(&(ni, nj)) && terrain.walkable(point(i, j), point(ni, nj), r) {
                seen.insert((ni, nj));
                open.push_back((ni, nj));
            }
        }
    }
    false
}

/// A digger walks and digs at random in the 7 × 7 tiles around `terrain`'s
/// center, `moves` times; the dig rules (`Terrain::dig_refusal`) decide
/// what it may dig. After every dig, its own tile is no steeper than it
/// was, and it, and a walker on every spot it ever stood on, can still
/// walk to undug ground. Returns how many digs it made and how deep the
/// deepest pit went.
fn dig_at_random(mut terrain: Terrain, seed: u64, moves: u32) -> (u32, u8) {
    let r = radius();
    let middle = Terrain::pit_tile(terrain.center);
    let region = |t: (i32, i32)| (t.0 - middle.0).abs() <= 3 && (t.1 - middle.1).abs() <= 3;
    let box_ = ((middle.0 - 6, middle.1 - 6), (middle.0 + 6, middle.1 + 6));
    let mut rng = Rng::new(seed, 0, 0, 0);
    let mut at = Terrain::pit_center(middle);
    let mut stood = vec![at];
    let (mut digs, mut deepest) = (0, 0);
    for _ in 0..moves {
        if rng.chance(1, 2) {
            // Walk somewhere in the region, anywhere in a tile.
            let goal = Terrain::pit_center((middle.0 + rng.below(7) as i32 - 3, middle.1 + rng.below(7) as i32 - 3))
                + FxVec2::new(rng.range(-Fx::from_ratio(2, 5), Fx::from_ratio(2, 5)), rng.range(-Fx::from_ratio(2, 5), Fx::from_ratio(2, 5)));
            if !nav::path(&terrain, at, goal, r).is_empty() {
                at = goal;
                stood.push(at);
            }
            continue;
        }
        let forward = [(1, 0), (-1, 0), (0, 1), (0, -1)][rng.below(4) as usize];
        let tile = Terrain::front_tile(at, FxVec2::new(Fx::from_int(forward.0), Fx::from_int(forward.1)));
        let own = Terrain::pit_tile(at);
        if !region(tile) || terrain.dig_refusal(own, tile, radius()).is_some() {
            continue;
        }
        let steep = terrain.steep(own);
        assert!(terrain.deepen(tile));
        digs += 1;
        deepest = deepest.max(terrain.pit(tile));
        assert!(steep || !terrain.steep(own), "dig {digs} at {tile:?} made the digger's tile {own:?} steep");
        assert!(escapes(&terrain, at, box_), "dig {digs} at {tile:?} trapped the digger at {at:?}: {:?}", terrain.pits);
        for &p in &stood {
            assert!(escapes(&terrain, p, box_), "dig {digs} at {tile:?} trapped a walker at {p:?}: {:?}", terrain.pits);
        }
    }
    (digs, deepest)
}

/// A sloped, diggable, six-layer arena of level ground.
fn sloped_ground() -> Terrain {
    let mut terrain = Terrain::flat(FxVec2::ZERO, Fx::from_int(8));
    (terrain.sloped, terrain.diggable, terrain.max_layers) = (true, true, 6);
    terrain
}

/// No run of allowed digs leaves anyone without a walk to undug ground:
/// not the digger, nor anyone standing where it stood, though digs beside
/// them make their ground steep. On level ground and on a mine's terraces.
#[test]
fn no_allowed_digs_trap_a_walker() {
    let sloped = sloped_ground;
    for seed in 1..=3 {
        let (digs, deepest) = dig_at_random(sloped(), seed, 300);
        assert!(digs > 40 && deepest >= 2, "seed {seed}: {digs} digs, {deepest} deep");
        let mut hills = sloped();
        // The landing far off, so terraces rise around the digger.
        hills.generate_mine(seed, FxVec2::new(Fx::from_int(40), Fx::from_int(40)), Veins::Stone);
        let (digs, _) = dig_at_random(hills, seed, 300);
        assert!(digs > 40, "seed {seed}: {digs} digs on terraces");
    }
}

/// A body on ground a dig made steep may walk off it, downhill or along
/// it, but not climb it; from gentle ground, no walk goes onto it.
#[test]
fn a_body_on_steep_ground_may_walk_off_it() {
    let r = radius();
    let mut terrain = sloped_ground();
    // A stair down along +x: tiles 1 and 2 a layer and two deep. Tile
    // (1, 1) touches the two-layer pit only at a corner, and its edge
    // there falls a meter: too steep.
    terrain.deepen((1, 0));
    terrain.deepen((2, 0));
    terrain.deepen((2, 0));
    assert!(terrain.steep((1, 1)));
    let on = FxVec2::new(Fx::ONE, Fx::ONE);
    let p = |x: i32, y: i32| FxVec2::new(Fx::from_int(x), Fx::from_int(y));
    // Off it: along the slope away from the pit (rising a quarter layer
    // per tile), or down into the pit.
    assert!(terrain.walkable(on, p(-2, 1), r));
    assert!(terrain.walkable(on, p(1, 0), r));
    assert!(!nav::path(&terrain, on, p(-3, 3), r).is_empty());
    // Not up its steep side, nor back onto it from gentle ground.
    assert!(!terrain.walkable(on, p(1, 2), r));
    assert!(!terrain.walkable(p(-2, 1), on, r));
    assert!(!terrain.walkable(p(1, 0), on, r));
}
