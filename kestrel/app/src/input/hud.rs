//! What the controller shows: the claim prompt, the selected target (its
//! outline, name, verb or why it is unavailable), the last refusal, the
//! drop preview, and the utility panel or item chooser. In a town, the
//! interact target's ring, a blueprint ghost's rings, each site's plot, a
//! possessed ballista's range and reticle, and the edges of burning ground.

use bevy::prelude::*;

use super::gameplay::Dispatcher;
use super::{GamepadSnapshots, InputService, Panel};

#[derive(Component)]
pub(super) struct ControllerText;

#[allow(clippy::too_many_arguments)]
pub(super) fn show(
    mut commands: Commands,
    input: Res<InputService>,
    snapshots: Res<GamepadSnapshots>,
    dispatcher: Option<Res<Dispatcher>>,
    session: Option<Res<crate::lockstep::SessionService>>,
    time: Res<Time>,
    mut text: Query<&mut Text, With<ControllerText>>,
    mut gizmos: Gizmos,
) {
    let Ok(mut text) = text.single_mut() else {
        commands.spawn((
            ControllerText,
            Text::new(""),
            TextFont { font_size: FontSize::Px(15.0), ..default() },
            TextColor(Color::srgb(0.85, 0.95, 0.93)),
            TextLayout::justify(Justify::Center),
            Node { position_type: PositionType::Absolute, bottom: px(18), left: percent(25), width: percent(50), ..default() },
            GlobalZIndex(70),
        ));
        return;
    };
    let mut lines = Vec::new();
    let connected = snapshots.0.iter().any(|s| s.connected);
    if input.seat().is_none() && connected {
        lines.push("Controller connected: press Menu/Start to use it".to_string());
    }
    if let Some(panel) = input.panel() {
        let (title, options, focus): (String, Vec<String>, usize) = match panel {
            Panel::Utility { items, focus } => ("UTILITY".into(), items.iter().map(|u| u.label(input.recording()).to_string()).collect(), *focus),
            Panel::Chooser { title, options, focus } => (title.clone(), options.clone(), *focus),
            // The ring itself is drawn around the sprite (`settlement`).
            Panel::Radial { title, options, focus } => (title.clone(), vec![options[*focus].clone()], usize::MAX),
        };
        let radial = matches!(panel, Panel::Radial { .. });
        lines.push(title);
        for (i, option) in options.iter().enumerate() {
            lines.push(format!("{} {option}", if i == focus { ">" } else { " " }));
        }
        lines.push(if radial { "Stick or WASD to pick, A or E confirm, B or Esc back" } else { "A confirm / B back" }.into());
    } else if let (Some(dispatcher), true) = (dispatcher.as_ref(), input.seat().is_some()) {
        if let Some(target) = dispatcher.targets.selected() {
            lines.push(match &target.verb {
                Ok(verb) => format!("{}  -  A: {verb}", target.name),
                Err(reason) => format!("{}  -  {reason}", target.name),
            });
            let color = if target.verb.is_ok() { Color::srgba(0.35, 1.0, 0.85, 0.9) } else { Color::srgba(1.0, 0.55, 0.3, 0.9) };
            gizmos.circle(
                Isometry3d::new(target.position + Vec3::Y * 0.05, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
                target.surface + 0.25,
                color,
            );
        }
        if dispatcher.targets.candidates.len() > 1 {
            lines.push(format!("D-pad left/right: {} nearby", dispatcher.targets.candidates.len()));
        }
        if let Some(at) = dispatcher.drop_preview {
            gizmos.circle(Isometry3d::new(at + Vec3::Y * 0.05, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), 0.45, Color::srgba(1.0, 0.85, 0.3, 0.8));
            lines.push("B: drop cargo here".into());
        }
        if !dispatcher.notice.is_empty() {
            lines.push(dispatcher.notice.clone());
        }
    }
    // The town's target, for every device: a ring on the ground, and a
    // fainter one pulsing out from it (its prompt is a caption,
    // `settlement`).
    if let Some(h) = dispatcher.as_ref().and_then(|d| d.town.highlight.as_ref()) {
        let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        let color = Color::srgba(0.35, 1.0, 0.85, 0.9);
        gizmos.circle(Isometry3d::new(h.position + Vec3::Y * 0.05, flat), h.radius, color);
        let pulse = (time.elapsed_secs() * 1.6).fract();
        gizmos.circle(Isometry3d::new(h.position + Vec3::Y * 0.05, flat), h.radius + 0.25 * pulse, color.with_alpha(0.6 * (1.0 - pulse)));
    }
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    // A possessed ballista's range on the ground, and its reticle: a ring
    // and a cross where the next bolt goes (`town::aim_reticle`).
    if let Some(r) = dispatcher.as_ref().and_then(|d| d.town.reticle.as_ref()) {
        let color = Color::srgba(1.0, 0.82, 0.35, 0.95);
        gizmos.circle(Isometry3d::new(r.center + Vec3::Y * 0.05, flat), r.range, color.with_alpha(0.35));
        let at = r.point + Vec3::Y * 0.07;
        // The line of fire, faint, then the reticle itself.
        let from = r.center + Vec3::Y * 0.07;
        if (at - from).length() > 0.8 {
            gizmos.line(from + (at - from).normalize() * 0.8, at, color.with_alpha(0.3));
        }
        gizmos.circle(Isometry3d::new(at, flat), 0.55, color);
        gizmos.circle(Isometry3d::new(at, flat), 0.2, color.with_alpha(0.8));
        for d in [Vec3::X, Vec3::Z] {
            gizmos.line(at - d * 0.8, at - d * 0.3, color);
            gizmos.line(at + d * 0.3, at + d * 0.8, color);
        }
    }
    // A blueprint's ghost: its footprint as rings filling in toward the
    // center, and its output pile and slot spot, green while it may go down
    // and red while it may not (its skin and reason are `settlement`'s).
    if let Some(g) = dispatcher.as_ref().and_then(|d| d.town.ghost.as_ref()) {
        let color = if g.verdict.is_ok() { Color::srgba(0.35, 1.0, 0.55, 0.95) } else { Color::srgba(1.0, 0.3, 0.25, 0.95) };
        for (i, share) in [1.0, 0.75, 0.5, 0.25].into_iter().enumerate() {
            gizmos.circle(Isometry3d::new(g.center + Vec3::Y * 0.06, flat), g.radius * share, color.with_alpha(0.95 - 0.2 * i as f32));
        }
        gizmos.circle(Isometry3d::new(g.output + Vec3::Y * 0.06, flat), 0.25, color);
        gizmos.circle(Isometry3d::new(g.slot + Vec3::Y * 0.06, flat), 0.3, color.with_alpha(0.6));
    }
    // Each site's plot, staked out on the ground until it is built.
    if let Some(s) = session.as_ref().and_then(|s| s.settlement().map(|settlement| (s, settlement))) {
        let (service, settlement) = s;
        for (i, b) in settlement.buildings.iter().enumerate().filter(|(_, b)| !b.built()) {
            let center = service.to_view(b.position).with_y(0.06);
            let radius = settlement.building_kind(i as u16).radius.to_f32();
            gizmos.circle(Isometry3d::new(center, flat), radius, Color::srgba(0.95, 0.78, 0.40, 0.85));
            gizmos.circle(Isometry3d::new(center, flat), radius * 0.92, Color::srgba(0.95, 0.78, 0.40, 0.4));
        }
        // Burning ground (B4): the fire's edge, flickering, and a fainter
        // ring inside it; its flames are glows (`settlement`).
        let pulse = (time.elapsed_secs() * 9.0).sin() * 0.5 + 0.5;
        for fire in &settlement.fires {
            let center = service.to_view(fire.position).with_y(0.06);
            let radius = fire.radius.to_f32();
            gizmos.circle(Isometry3d::new(center, flat), radius * (0.97 + 0.03 * pulse), Color::srgba(1.0, 0.42, 0.08, 0.75 + 0.2 * pulse));
            gizmos.circle(Isometry3d::new(center, flat), radius * 0.6, Color::srgba(1.0, 0.65, 0.2, 0.35 + 0.2 * (1.0 - pulse)));
        }
    }
    let shown = lines.join("\n");
    if text.0 != shown {
        text.0 = shown;
    }
}
