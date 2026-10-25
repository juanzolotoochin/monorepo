//! Controller tuning, in one place. The values were checked against
//! synthetic traces (see `input::tests`); physical feel over USB is
//! unverified and may move them.

/// Movement stick: active past `ENTER`, inactive again under `EXIT`.
pub const MOVE_STICK_ENTER: f32 = 0.30;
pub const MOVE_STICK_EXIT: f32 = 0.20;
/// Aim stick: a little stiffer, so resting a thumb on it does not re-aim.
pub const AIM_STICK_ENTER: f32 = 0.45;
pub const AIM_STICK_EXIT: f32 = 0.30;
/// Triggers: pressed at `PRESS`, released only under `RELEASE`.
pub const TRIGGER_PRESS: f32 = 0.55;
pub const TRIGGER_RELEASE: f32 = 0.35;
/// A held stick changes the movement direction only when it turns further
/// than this (radians, about 3°).
pub const MOVE_DIRECTION_STEP: f32 = 0.052;
/// Menu navigation from a held stick or D-pad: first repeat, then every.
pub const NAV_REPEAT_DELAY: f32 = 0.40;
pub const NAV_REPEAT_INTERVAL: f32 = 0.12;
/// The stick navigates menus past this, on its dominant axis.
pub const NAV_STICK: f32 = 0.6;
/// Mouse movement in one frame that counts as deliberate (pixels).
pub const MOUSE_TAKEOVER_PIXELS: f32 = 8.0;
/// Nearby targets: how far beyond a target's interaction surface (meters).
pub const TARGET_RADIUS: f32 = 3.0;
/// Targets on another floor (meters of height difference) are left out.
pub const TARGET_FLOOR: f32 = 0.75;
/// How often target approaches are re-checked (seconds).
pub const TARGET_RECHECK: f32 = 0.25;
/// A proposed cargo drop sits this far ahead of the avatar (meters).
pub const DROP_AHEAD: f32 = 1.4;
/// Zoom per D-pad press (the wheel moves 0.8 per notch).
pub const ZOOM_STEP: f32 = 2.0;
