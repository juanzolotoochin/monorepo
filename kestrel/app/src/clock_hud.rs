//! The time on the HUD (`SACRED_TREE_DAYNIGHT.md`, N8, N10): in a scene
//! with days and nights (`Settlement::clock`), a small dial in the top
//! right corner. A sun crosses its dome from east to west by day and a moon
//! by night; under it the day or night it is ("DAY 1", "NIGHT 2"), a
//! "NIGHT FALLS IN 0:20" warning in the last `NIGHT_WARNING_SECONDS` of a
//! day, and from the first dawn the nights survived. View only: it reads
//! the mirror's clock and tick, and keeps nothing of its own. Once the tree
//! has fallen the dial goes, and the fallen overlay speaks.
use bevy::prelude::*;
use sim::settlement::clock::{Clock, Phase};
use voxel_assets::day_cycle::Hour;

/// From how long before dusk the dial warns that night is falling.
pub const NIGHT_WARNING_SECONDS: u64 = 30;

const TICKS: u64 = content::TICKS_PER_SECOND as u64;

/// The dome's size and the arc its sun and moon ride, in pixels.
const DOME_WIDTH: f32 = 64.0;
const DOME_HEIGHT: f32 = 34.0;
const DOME_BORDER: f32 = 2.0;
const ARC_RADIUS: f32 = 22.0;
const MARKER: f32 = 12.0;

const TEXT: Color = Color::srgb(0.9, 0.83, 0.63);
const WARNING: Color = Color::srgb(1.0, 0.62, 0.3);
const MUTED: Color = Color::srgb(0.62, 0.6, 0.52);
const SUN: Color = Color::srgb(1.0, 0.82, 0.36);
const MOON: Color = Color::srgb(0.85, 0.9, 1.0);
const DAY_SKY: Color = Color::srgba(0.36, 0.52, 0.66, 0.55);
const NIGHT_SKY: Color = Color::srgba(0.05, 0.07, 0.17, 0.75);
const RIM: Color = Color::srgb(0.9, 0.83, 0.63);

/// What the dial says at one tick.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClockText {
    /// "DAY 1" before the first dusk, "NIGHT 1" through it, "DAY 2" after.
    pub label: String,
    /// "NIGHT FALLS IN 0:20" near dusk, else empty.
    pub warning: String,
    /// "NIGHTS SURVIVED 1" from the first dawn, else empty.
    pub survived: String,
}

/// `m:ss`, rounding a part second up (as the wave countdown does).
pub fn countdown(ticks: u64) -> String {
    let seconds = ticks.div_ceil(TICKS);
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// The dial's text at `tick`, with the nights the tree has seen through
/// (`Settlement::nights_survived`).
pub fn clock_text(clock: &Clock, tick: u64, survived: u64) -> ClockText {
    let time = clock.at(tick);
    let (label, warning) = match time.phase {
        Phase::Day => {
            let warning = if time.to_dusk <= NIGHT_WARNING_SECONDS * TICKS { format!("NIGHT FALLS IN {}", countdown(time.to_dusk)) } else { String::new() };
            (format!("DAY {}", time.night + 1), warning)
        }
        Phase::Night => (format!("NIGHT {}", time.night), String::new()),
    };
    let survived = if survived > 0 { format!("NIGHTS SURVIVED {survived}") } else { String::new() };
    ClockText { label, warning, survived }
}

/// The status line's wave part (M5.6): how many ghouls are out, or with
/// none out, how long until the next comes; nothing once the last came.
/// With days and nights, a wave due at the next dusk isn't counted down
/// here: the dial says when night falls (`to_dusk`, ticks).
pub fn waves_text(threats: usize, next: Option<u64>, tick: u64, to_dusk: Option<u64>) -> String {
    match (threats, next) {
        (0, Some(next)) if to_dusk.is_some_and(|to_dusk| next >= tick + to_dusk) => String::new(),
        (0, Some(next)) => format!("  /  NEXT WAVE {}", countdown(next.saturating_sub(tick))),
        (0, None) => String::new(),
        (n, _) => format!("  /  GHOULS {n}"),
    }
}

/// Where the sun (by day) or the moon (by night) stands on the dome, its
/// marker's top-left corner inside the dome's border: half risen in the
/// east (left) at the phase's start, at the top halfway, setting in the
/// west at its end.
pub fn marker_at(hour: Hour) -> Vec2 {
    let angle = std::f32::consts::PI * (1.0 - hour.fraction.clamp(0.0, 1.0));
    let inner = DOME_WIDTH - 2.0 * DOME_BORDER;
    let center = Vec2::new(inner / 2.0, DOME_HEIGHT - 2.0 * DOME_BORDER);
    let at = center + Vec2::new(angle.cos(), -angle.sin()) * ARC_RADIUS;
    at - Vec2::splat(MARKER / 2.0)
}

/// The dial's entities.
pub struct ClockHud {
    root: Entity,
    dome: Entity,
    marker: Entity,
    label: Entity,
    warning: Entity,
    survived: Entity,
}

fn line(size: f32, color: Color) -> impl Bundle {
    (Text::new(""), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color), TextLayout::justify(Justify::Right))
}

impl ClockHud {
    /// Spawns the dial, hidden until a frame with a clock shows it.
    pub fn spawn(world: &mut World) -> ClockHud {
        let marker = world
            .spawn((
                Node { position_type: PositionType::Absolute, width: px(MARKER), height: px(MARKER), border_radius: BorderRadius::MAX, ..default() },
                BackgroundColor(SUN),
            ))
            .id();
        let dome = world
            .spawn((
                Node {
                    width: px(DOME_WIDTH),
                    height: px(DOME_HEIGHT),
                    border: UiRect::all(px(DOME_BORDER)),
                    border_radius: BorderRadius::top(px(DOME_WIDTH / 2.0)),
                    margin: UiRect::bottom(px(4)),
                    ..default()
                },
                BorderColor::all(RIM),
                BackgroundColor(DAY_SKY),
            ))
            .add_child(marker)
            .id();
        let label = world.spawn(line(14.0, TEXT)).id();
        let warning = world.spawn(line(16.0, WARNING)).id();
        let survived = world.spawn(line(12.0, MUTED)).id();
        let root = world
            .spawn(Node {
                position_type: PositionType::Absolute,
                right: px(36),
                top: px(32),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::End,
                row_gap: px(4),
                display: Display::None,
                ..default()
            })
            .add_children(&[dome, label, warning, survived])
            .id();
        ClockHud { root, dome, marker, label, warning, survived }
    }

    /// This frame's dial: `None` hides it (no clock, or the tree has
    /// fallen); otherwise the text and the hour the marker shows (eased
    /// between ticks).
    pub fn update(&self, world: &mut World, shown: Option<(ClockText, Hour)>) {
        set_display(world, self.root, shown.is_some());
        let Some((text, hour)) = shown else { return };
        for (e, s) in [(self.label, &text.label), (self.warning, &text.warning), (self.survived, &text.survived)] {
            set_display(world, e, !s.is_empty());
            if world.get::<Text>(e).is_some_and(|t| &t.0 != s) {
                world.get_mut::<Text>(e).unwrap().0 = s.clone();
            }
        }
        let at = marker_at(hour);
        if let Some(mut node) = world.get_mut::<Node>(self.marker) {
            if node.left != px(at.x) || node.top != px(at.y) {
                node.left = px(at.x);
                node.top = px(at.y);
            }
        }
        let (body, sky) = if hour.night { (MOON, NIGHT_SKY) } else { (SUN, DAY_SKY) };
        if let Some(mut color) = world.get_mut::<BackgroundColor>(self.marker) {
            color.set_if_neq(BackgroundColor(body));
        }
        if let Some(mut color) = world.get_mut::<BackgroundColor>(self.dome) {
            color.set_if_neq(BackgroundColor(sky));
        }
    }

    /// Despawns the dial and its parts.
    pub fn despawn(self, world: &mut World) {
        world.despawn(self.root);
    }

    /// What the dial shows now, for the control snapshot: null while hidden.
    pub fn snapshot(&self, world: &World) -> serde_json::Value {
        let shown = world.get::<Node>(self.root).is_some_and(|n| n.display != Display::None);
        if !shown {
            return serde_json::Value::Null;
        }
        let text = |e: Entity| world.get::<Text>(e).map_or_else(String::new, |t| t.0.clone());
        let marker = world.get::<Node>(self.marker).map(|n| [n.left, n.top].map(|v| if let Val::Px(p) = v { p } else { 0.0 }));
        serde_json::json!({"label": text(self.label), "warning": text(self.warning), "survived": text(self.survived), "marker": marker})
    }
}

fn set_display(world: &mut World, e: Entity, shown: bool) {
    let display = if shown { Display::Flex } else { Display::None };
    if let Some(mut node) = world.get_mut::<Node>(e) {
        if node.display != display {
            node.display = display;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Sacred Tree's cycle: 150 s days, 75 s nights.
    fn clock() -> Clock {
        Clock::new(150 * TICKS, 75 * TICKS)
    }

    #[test]
    fn by_day_the_dial_names_the_day_and_warns_only_near_dusk() {
        let c = clock();
        assert_eq!(clock_text(&c, 0, 0), ClockText { label: "DAY 1".into(), ..default() });
        assert_eq!(clock_text(&c, 119 * TICKS, 0).warning, "", "31 s before dusk");
        assert_eq!(clock_text(&c, 120 * TICKS, 0).warning, "NIGHT FALLS IN 0:30");
        assert_eq!(clock_text(&c, 130 * TICKS, 0), ClockText { label: "DAY 1".into(), warning: "NIGHT FALLS IN 0:20".into(), survived: String::new() });
        // A part second rounds up, down to the day's last tick.
        assert_eq!(clock_text(&c, 130 * TICKS + 1, 0).warning, "NIGHT FALLS IN 0:20");
        assert_eq!(clock_text(&c, 150 * TICKS - 1, 0).warning, "NIGHT FALLS IN 0:01");
        // The second day, after the first night.
        let day2 = c.cycle() + 10 * TICKS;
        assert_eq!(clock_text(&c, day2, 1), ClockText { label: "DAY 2".into(), warning: String::new(), survived: "NIGHTS SURVIVED 1".into() });
    }

    #[test]
    fn by_night_the_dial_names_the_night_and_the_nights_survived() {
        let c = clock();
        assert_eq!(clock_text(&c, 150 * TICKS, 0), ClockText { label: "NIGHT 1".into(), ..default() });
        assert_eq!(clock_text(&c, c.cycle() - 1, 0).label, "NIGHT 1");
        // The fourth night, past the Sacred Tree's three listed ones: the
        // count goes on.
        let night4 = 3 * c.cycle() + 150 * TICKS + 5;
        assert_eq!(clock_text(&c, night4, 3), ClockText { label: "NIGHT 4".into(), warning: String::new(), survived: "NIGHTS SURVIVED 3".into() });
    }

    #[test]
    fn with_days_and_nights_the_status_line_leaves_dusk_to_the_dial() {
        let tick = 100 * TICKS;
        let dusk = 50 * TICKS;
        // The next wave comes at dusk: the dial says when.
        assert_eq!(waves_text(0, Some(tick + dusk), tick, Some(dusk)), "");
        // One due before dusk (a timed wave) is still counted down.
        assert_eq!(waves_text(0, Some(tick + 8 * TICKS), tick, Some(dusk)), "  /  NEXT WAVE 0:08");
        assert_eq!(waves_text(3, Some(tick + dusk), tick, Some(dusk)), "  /  GHOULS 3");
        // Without days and nights, as before.
        assert_eq!(waves_text(0, Some(tick + 75 * TICKS), tick, None), "  /  NEXT WAVE 1:15");
        assert_eq!(waves_text(0, None, tick, None), "");
    }

    #[test]
    fn the_sun_and_moon_rise_in_the_east_and_set_in_the_west() {
        let rise = marker_at(Hour { night: false, fraction: 0.0 });
        let noon = marker_at(Hour { night: false, fraction: 0.5 });
        let set = marker_at(Hour { night: true, fraction: 0.999 });
        assert!(rise.x < noon.x && noon.x < set.x, "{rise} {noon} {set}");
        assert!(noon.y < rise.y && noon.y < set.y, "noon is highest: {rise} {noon} {set}");
        assert!((rise.y - set.y).abs() < 0.1);
        // It stays inside the dome.
        let inner = Vec2::new(DOME_WIDTH, DOME_HEIGHT) - 2.0 * DOME_BORDER;
        for p in [rise, noon, set] {
            assert!(p.x >= 0.0 && p.y >= 0.0 && p.x + MARKER <= inner.x && p.y <= inner.y, "{p}");
        }
    }
}
