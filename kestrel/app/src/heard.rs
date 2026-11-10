//! Ghouls heard, not seen (F6 of the Sacred Tree fog, `SACRED_TREE_FOG.md`;
//! M9.3): in the fog mode a ghoul out of the team's sight within `HEAR`
//! meters of any player's elder shows only its glowing eyes, through the
//! fog and through rock underground, and growls when it is first heard.
//! You know something is there, not what. Further off, nothing.
//!
//! The eyes are two small glowing marks drawn over the scene (UI, so no
//! ground hides them), where the ghoul's head is. `settlement` says which
//! ghouls are heard each frame ([`Heard::hear`]); a ghoul heard again after
//! `GROWL_AGAIN` seconds out of hearing growls again, and the growls of a
//! pack coming at once are one (`GROWL_GAP`).

use bevy::prelude::*;
use std::collections::BTreeMap;

/// How near an elder a ghoul out of sight is heard, meters.
pub const HEAR: f32 = 12.0;
/// How long a ghoul must have been out of hearing to growl again, seconds.
pub const GROWL_AGAIN: f32 = 8.0;
/// The least time between two growls, seconds.
pub const GROWL_GAP: f32 = 1.5;
/// The eyes' height over a ghoul's feet, meters.
pub const EYES: f32 = 1.45;
/// The eyes' color.
const GLOW: Color = Color::srgb(1.0, 0.32, 0.08);

/// The ghouls heard this frame, by id, and where their eyes are.
#[derive(Resource, Default)]
pub struct Heard {
    pub ghouls: Vec<(u64, Vec3)>,
    /// When each was last heard, seconds of the sim.
    last: BTreeMap<u64, f32>,
    /// When the last growl was.
    growled: Option<f32>,
}

impl Heard {
    /// The ghouls heard at `now` (seconds of the sim): their ids and eyes.
    /// Returns whether to growl: one was first heard (or again, after
    /// `GROWL_AGAIN` out of hearing), and no growl came within `GROWL_GAP`.
    pub fn hear(&mut self, ghouls: Vec<(u64, Vec3)>, now: f32) -> bool {
        let fresh = ghouls.iter().any(|(id, _)| self.last.get(id).is_none_or(|&t| now - t > GROWL_AGAIN));
        for (id, _) in &ghouls {
            self.last.insert(*id, now);
        }
        self.ghouls = ghouls;
        let growl = fresh && self.growled.is_none_or(|t| now - t >= GROWL_GAP);
        if growl {
            self.growled = Some(now);
        }
        growl
    }
}

/// The ghouls of `ghouls` (id, feet in the arena, and what the team knows
/// there) heard by `elders` (their feet): out of sight now and within
/// `HEAR` of one, with their eyes' place.
pub fn heard(ghouls: &[(u64, Vec3, crate::fog::State)], elders: &[Vec3]) -> Vec<(u64, Vec3)> {
    ghouls
        .iter()
        .filter(|(_, feet, known)| *known != crate::fog::State::Now && elders.iter().any(|e| e.distance(*feet) <= HEAR))
        .map(|&(id, feet, _)| (id, feet + Vec3::Y * EYES))
        .collect()
}

/// A heard ghoul's eyes on screen, by its id.
#[derive(Component)]
pub struct Eyes(u64);

/// Places the heard ghouls' eyes over the scene, making and dropping them
/// as ghouls are heard and not; they glow slowly.
pub fn draw(
    mut commands: Commands,
    heard: Option<Res<Heard>>,
    time: Res<Time>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut eyes: Query<(Entity, &Eyes, &mut Node, &mut Visibility)>,
    mut dots: Query<&mut BackgroundColor, With<EyeDot>>,
) {
    let ghouls = heard.as_ref().map_or(&[][..], |h| &h.ghouls[..]);
    let glow = 0.7 + 0.3 * (time.elapsed_secs() * 2.3).sin();
    for mut dot in &mut dots {
        dot.0 = GLOW.with_alpha(glow);
    }
    let mut drawn = Vec::new();
    for (entity, Eyes(id), mut node, mut visibility) in &mut eyes {
        let Some(&(_, at)) = ghouls.iter().find(|(g, _)| g == id) else {
            commands.entity(entity).despawn();
            continue;
        };
        drawn.push(*id);
        match camera.0.world_to_viewport(camera.1, at) {
            Ok(v) => {
                node.left = px(v.x - 12.0);
                node.top = px(v.y - 5.5);
                *visibility = Visibility::Inherited;
            }
            Err(_) => *visibility = Visibility::Hidden,
        }
    }
    for &(id, _) in ghouls.iter().filter(|(id, _)| !drawn.contains(id)) {
        commands
            .spawn((
                Eyes(id),
                service::WorldScoped::default(),
                // A faint glow round the pair, so it reads at the town's zoom.
                BackgroundColor(GLOW.with_alpha(0.16)),
                Node { position_type: PositionType::Absolute, column_gap: px(6.0), padding: UiRect::axes(px(3.0), px(3.0)), border_radius: BorderRadius::all(px(5.0)), ..default() },
                Visibility::Hidden,
            ))
            .with_children(|eyes| {
                for _ in 0..2 {
                    eyes.spawn((EyeDot, BackgroundColor(GLOW), Node { width: px(6.0), height: px(5.0), border_radius: BorderRadius::all(px(2.5)), ..default() }));
                }
            });
    }
}

/// One glowing eye.
#[derive(Component)]
pub struct EyeDot;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fog::State;

    #[test]
    fn a_ghoul_out_of_sight_is_heard_only_within_twelve_meters() {
        let elder = Vec3::ZERO;
        let ghouls = [
            (1, Vec3::new(11.0, 0.0, 0.0), State::Seen),
            (2, Vec3::new(13.0, 0.0, 0.0), State::Seen),
            (3, Vec3::new(0.0, 0.0, 5.0), State::Now),
            (4, Vec3::new(0.0, -4.0, 8.0), State::Never),
        ];
        let heard = heard(&ghouls, &[elder]);
        let ids: Vec<u64> = heard.iter().map(|(id, _)| *id).collect();
        // Within 12 m and out of sight (through rock too); not further, and
        // not one in sight (it is drawn).
        assert_eq!(ids, [1, 4]);
        assert_eq!(heard[0].1, Vec3::new(11.0, EYES, 0.0), "its eyes over its feet");
        // Any player's elder hears.
        let ids: Vec<u64> = super::heard(&ghouls, &[elder, Vec3::new(20.0, 0.0, 0.0)]).iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [1, 2, 4]);
        assert!(super::heard(&ghouls, &[]).is_empty(), "no elder, nothing heard");
    }

    #[test]
    fn a_ghoul_growls_when_first_heard() {
        let mut heard = Heard::default();
        let at = Vec3::ZERO;
        assert!(heard.hear(vec![(1, at)], 0.0), "first heard");
        assert!(!heard.hear(vec![(1, at)], 1.0), "still heard");
        assert!(heard.hear(vec![(1, at), (2, at)], 3.0), "a second ghoul");
        // A pack coming at once growls once (`GROWL_GAP`).
        assert!(!heard.hear(vec![(1, at), (2, at), (3, at)], 3.5));
        assert_eq!(heard.ghouls.len(), 3);
        // Out of hearing a moment, then back: no growl; after a long while
        // out of hearing, it growls again.
        assert!(!heard.hear(vec![], 4.0));
        assert!(!heard.hear(vec![(1, at)], 6.0), "back soon");
        assert!(!heard.hear(vec![], 7.0));
        assert!(heard.hear(vec![(1, at)], 7.0 + GROWL_AGAIN + 1.0), "back after a long while");
        // Nothing heard, nothing drawn.
        heard.hear(vec![], 30.0);
        assert!(heard.ghouls.is_empty());
    }
}
