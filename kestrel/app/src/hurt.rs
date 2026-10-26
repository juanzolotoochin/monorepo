//! A short red flash at the screen's edges when the local player is hurt,
//! by anything: a laser, a ghoul. It reads the confirmed health of the
//! local avatar (`WorldMirror::current`), since health is stage-5 state
//! the client never predicts. A respawn raises health and doesn't flash.

use bevy::prelude::*;

use crate::lockstep::WorldMirror;

/// How long the flash takes to fade, in seconds.
const FADE_SECONDS: f32 = 0.45;
/// The edge color at full strength.
const RED: (f32, f32, f32, f32) = (0.85, 0.04, 0.02, 0.6);

/// The full-screen vignette node. It goes with the world, so a new world
/// starts with nothing seen and no flash.
#[derive(Component, Default)]
pub(crate) struct HurtVignette {
    /// The local avatar and its health on the last frame.
    seen: Option<(u16, u32)>,
    /// 1 when a hit lands, fading to 0.
    strength: f32,
}

/// Whether the local player was hurt since the last frame: the same avatar
/// with less health. A new avatar, or more health (a respawn), isn't.
pub(crate) fn hurt(previous: Option<(u16, u32)>, now: Option<(u16, u32)>) -> bool {
    matches!((previous, now), (Some((was, before)), Some((agent, health))) if was == agent && health < before)
}

pub(crate) fn update(
    mut commands: Commands,
    time: Res<Time>,
    mirror: Res<WorldMirror>,
    mut vignette: Query<(&mut HurtVignette, &mut BackgroundGradient, &mut Visibility)>,
) {
    let Ok((mut state, mut gradient, mut visibility)) = vignette.single_mut() else {
        commands.spawn((
            HurtVignette::default(),
            service::WorldScoped::default(),
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
            edges(0.0),
            GlobalZIndex(60),
            Visibility::Hidden,
        ));
        return;
    };
    let now = mirror.player().and_then(|agent| Some((agent, mirror.current.as_ref()?.agents.get(agent as usize)?.avatar.health)));
    if hurt(state.seen, now) {
        state.strength = 1.0;
    }
    state.seen = now;
    if state.strength <= 0.0 {
        return;
    }
    state.strength = (state.strength - time.delta_secs() / FADE_SECONDS).max(0.0);
    *gradient = edges(state.strength);
    *visibility = if state.strength > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
}

/// Clear in the middle, red toward the edges, at `strength` (0 to 1).
fn edges(strength: f32) -> BackgroundGradient {
    let (r, g, b, a) = RED;
    let red = |alpha: f32| Color::srgba(r, g, b, alpha * strength);
    let stops = vec![ColorStop::new(red(0.0), percent(55)), ColorStop::new(red(a), percent(100))];
    BackgroundGradient::from(RadialGradient::new(UiPosition::CENTER, RadialGradientShape::FarthestCorner, stops))
}

#[cfg(test)]
mod tests {
    use super::hurt;

    #[test]
    fn a_health_drop_flashes() {
        assert!(hurt(Some((2, 6)), Some((2, 5))));
        assert!(hurt(Some((2, 1)), Some((2, 0))), "the killing hit too");
    }

    #[test]
    fn a_respawn_or_steady_health_does_not_flash() {
        assert!(!hurt(Some((2, 0)), Some((2, 6))), "respawning raises health");
        assert!(!hurt(Some((2, 6)), Some((2, 6))));
    }

    #[test]
    fn a_new_or_missing_avatar_does_not_flash() {
        assert!(!hurt(None, Some((2, 3))));
        assert!(!hurt(Some((1, 6)), Some((2, 3))), "another avatar");
        assert!(!hurt(Some((2, 6)), None));
    }
}
