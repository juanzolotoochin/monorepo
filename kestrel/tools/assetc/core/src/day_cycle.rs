//! A scene's light through the day and night (`Scene::day_cycle`): four
//! keys, blended by the sim's clock, and the arc the sun (and by night the
//! moon) follows. Renderer-independent, so the blends and the arc are pure
//! functions the app samples every frame.
use serde::{Deserialize, Serialize};
use sim::settlement::Clock;

/// The light at one hour: the key of a [`DayCycleLight`]. Colors are sRGB
/// in 0..1, as a style profile's.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightKey {
    /// The sun's (by night the moon's) color.
    pub sunlight: [f32; 3],
    /// Its illuminance, in lux.
    pub illuminance: f32,
    pub ambient: [f32; 3],
    pub ambient_brightness: f32,
    /// The clear color, which the fog fades to.
    pub sky: [f32; 3],
    pub fog_density: f32,
}

/// A scene's day cycle of light: the key at dawn (the day's first tick),
/// noon (mid-day), dusk (the night's first tick) and midnight (mid-night).
/// The scene's scenario must have a day cycle (`Scenario::day_cycle`),
/// whose clock says the hour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DayCycleLight {
    pub dawn: LightKey,
    pub noon: LightKey,
    pub dusk: LightKey,
    pub midnight: LightKey,
}

/// An hour of the cycle: day or night, and how far through it (0 at dawn
/// or dusk, under 1 at the phase's end).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hour {
    pub night: bool,
    pub fraction: f32,
}

/// The hour at `time` ticks (fractional, so the view can ease between
/// ticks), on the sim's clock: the cycle starts at dawn at tick 0.
pub fn hour(clock: &Clock, time: f64) -> Hour {
    let (day, night) = (clock.day as f64, clock.night as f64);
    let t = time.max(0.0).rem_euclid(day + night);
    if t < day {
        Hour { night: false, fraction: (t / day) as f32 }
    } else {
        Hour { night: true, fraction: (((t - day) / night) as f32).min(1.0) }
    }
}

/// How far into the day noon's light is reached (dawn's fades by then),
/// and from when it turns towards dusk's.
pub const MORNING: f32 = 0.3;
pub const AFTERNOON: f32 = 0.7;
/// How far into the night midnight's light is reached, and from when it
/// turns towards dawn's.
pub const EVENING: f32 = 0.3;
pub const SMALL_HOURS: f32 = 0.75;
/// Over this much of a phase at each end the light dims to nothing, so
/// the sun setting in the west can come back as the moon rising in the
/// east (and the moon as the sun) unseen.
pub const SWAP: f32 = 0.03;
/// The sun's (and moon's) elevation on the horizon and at its height, in
/// degrees.
pub const HORIZON_ELEVATION: f32 = 10.0;
pub const PEAK_ELEVATION: f32 = 58.0;

/// The blended light at an hour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub key: LightKey,
    /// The direction towards the sun (by night the moon), unit length:
    /// [`sky_arc`].
    pub toward: [f32; 3],
}

impl DayCycleLight {
    pub fn validate(&self) -> crate::Result<()> {
        for key in [&self.dawn, &self.noon, &self.dusk, &self.midnight] {
            if key
                .sunlight
                .iter()
                .chain(&key.ambient)
                .chain(&key.sky)
                .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
                || [
                    (key.illuminance, 100000.0),
                    (key.ambient_brightness, 1000.0),
                    (key.fog_density, 0.1),
                ]
                .iter()
                .any(|&(v, max)| !v.is_finite() || !(0.0..=max).contains(&v))
            {
                return Err("invalid day cycle light key".into());
            }
        }
        Ok(())
    }

    /// The keys blended at an hour: each key holds at its own hour and
    /// eases (smoothstep) to the next over the twilight between them:
    /// dawn to noon over the [`MORNING`], noon from the [`AFTERNOON`] to
    /// dusk, dusk to midnight over the [`EVENING`], and midnight from the
    /// [`SMALL_HOURS`] to dawn. The illuminance dims to zero over the
    /// [`SWAP`] at each end of a phase.
    pub fn at(&self, hour: Hour) -> Light {
        let f = hour.fraction.clamp(0.0, 1.0);
        let key = if !hour.night {
            if f < MORNING {
                blend(&self.dawn, &self.noon, smoothstep(f / MORNING))
            } else if f < AFTERNOON {
                self.noon
            } else {
                blend(&self.noon, &self.dusk, smoothstep((f - AFTERNOON) / (1.0 - AFTERNOON)))
            }
        } else if f < EVENING {
            blend(&self.dusk, &self.midnight, smoothstep(f / EVENING))
        } else if f < SMALL_HOURS {
            self.midnight
        } else {
            blend(&self.midnight, &self.dawn, smoothstep((f - SMALL_HOURS) / (1.0 - SMALL_HOURS)))
        };
        let swap = smoothstep(f / SWAP) * smoothstep((1.0 - f) / SWAP);
        Light { key: LightKey { illuminance: key.illuminance * swap, ..key }, toward: sky_arc(f) }
    }
}

/// The direction towards the sun at `fraction` through the day (or the
/// moon through the night), unit length, scene axes (+y up): it rises in
/// the east (+x), stands due south (+z) at its height in the middle of the
/// phase, and sets in the west (-x). Its elevation is
/// [`HORIZON_ELEVATION`] at either end and [`PEAK_ELEVATION`] at the
/// middle, so shadows never grow longer than about 5.7 times a caster's
/// height.
pub fn sky_arc(fraction: f32) -> [f32; 3] {
    let angle = std::f32::consts::PI * fraction.clamp(0.0, 1.0);
    let elevation = (HORIZON_ELEVATION + (PEAK_ELEVATION - HORIZON_ELEVATION) * angle.sin()).to_radians();
    let (up, flat) = elevation.sin_cos();
    [angle.cos() * flat, up, angle.sin() * flat]
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// `a` eased `t` of the way to `b`, every value linearly.
pub fn blend(a: &LightKey, b: &LightKey, t: f32) -> LightKey {
    LightKey {
        sunlight: mix3(a.sunlight, b.sunlight, t),
        illuminance: mix(a.illuminance, b.illuminance, t),
        ambient: mix3(a.ambient, b.ambient, t),
        ambient_brightness: mix(a.ambient_brightness, b.ambient_brightness, t),
        sky: mix3(a.sky, b.sky, t),
        fog_density: mix(a.fog_density, b.fog_density, t),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(v: f32) -> LightKey {
        LightKey {
            sunlight: [v; 3],
            illuminance: v * 1000.0,
            ambient: [v; 3],
            ambient_brightness: v * 100.0,
            sky: [v; 3],
            fog_density: v / 100.0,
        }
    }

    fn cycle() -> DayCycleLight {
        DayCycleLight { dawn: key(0.2), noon: key(1.0), dusk: key(0.4), midnight: key(0.0) }
    }

    fn day(fraction: f32) -> Hour {
        Hour { night: false, fraction }
    }

    fn night(fraction: f32) -> Hour {
        Hour { night: true, fraction }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn each_key_holds_at_its_hour_and_the_light_dims_at_the_swaps() {
        let c = cycle();
        // Dawn and dusk: the key's colors, its light dimmed to nothing.
        for (hour, k) in [(day(0.0), c.dawn), (night(0.0), c.dusk)] {
            let l = c.at(hour).key;
            assert_eq!(l.sunlight, k.sunlight);
            assert_eq!(l.ambient, k.ambient);
            assert_eq!(l.sky, k.sky);
            assert_eq!(l.fog_density, k.fog_density);
            assert_eq!(l.illuminance, 0.0);
        }
        // A day's last moment is dusk's, a night's last dawn's.
        assert!(close(c.at(day(0.9999)).key.ambient[0], c.dusk.ambient[0]));
        assert!(close(c.at(night(0.9999)).key.ambient[0], c.dawn.ambient[0]));
        // Past the swap a key shines at full strength.
        assert!(close(c.at(day(SWAP)).key.illuminance, blend(&c.dawn, &c.noon, smoothstep(SWAP / MORNING)).illuminance));
        // Noon all through the middle of the day, midnight through the night's.
        for f in [MORNING, 0.5, AFTERNOON - 0.001] {
            assert_eq!(c.at(day(f)).key, c.noon);
        }
        for f in [EVENING, 0.5, SMALL_HOURS - 0.001] {
            assert_eq!(c.at(night(f)).key, c.midnight);
        }
    }

    #[test]
    fn between_keys_the_light_eases_from_one_to_the_next() {
        let c = cycle();
        // Halfway through the morning: halfway from dawn to noon.
        let l = c.at(day(MORNING / 2.0)).key;
        assert!(close(l.ambient[0], 0.6) && close(l.illuminance, 600.0) && close(l.fog_density, 0.006));
        // Halfway through the evening: halfway from dusk to midnight.
        assert!(close(c.at(night(EVENING / 2.0)).key.sky[1], 0.2));
        // Halfway from the afternoon to dusk, and the small hours to dawn.
        assert!(close(c.at(day((AFTERNOON + 1.0) / 2.0)).key.sunlight[2], 0.7));
        assert!(close(c.at(night((SMALL_HOURS + 1.0) / 2.0)).key.ambient_brightness, 10.0));
        // Eased: a quarter of the way in is less than a quarter of the change.
        let quarter = c.at(day(MORNING / 4.0)).key.ambient[0];
        assert!(quarter > 0.2 && quarter < 0.4, "{quarter}");
        // Steadily, never past either key.
        let mut last = c.dawn.ambient[0];
        for i in 1..=30 {
            let a = c.at(day(MORNING * i as f32 / 30.0)).key.ambient[0];
            assert!(a >= last && a <= 1.0);
            last = a;
        }
    }

    #[test]
    fn the_sun_rises_east_stands_south_and_sets_west() {
        let elevation = |d: [f32; 3]| d[1].asin().to_degrees();
        let dawn = sky_arc(0.0);
        let noon = sky_arc(0.5);
        let dusk = sky_arc(1.0);
        for d in [dawn, noon, dusk, sky_arc(0.2)] {
            assert!(close(d.iter().map(|v| v * v).sum::<f32>(), 1.0));
        }
        assert!(dawn[0] > 0.98 && dawn[2].abs() < 1e-4 && close(elevation(dawn), HORIZON_ELEVATION));
        assert!(noon[0].abs() < 1e-4 && noon[2] > 0.5 && close(elevation(noon), PEAK_ELEVATION));
        assert!(dusk[0] < -0.98 && dusk[2].abs() < 1e-4 && close(elevation(dusk), HORIZON_ELEVATION));
        // Rising through the morning, east of south; west of it after noon.
        assert!(elevation(sky_arc(0.25)) > HORIZON_ELEVATION && elevation(sky_arc(0.25)) < PEAK_ELEVATION);
        assert!(sky_arc(0.25)[0] > 0.0 && sky_arc(0.75)[0] < 0.0 && sky_arc(0.25)[2] > 0.0);
        // By night the moon takes the same arc, and the blend says where.
        let c = cycle();
        assert_eq!(c.at(night(0.5)).toward, noon);
        assert_eq!(c.at(day(0.0)).toward, dawn);
    }

    #[test]
    fn the_hour_follows_the_sims_clock_between_ticks() {
        let clock = Clock::new(600, 300);
        assert_eq!(hour(&clock, 0.0), day(0.0));
        assert_eq!(hour(&clock, 300.0), day(0.5));
        assert_eq!(hour(&clock, 300.5).fraction, 300.5 / 600.0);
        assert_eq!(hour(&clock, 600.0), night(0.0));
        assert_eq!(hour(&clock, 750.0), night(0.5));
        assert_eq!(hour(&clock, 900.0), day(0.0));
        assert_eq!(hour(&clock, 1650.0), night(0.5));
        // Every tick agrees with the sim's own clock.
        for t in [0, 1, 599, 600, 601, 899, 900, 1234] {
            let sim = clock.at(t);
            let h = hour(&clock, t as f64);
            assert_eq!(h.night, sim.phase == sim::settlement::clock::Phase::Night);
            assert!((h.fraction - sim.fraction.to_f32()).abs() < 1e-3);
        }
    }

    #[test]
    fn keys_out_of_range_fail_validation() {
        let mut c = cycle();
        assert!(c.validate().is_ok());
        c.noon.sky[0] = 1.5;
        assert!(c.validate().is_err());
        c = cycle();
        c.midnight.illuminance = -1.0;
        assert!(c.validate().is_err());
        c = cycle();
        c.dusk.fog_density = f32::NAN;
        assert!(c.validate().is_err());
    }
}
