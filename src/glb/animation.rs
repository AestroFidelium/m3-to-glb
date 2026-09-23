//! Converted animation clips → glTF samplers and channels.

use super::buffers::Buffers;
use super::json_builder::{Accessor, FLOAT, GltfAnimChannel, GltfAnimSampler, GltfAnimation};
use crate::processor::anim::{Animation, Path, SamplerData};

/// Write every clip's keyframes into the buffer and describe them.
pub(super) fn animations(bufs: &mut Buffers, clips: &[Animation]) -> Vec<GltfAnimation> {
    clips
        .iter()
        .map(|clip| GltfAnimation {
            name:     clip.name.clone(),
            samplers: clip.samplers.iter().map(|s| sampler(bufs, s)).collect(),
            channels: clip.channels.iter().map(|c| GltfAnimChannel {
                sampler:     c.sampler,
                target_node: c.target_node,
                path:        match c.path {
                    Path::Translation => "translation",
                    Path::Rotation => "rotation",
                    Path::Scale => "scale",
                },
            }).collect(),
        })
        .collect()
}

fn sampler(bufs: &mut Buffers, s: &crate::processor::anim::Sampler) -> GltfAnimSampler {
    // Input: time in seconds. The spec requires min/max; a single key has
    // min == max, which is valid.
    let (lo, hi) = s.times_sec.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &t| (lo.min(t), hi.max(t)));
    let buffer_view = bufs.view(bytemuck::cast_slice(&s.times_sec), None);
    let input = bufs.accessor(Accessor {
        buffer_view,
        byte_offset: 0,
        component_type: FLOAT,
        count: s.times_sec.len(),
        element_type: "SCALAR",
        normalized: false,
        min: Some(vec![f64::from(lo)]),
        max: Some(vec![f64::from(hi)]),
    });
    let output = match &s.data {
        SamplerData::Vec3(v) => bufs.attribute(bytemuck::cast_slice(v), None, FLOAT, v.len(), "VEC3"),
        SamplerData::Quat(v) => bufs.attribute(bytemuck::cast_slice(v), None, FLOAT, v.len(), "VEC4"),
    };
    GltfAnimSampler { input, output, interpolation: if s.linear { "LINEAR" } else { "STEP" } }
}
