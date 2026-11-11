//! A crawler's gun (TU.2 of the crawler turret, `kestrel/CRAWLER_TURRET.md`
//! T5, T7, T9): the town's ballista on the cab's roof, moving with the
//! crawler.
//!
//! A crawler whose definition has a `gun` (`CrawlerDef::gun`) gets a
//! `GunState` when a scene parks it (`Settlement::arm`): a turret (when it
//! may shoot next, where and when it last did) and an ammo store of its
//! ammo item only, up to its cap, which starts full (counted as supplied).
//! The gun's `defence` is resolved as a ballista kind's is
//! (`buildings::defence`), from the definition each time, so no rules are
//! copied into state.
//!
//! Shots and fireballs leave from the mount (`mount`): the gun's place on
//! the body, at the deck's height plus the gun's, where the crawler is on
//! the tick they go. They go through the ballista's own helpers
//! (`Settlement::pick_target`, `launch_shot`, `throw_fireball`):
//!
//! - A sprite at the gun (`SpriteState::gun`; seated by the crew job,
//!   `crew`, through `man_gun`) shoots the nearest threat in range
//!   on its own, once a period, as a staffed ballista does (`aim_guns`,
//!   `gun_fire`).
//! - An elder at the gun (`Seat::Gunner`) aims instead: its aim is a
//!   reticle offset from the mount, cut to the range, so the reticle moves
//!   with the crawler; `Attack` shoots there (`gunner_fire`), the Burst key
//!   throws the gun's fireball on the elder's own fireball cooldown, at no
//!   cost in ammo (`gun_burst`). Refusals are a ballista's
//!   (`FireRefusal`, `BurstRefusal`).
//!
//! Each bolt spends one unit of the store (spent, in the balances). An
//! empty gun never fires. An elder carrying its ammo restocks it by
//! interacting beside the crawler's box while the crawler stands still
//! (TU.3, T6: `InteractTarget::Gun`, a bundle delivery as at a ballista);
//! a walking crawler takes none and nothing comes back out. Everyone aboard is sheltered: nothing hits a
//! seated elder or a sprite at the gun.

use content::character::{CharacterDef, CrawlerDef, GunDef};
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::buildings::{self, BurstRefusal, Defence, FireRefusal, Turret};
use super::combat::Launcher;
use super::crawler::Pose;
use super::fire::{FireballDef, Mount};
use super::vehicle::{VehicleIx, VehicleState};
use super::{AgentIx, AgentState, Event, Inventory, InventoryRef, Kinds, Role, Seat, Settlement};
use crate::messages::{GunFire, GunnerFire, Messages};
use crate::nav::heading_vector;
use crate::terrain::Terrain;

/// A crawler's gun's state: its turret and its ammo store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GunState {
    pub turret: Turret,
    /// Its ammo item only, up to the gun's `ammo_cap`. An elder carrying
    /// it delivers by interacting beside the stopped crawler (TU.3,
    /// `interact`); nothing comes back out.
    pub store: Inventory,
}

/// A crawler's body and gun, if the character is a crawler with one.
pub fn gun_of(def: &CharacterDef) -> Option<(&CrawlerDef, &GunDef)> {
    let c = def.crawler.as_ref()?;
    Some((c, c.gun.as_ref()?))
}

/// Where vehicle `v`'s gun is now: its mount on the body, and its height
/// over the arena floor (the deck's, plus the gun's over it).
pub fn mount(v: &VehicleState, c: &CrawlerDef, gun: &GunDef) -> (FxVec2, Fx) {
    (Pose::of(v).at(gun.mount), v.height + c.deck + gun.height)
}

/// The range of vehicle `ix`'s gun, if it has one.
pub(super) fn range(vehicles: &[VehicleState], characters: &[CharacterDef], ix: VehicleIx) -> Option<Fx> {
    let v = vehicles.get(ix as usize)?;
    gun_of(&characters[v.character as usize]).map(|(_, gun)| gun.defence.range)
}

/// A gun with its state and ammo item: its defence, its mount and the
/// mount's height over the arena floor.
struct Armed {
    defence: Defence,
    from: FxVec2,
    top: Fx,
}

/// Where an elder at a gun's fireball goes (`Settlement::gun_burst_plan`).
pub(super) struct GunBurst {
    pub to: FxVec2,
    from: FxVec2,
    top: Fx,
    def: FireballDef,
}

impl Settlement {
    /// Gives a parked vehicle its gun's state, if its definition has a gun
    /// and the scenario its ammo item: a full store, counted as supplied.
    /// The Crawler Lab's crawler starts full this way (T6).
    pub(super) fn arm(&mut self, v: &mut VehicleState, def: &CharacterDef) {
        let Some((_, gun)) = gun_of(def) else { return };
        let Some(ammo) = self.item_index(&gun.defence.ammo) else { return };
        let cap = gun.defence.ammo_cap;
        let mut store = Inventory::new(self.items.len(), cap, Kinds::Only(ammo));
        let added = store.add(ammo, cap);
        debug_assert!(added, "the cap fits");
        self.accounts[ammo as usize].initial += cap as u64;
        v.gun = Some(GunState { turret: Turret::default(), store });
    }

    /// The sprite at vehicle `vehicle`'s gun, if one is.
    pub fn sprite_gunner_of(&self, vehicle: VehicleIx) -> Option<usize> {
        self.agents.iter().position(|a| a.sprite().is_some_and(|s| s.gun == Some(vehicle)))
    }

    /// Puts sprite `agent` at vehicle `vehicle`'s gun, if the gun is there
    /// and free (no elder or sprite at it) and the sprite isn't aboard: it
    /// gives up its slot, its errand and its plan, takes the crew job
    /// (`SpriteJob::Crew`, so it stays), and housekeeping puts it in the
    /// gunner's place. The crew job's stage 5 (`crew_guns`) seats sprites
    /// this way; tests call it directly. Returns whether it did.
    pub fn man_gun(&mut self, agent: AgentIx, vehicle: VehicleIx) -> bool {
        let Some(v) = self.vehicles.get(vehicle as usize) else { return false };
        let armed = gun_of(&self.characters[v.character as usize]).is_some();
        let free = self.gunner_of(vehicle).is_none() && self.sprite_gunner_of(vehicle).is_none();
        let sprite = self.agents.get(agent as usize).is_some_and(|a| a.body.alive() && a.sprite().is_some_and(|s| s.aboard().is_none()));
        if !armed || !free || !sprite {
            return false;
        }
        self.release_slot(agent);
        self.drop_sprite_errand(agent as usize);
        let AgentState { body, role: Role::Sprite(sprite), .. } = &mut self.agents[agent as usize] else { unreachable!("checked") };
        sprite.job = super::sprites::SpriteJob::Crew;
        sprite.plan.clear();
        sprite.step = 0;
        sprite.gun = Some(vehicle);
        body.route.clear();
        body.velocity = FxVec2::ZERO;
        body.push = FxVec2::ZERO;
        true
    }

    /// Vehicle `ix`'s gun with its state and its ammo item, where it is now.
    fn armed(&self, ix: VehicleIx) -> Option<Armed> {
        let v = self.vehicles.get(ix as usize)?;
        let (c, gun) = gun_of(&self.characters[v.character as usize])?;
        v.gun.as_ref()?;
        let ammo = self.item_index(&gun.defence.ammo)?;
        let (from, top) = mount(v, c, gun);
        Some(Armed { defence: buildings::defence(&gun.defence, ammo), from, top })
    }

    /// Vehicle `ix`'s gun's own shooting, when it can shoot on `tick`: a
    /// sprite at it, alive, no elder there, ammo in the store and its
    /// period over since the last shot, as `ready_to_shoot` for a ballista.
    fn sprite_ready(&self, ix: VehicleIx, tick: u64) -> Option<Armed> {
        let armed = self.armed(ix)?;
        let g = self.vehicles[ix as usize].gun.as_ref()?;
        let manned = self.sprite_gunner_of(ix).is_some_and(|a| self.agents[a].body.alive()) && self.gunner_of(ix).is_none();
        (manned && g.store.available(armed.defence.ammo) > 0 && g.turret.ready_at <= tick).then_some(armed)
    }

    /// Stage 3: every gun a sprite mans that can shoot now aims at the
    /// nearest threat in range of its mount that the shots in flight won't
    /// kill (`pick_target`), as a ballista does, on any floor in sight
    /// below the mount (`below_gun`).
    pub(super) fn aim_guns(&self, tick: u64, terrain: &Terrain, out: &mut Vec<(FxVec2, Messages)>) {
        for ix in 0..self.vehicles.len() as VehicleIx {
            let Some(armed) = self.sprite_ready(ix, tick) else { continue };
            if let Some(t) = self.pick_target(armed.from, armed.defence.range, Some(armed.top), terrain) {
                let mut messages = Messages::default();
                messages.gun_fire.push(GunFire { vehicle: ix, target: t.id, to: t.position });
                out.push((armed.from, messages));
            }
        }
    }

    /// Stage 5: a sprite's shot from vehicle `message.vehicle`'s gun, if it
    /// can still shoot (`sprite_ready`).
    pub(crate) fn gun_fire(&mut self, tick: u64, message: GunFire, terrain: &Terrain) {
        let Some(armed) = self.sprite_ready(message.vehicle, tick) else { return };
        self.shoot_gun(tick, message.vehicle, &armed, Some(message.target), message.to, terrain);
    }

    /// One bolt from vehicle `ix`'s gun, through the shared launch
    /// (`launch_shot`): a unit of ammo spent, the bolt from the mount (its
    /// height over the ground there), and the period to wait.
    fn shoot_gun(&mut self, tick: u64, ix: VehicleIx, armed: &Armed, target: Option<u64>, to: FxVec2, terrain: &Terrain) {
        let d = &armed.defence;
        let height = armed.top - terrain.height(armed.from);
        let launcher = Launcher { from: armed.from, height, damage: d.damage, speed: d.speed, kind: d.projectile, defender: None, top: Some(armed.top) };
        self.launch_shot(tick, &launcher, InventoryRef::Gun(ix), d.ammo, target, to, Event::GunShot { vehicle: ix });
        let turret = &mut self.vehicles[ix as usize].gun.as_mut().expect("armed").turret;
        turret.aim = Some(to);
        turret.ready_at = tick + d.period;
        turret.fired_at = tick;
    }

    /// Whether `agent` could shoot the gun it mans on `tick`: the vehicle
    /// and its gun, or why not. An empty store (or a gun with nothing to
    /// load) reads before a reload, as at the ballista.
    fn gunner_plan(&self, agent: AgentIx, tick: u64) -> Result<(VehicleIx, Armed), FireRefusal> {
        let Some(Seat::Gunner { vehicle }) = self.agents.get(agent as usize).and_then(|a| a.avatar()).and_then(|a| a.seat) else {
            return Err(FireRefusal::NotAtGun);
        };
        let Some(armed) = self.armed(vehicle) else { return Err(FireRefusal::NoAmmo) };
        let g = self.vehicles[vehicle as usize].gun.as_ref().expect("armed");
        if g.store.available(armed.defence.ammo) == 0 {
            return Err(FireRefusal::NoAmmo);
        }
        if g.turret.ready_at > tick {
            return Err(FireRefusal::Reloading { ticks: g.turret.ready_at - tick });
        }
        Ok((vehicle, armed))
    }

    /// Whether `agent` could shoot the crawler's gun it mans on `tick`, as
    /// stage 5 checks it (`gunner_fire`): for the view's fire key.
    pub fn gun_fire_check(&self, agent: usize, tick: u64) -> Result<VehicleIx, FireRefusal> {
        self.gunner_plan(agent as AgentIx, tick).map(|(v, _)| v)
    }

    /// Stage 5: elders' shots from crawlers' guns at their reticles, in
    /// command order, each if the elder can still shoot (`gunner_plan`):
    /// from where the mount is now to the mount plus the reticle, aimed at
    /// no threat in particular. A second on one tick, or one before the
    /// period is over, does nothing.
    pub(super) fn gunner_fire(&mut self, messages: Vec<GunnerFire>, tick: u64, terrain: &Terrain) {
        for GunnerFire { agent, vehicle, offset } in messages {
            let Ok((v, armed)) = self.gunner_plan(agent, tick) else { continue };
            if v != vehicle {
                continue;
            }
            let to = armed.from + offset;
            self.shoot_gun(tick, v, &armed, None, to, terrain);
        }
    }

    /// Where `agent`'s fireball from vehicle `vehicle`'s gun would go on
    /// `tick`: the reticle (half the range straight ahead before any aim),
    /// or why not: no fireball, or the elder's cooldown still running.
    pub(super) fn gun_burst_plan(&self, agent: AgentIx, vehicle: VehicleIx, tick: u64) -> Result<GunBurst, BurstRefusal> {
        let avatar = self.agents.get(agent as usize).and_then(|a| a.avatar()).ok_or(BurstRefusal::NotPossessing)?;
        let v = self.vehicles.get(vehicle as usize).ok_or(BurstRefusal::NotPossessing)?;
        let (c, gun) = gun_of(&self.characters[v.character as usize]).ok_or(BurstRefusal::NoBurst)?;
        // The fireball needs no ammo: resolved with any item.
        let def = buildings::defence(&gun.defence, 0).fireball.ok_or(BurstRefusal::NoBurst)?;
        if tick < avatar.fireball_ready {
            return Err(BurstRefusal::FireballCooling { ticks: avatar.fireball_ready - tick });
        }
        let range = gun.defence.range;
        let ahead = heading_vector(v.heading) * range.div_int(2);
        let offset = avatar.aim.and_then(|aim| buildings::reticle(aim, range)).unwrap_or(ahead);
        let (from, top) = mount(v, c, gun);
        Ok(GunBurst { to: from + offset, from, top, def })
    }

    /// Stage 5 (`bursts`): an elder at vehicle `vehicle`'s gun throws its
    /// fireball (`throw_fireball`), or the view hears why not.
    pub(super) fn gun_burst(&mut self, agent: AgentIx, vehicle: VehicleIx, tick: u64, terrain: &Terrain) {
        match self.gun_burst_plan(agent, vehicle, tick) {
            Ok(GunBurst { to, from, top, def }) => self.throw_fireball(tick, agent, Mount::Gun(vehicle), from, to, def, top - terrain.height(from)),
            Err(reason) => self.events.push(Event::BurstRefused { agent, reason }),
        }
    }

    /// Every gun's state belongs to a crawler with a gun: its store of the
    /// gun's ammo only, up to its cap, its reservations those of the
    /// deliveries on it and the units flying there; every sprite at a gun is alive, holds no slot, and is alone
    /// there, with no elder.
    pub(super) fn validate_guns(&self) -> Result<(), String> {
        for (i, v) in self.vehicles.iter().enumerate() {
            let Some(g) = &v.gun else { continue };
            let Some((_, gun)) = gun_of(&self.characters[v.character as usize]) else {
                return Err(format!("vehicle {i} has a gun's store but no gun"));
            };
            let ammo = self.item_index(&gun.defence.ammo).ok_or_else(|| format!("vehicle {i}'s gun has no ammo item"))?;
            g.store.validate().map_err(|e| format!("vehicle {i}'s gun store {e}"))?;
            if g.store.kinds != Kinds::Only(ammo) || g.store.capacity != gun.defence.ammo_cap {
                return Err(format!("vehicle {i}'s gun store takes {:?} up to {}, not its gun's", g.store.kinds, g.store.capacity));
            }
            // Deliveries (TU.3) reserve on it as on any holder.
            if (g.store.reserved_out.clone(), g.store.reserved_in.clone()) != self.expected_reservations(InventoryRef::Gun(i as VehicleIx)) {
                return Err(format!("vehicle {i}'s gun store reservations drifted"));
            }
        }
        let mut manned = std::collections::BTreeSet::new();
        for (i, a) in self.agents.iter().enumerate() {
            let Some(ix) = a.sprite().and_then(|s| s.gun) else { continue };
            let gunned = self.vehicles.get(ix as usize).is_some_and(|v| gun_of(&self.characters[v.character as usize]).is_some());
            if !gunned {
                return Err(format!("sprite {i} mans vehicle {ix}, which has no gun"));
            }
            if !a.body.alive() || a.sprite().is_some_and(|s| s.holds.is_some()) {
                return Err(format!("sprite {i} at vehicle {ix}'s gun is dead or holds a slot"));
            }
            if !manned.insert(ix) || self.gunner_of(ix).is_some() {
                return Err(format!("sprite {i} shares vehicle {ix}'s gun"));
            }
        }
        // Riding sprites (TU.4): alive, holding no slot, not at a gun too,
        // on a crawler in one of its places, alone there.
        let mut places = std::collections::BTreeSet::new();
        for (i, a) in self.agents.iter().enumerate() {
            let Some(sprite) = a.sprite() else { continue };
            let Some((ix, place)) = sprite.rider else { continue };
            if sprite.gun.is_some() || !a.body.alive() || sprite.holds.is_some() {
                return Err(format!("sprite {i} riding vehicle {ix} is at a gun too, dead or holds a slot"));
            }
            let crawler = self.vehicles.get(ix as usize).is_some_and(|v| self.characters[v.character as usize].crawler.is_some());
            if !crawler || place as usize >= super::crawler::RIDERS.len() {
                return Err(format!("sprite {i} rides vehicle {ix} in place {place}, which it hasn't"));
            }
            if !places.insert((ix, place)) || self.rider_of(ix, place).is_some() {
                return Err(format!("sprite {i} shares vehicle {ix}'s place {place}"));
            }
        }
        Ok(())
    }
}
