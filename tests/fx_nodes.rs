//! Effect nodes as the whole pipeline writes them: the spawn window recovered
//! from an animated rate, and the node name built from the bone's.

mod common;

use bolero::{TypeGenerator, check};
use bytemuck::Zeroable;
use common::*;
use m3_to_glb::Converter;
use m3_to_glb::m3::structures::Par;

const RATE_ID: u32 = 77;

fn convert(spec: &ModelSpec) -> GlbSummary {
    let glb = Converter::new()
        .convert(&spec.build())
        .expect("conversion failed");
    check_glb(&glb.bytes).unwrap_or_else(|e| panic!("invalid GLB: {e}"))
}

/// One particle system on one bone.
fn emitter(bone: &str, par: &Par) -> ModelSpec {
    ModelSpec {
        bones: vec![BoneSpec::named(bone, -1)],
        particles: vec![*par],
        ..ModelSpec::default()
    }
}

// ─── Spawn window ────────────────────────────────────────────────────────────

/// When the emitter switches on, from the sequence start.
#[derive(Debug, Clone, Copy, TypeGenerator)]
enum Delay {
    AtStart,
    After(#[generator(1u16..=2000)] u16),
}

impl Delay {
    fn ms(self) -> i32 {
        match self {
            Self::AtStart => 0,
            Self::After(ms) => i32::from(ms),
        }
    }
}

/// How the rate curve switches the emitter on at its [`Delay`].
#[derive(Debug, Clone, Copy, TypeGenerator)]
enum Rise {
    /// The first key is already at the peak.
    Step,
    /// An explicit off key, then the peak `ramp_ms` later.
    FromOff {
        #[generator(1u16..=500)]
        ramp_ms: u16,
    },
}

/// Whether the curve switches the emitter off again.
#[derive(Debug, Clone, Copy, TypeGenerator)]
enum Fall {
    /// The peak is the last key.
    Never,
    /// An off key `after_ms` past the peak.
    After {
        #[generator(1u16..=2000)]
        after_ms: u16,
    },
}

#[derive(Debug, TypeGenerator)]
struct RateBurst {
    delay: Delay,
    rise: Rise,
    fall: Fall,
    #[generator(1u8..=100)]
    peak: u8,
}

impl RateBurst {
    /// The `(ms, rate)` keys of the curve.
    fn keys(&self) -> Vec<(i32, f32)> {
        let delay = self.delay.ms();
        let peak = f32::from(self.peak);
        let (mut keys, peak_at) = match self.rise {
            Rise::Step => (vec![(delay, peak)], delay),
            Rise::FromOff { ramp_ms } => {
                let at = delay + i32::from(ramp_ms);
                (vec![(delay, 0.0), (at, peak)], at)
            }
        };
        if let Fall::After { after_ms } = self.fall {
            keys.push((peak_at + i32::from(after_ms), 0.0));
        }
        keys
    }

    /// Seconds from the sequence start to the emitter switching on, and how
    /// long it stays on: from the key that starts the rise to the key that ends
    /// the burst (the last one).
    fn window(&self) -> (f32, f32) {
        let keys = self.keys();
        let secs = |ms: i32| ms as f32 / 1000.0;
        let start = secs(keys.first().expect("a key").0);
        let end = secs(keys.last().expect("a key").0);
        (start, end - start)
    }

    fn spec(&self) -> ModelSpec {
        let mut par = Par::zeroed();
        par.emit_rate.header.id = RATE_ID;
        let mut spec = emitter("Bone_FX", &par);
        spec.stcs = vec![StcSpec {
            name: "Birth".into(),
            tracks: vec![(RATE_ID, Track::Real(self.keys()))],
            ..StcSpec::default()
        }];
        spec
    }
}

/// A number the writer must leave out when it is zero.
fn optional(spawn: &serde_json::Value, key: &str) -> Option<f32> {
    #[expect(clippy::cast_possible_truncation, reason = "comparing at f32")]
    spawn.get(key).map(|v| v.as_f64().expect("a number") as f32)
}

#[test]
fn an_animated_rate_carries_its_spawn_window() {
    check!().with_type::<RateBurst>().for_each(|burst| {
        let g = convert(&burst.spec());
        let fx = g.extras("m3fx");
        let spawn = &fx.first().expect("one effect")["spawn"];
        let (delay, duration) = burst.window();
        assert_eq!(spawn["driven"], true, "{spawn}");
        assert_eq!(spawn["rate"], f64::from(burst.peak), "{spawn}");
        assert_eq!(
            optional(spawn, "delay"),
            (delay > 0.0).then_some(delay),
            "{spawn}"
        );
        assert_eq!(
            optional(spawn, "duration"),
            (duration > 0.0).then_some(duration),
            "{spawn}"
        );
    });
}

// ─── Node names ──────────────────────────────────────────────────────────────

/// A character of a bone name, from the classes the node name treats apart.
#[derive(Debug, Clone, Copy, TypeGenerator)]
enum NameChar {
    Lower,
    Upper,
    Digit,
    Underscore,
    Dash,
    Space,
    Dot,
    Quote,
    Cyrillic,
}

impl NameChar {
    fn raw(self) -> char {
        match self {
            Self::Lower => 'a',
            Self::Upper => 'Z',
            Self::Digit => '7',
            Self::Underscore => '_',
            Self::Dash => '-',
            Self::Space => ' ',
            Self::Dot => '.',
            Self::Quote => '"',
            Self::Cyrillic => 'ж',
        }
    }

    /// What survives into a node name: ASCII letters, digits, `_` and `-`.
    fn kept(self) -> char {
        match self {
            Self::Lower | Self::Upper | Self::Digit | Self::Underscore | Self::Dash => self.raw(),
            Self::Space | Self::Dot | Self::Quote | Self::Cyrillic => '_',
        }
    }
}

#[derive(Debug, TypeGenerator)]
struct BoneName {
    #[generator(bolero::generator::produce::<Vec<NameChar>>().with().len(1usize..=8))]
    chars: Vec<NameChar>,
}

#[test]
fn effect_nodes_are_named_after_their_bone() {
    check!().with_type::<BoneName>().for_each(|name| {
        let bone: String = name.chars.iter().map(|c| c.raw()).collect();
        let kept: String = name.chars.iter().map(|c| c.kept()).collect();
        let g = convert(&emitter(&bone, &Par::zeroed()));
        let expected = format!("PAR0_{kept}");
        assert!(
            g.node_named(&expected)
                .is_some_and(|n| n.get("extras").is_some()),
            "no effect node {expected:?} for bone {bone:?}: {}",
            g.json["nodes"]
        );
    });
}
