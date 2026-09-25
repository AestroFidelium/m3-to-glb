//! `M3File` accessors on hand-built files, including the malformed shapes each
//! one has to reject without panicking.

mod common;

use bytemuck::Zeroable;
use common::*;
use m3_to_glb::m3::structures::{Div, Reference};
use m3_to_glb::m3::{M3Version, parse};

#[test]
fn header_variant_is_reported() {
    for (magic, v) in [(*b"43DM", M3Version::Md34), (*b"33DM", M3Version::Md33), (*b"MD32", M3Version::Md32)] {
        let bytes = ModelSpec { magic, ..ModelSpec::default() }.build();
        assert_eq!(parse(&bytes).unwrap().version(), v);
    }
    let mut bytes = ModelSpec::default().build();
    bytes[..4].copy_from_slice(b"\xff\xfe\x00\x01");
    assert!(parse(&bytes).unwrap_err().to_string().contains("unknown magic"));
    assert!(parse(b"43DM").is_err(), "a bare magic has no header");
}

#[test]
fn counts_and_optional_tables() {
    let spec = skinned_quad();
    let bytes = spec.build();
    let m3 = parse(&bytes).unwrap();
    assert_eq!((m3.mesh_count(), m3.material_count(), m3.madd_count(), m3.bone_count()), (1, 1, 0, 2));
    assert_eq!(m3.bone_rests().unwrap().len(), 2);
    assert!(!m3.has_attachment_points());
    assert!(m3.madd_texture_paths(0).unwrap().is_empty());

    let empty = ModelSpec::default().build();
    let m3 = parse(&empty).unwrap();
    assert!(m3.bone_rests().unwrap().is_empty());
    assert!(m3.material_references().unwrap().is_empty());
    assert!(m3.divisions().is_err(), "no DIV_ tag");
    assert!(m3.vertex_data().is_err(), "no U8__ tag");
    assert_eq!(m3.vertex_flags(), 0);
    // No MODL at all: the historical default layout.
    let bare = M3Writer::new(*b"43DM").finish();
    let m3 = parse(&bare).unwrap();
    assert_eq!(m3.vertex_flags(), 0x0180_007d);
    assert!(m3.bones().unwrap().is_empty() && m3.sequences().unwrap().is_empty());
    assert!(format!("{m3:?}").contains("Md34"));
}

#[test]
fn layer_tiling_and_colour() {
    let mut spec = ModelSpec::quad();
    for v in [22, 24] {
        spec.layr_version = v;
        spec.materials[0].layers = vec![
            ("diff", LayerSpec { texture: "t.dds".into(), uv_tiling: [2.0, 0.0], ..LayerSpec::default() }),
            ("emis1", LayerSpec { color: Some([1, 2, 3, 4]), ..LayerSpec::default() }),
        ];
        let bytes = spec.build();
        let m3 = parse(&bytes).unwrap();
        let diff = m3.mat_layer_ref(0, "diff").unwrap();
        // A zero tiling is treated as 1 — it would otherwise collapse the UVs.
        assert_eq!(m3.read_layer_uv_tiling(&diff), (2.0, 1.0), "LAYR v{v}");
        assert!(m3.layer_color(0, "diff").is_none(), "a bitmap layer has no flat colour");
        let c = m3.layer_color(0, "emis1").unwrap();
        assert!((c[0] - 3.0 / 255.0).abs() < 1e-6 && (c[3] - 4.0 / 255.0).abs() < 1e-6);
        assert_eq!(m3.texture_path_for_layer(0, "diff"), "t.dds");
        assert_eq!(m3.texture_path_for_layer(0, "emis1"), "");
        assert_eq!(m3.texture_path_for_layer(0, "no-such-layer"), "");
        assert_eq!(m3.texture_path_for_layer(0, "norm"), "", "absent layer");
        assert_eq!(m3.texture_path_for_layer(9, "diff"), "", "past the table");
        assert!(m3.layer_color(9, "diff").is_none());
    }
    let bytes = spec.build();
    let m3 = parse(&bytes).unwrap();
    let null = Reference::zeroed();
    assert_eq!(m3.read_layer_uv_tiling(&null), (1.0, 1.0));
    let dangling = Reference { entries: 1, index: 9999, flags: 0 };
    assert_eq!(m3.read_layer_uv_tiling(&dangling), (1.0, 1.0));
    assert!(m3.read_layer_bitmap_ref(&dangling).is_none());
    assert!(m3.read_layer_bitmap_ref(&null).is_none());
    assert!(m3.read_char(&dangling).is_err());
    assert!(m3.read_ref_f32(&dangling).is_err());
    assert!(m3.read_ref_i16(&null).unwrap().is_empty());
    assert!(m3.read_ref_u16(&null).unwrap().is_empty());
    assert!(m3.read_ref_u32(&null).unwrap().is_empty());
    {
        let r = &null;
        assert!(m3.read_ref_i32(r).unwrap().is_empty());
        assert!(m3.read_ref_vec3(r).unwrap().is_empty());
        assert!(m3.read_ref_quat(r).unwrap().is_empty());
        assert!(m3.read_sd3v(r).unwrap().is_empty());
        assert!(m3.read_sd4q(r).unwrap().is_empty());
        assert!(m3.read_sdr3(r).unwrap().is_empty());
        assert!(m3.read_sds6(r).unwrap().is_empty());
        assert!(m3.read_sdu6(r).unwrap().is_empty());
    }
}

#[test]
fn material_accessors_tolerate_a_short_table() {
    let mut w = M3Writer::new(*b"43DM");
    // One MAT_ v19 record declared, far fewer bytes present.
    w.raw("MAT_", 19, vec![0; 30], 1);
    let bytes = w.finish();
    let m3 = parse(&bytes).unwrap();
    // The first layer reference ends inside the 30 bytes present; later ones do not.
    assert!(m3.mat_layer_ref(0, "norm").is_none());
    assert_eq!(m3.mat_blend_mode(3), 0);
    assert_eq!(m3.mat_flags(3), 0);
    assert_eq!(m3.mat_alpha_threshold(3), 0);
    assert!(m3.mat_layer_ref(3, "diff").is_none());
}

#[test]
fn madd_paths_out_of_range_or_truncated() {
    let spec = ModelSpec { madds: vec![vec!["a_diff.dds".into()]], ..ModelSpec::default() };
    let bytes = spec.build();
    let m3 = parse(&bytes).unwrap();
    assert_eq!(m3.madd_texture_paths(0).unwrap(), ["a_diff.dds"]);
    assert!(m3.madd_texture_paths(5).unwrap().is_empty());
    assert_eq!(m3.madd_blend_mode(5), 0);

    let mut w = M3Writer::new(*b"43DM");
    w.raw("MADD", 3, vec![0; 8], 1); // record cut short
    let bytes = w.finish();
    assert!(parse(&bytes).unwrap().madd_texture_paths(0).unwrap().is_empty());
    assert_eq!(parse(&bytes).unwrap().madd_blend_mode(0), 0);
}

#[test]
fn tag_tables_that_overrun_the_file_are_errors() {
    let mut w = M3Writer::new(*b"43DM");
    w.raw("DIV_", 2, vec![0; 16], 50); // 50 records, 16 bytes
    w.raw("BONE", 1, vec![0; 16], 50);
    w.raw("PAR_", 24, vec![0; 16], 3);
    let bytes = w.finish();
    let m3 = parse(&bytes).unwrap();
    assert!(m3.divisions().unwrap_err().to_string().contains("out of bounds"));
    assert!(m3.particle_systems().unwrap_err().to_string().contains("out of bounds"));
}

#[test]
fn division_references_are_checked() {
    let m3_bytes = ModelSpec::quad().build();
    let m3 = parse(&m3_bytes).unwrap();
    let mut div = Div::zeroed();
    assert_eq!(m3.regions(&div).unwrap().0.len(), 0);
    div.regions = Reference { entries: 1, index: 9999, flags: 0 };
    assert!(m3.regions(&div).is_err());
    div.faces = Reference { entries: 1_000_000, index: 1, flags: 0 };
    assert!(m3.face_indices(&div).is_err());
}

#[test]
fn a_huge_region_count_does_not_allocate_it() {
    // Found by the pipeline fuzzer: a corrupt REGN count used to be passed
    // straight to `Vec::with_capacity`, asking for ~200 GB.
    let bytes = ModelSpec::quad().build();
    let m3 = parse(&bytes).unwrap();
    let mut div = m3.divisions().unwrap()[0];
    div.regions.entries = u32::MAX;
    let (regions, _) = m3.regions(&div).unwrap();
    assert!(regions.len() < 1000);
}

#[test]
fn sequences_must_fit() {
    let mut spec = animated();
    spec.sequences.push(SeqSpec { name: "B".into(), stcs: vec![] });
    let mut bytes = spec.build();
    let m3 = parse(&bytes).unwrap();
    assert_eq!(m3.sequences().unwrap().len(), 2);
    assert_eq!(m3.sequence_groups().unwrap().len(), 2);

    // Point MODL.sequences at far more records than exist.
    assert!(m3.find_tag(b"LDOM").is_some());
    let at = find_modl(&bytes) + 16;
    bytes[at..at + 4].copy_from_slice(&1_000_000u32.to_le_bytes());
    assert!(parse(&bytes).unwrap().sequences().is_err());
    bytes[at + 4..at + 8].copy_from_slice(&9999u32.to_le_bytes());
    assert!(parse(&bytes).unwrap().sequences().is_err());
}

/// Byte offset of the MODL payload.
fn find_modl(bytes: &[u8]) -> usize {
    let index = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let n = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    (0..n)
        .map(|i| &bytes[index + i * 16..index + i * 16 + 16])
        .find(|e| &e[..4] == b"LDOM")
        .map(|e| u32::from_le_bytes(e[4..8].try_into().unwrap()) as usize)
        .unwrap()
}

#[test]
fn invalid_utf8_names_are_errors() {
    let mut w = M3Writer::new(*b"43DM");
    let i = w.raw("CHAR", 0, vec![0xff, 0xfe, 0], 3);
    let bytes = w.finish();
    let m3 = parse(&bytes).unwrap();
    assert!(m3.read_char(&Reference { entries: 3, index: i, flags: 0 }).is_err());
}

#[test]
fn misaligned_payloads_are_copied_not_cast() {
    // Shift every payload by one byte so no `u16`/`u32` table is aligned.
    let spec = animated();
    let clean = spec.build();
    let mut shifted = vec![0u8; clean.len() + 1];
    let index = u32::from_le_bytes(clean[4..8].try_into().unwrap()) as usize;
    let n = u32::from_le_bytes(clean[8..12].try_into().unwrap()) as usize;
    shifted[..16].copy_from_slice(&clean[..16]);
    shifted[17..].copy_from_slice(&clean[16..]);
    // New tag table: same entries, offsets + 1, table itself at index + 1 — but
    // 4-byte aligned for the cast, so move it to the end.
    let table_at = shifted.len().next_multiple_of(4);
    shifted.resize(table_at, 0);
    for i in 0..n {
        let mut e: [u8; 16] = clean[index + i * 16..index + i * 16 + 16].try_into().unwrap();
        let off = u32::from_le_bytes(e[4..8].try_into().unwrap()) + 1;
        e[4..8].copy_from_slice(&off.to_le_bytes());
        shifted.extend_from_slice(&e);
    }
    shifted[4..8].copy_from_slice(&len32(table_at).to_le_bytes());
    let a = m3_to_glb::Converter::new().convert(&clean).unwrap();
    let b = m3_to_glb::Converter::new().convert(&shifted).unwrap();
    assert_eq!(a.bytes, b.bytes);
}

#[test]
fn layer_records_past_the_end_of_the_file() {
    // Every LAYR accessor must bounds-check the record it is pointed at, even
    // when the tag table places that record beyond the last byte.
    let mut spec = ModelSpec::quad();
    spec.materials[0].layers = vec![("diff", LayerSpec { texture: "t.dds".into(), color: Some([1; 4]), ..LayerSpec::default() })];
    let mut bytes = spec.build();
    let index = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let n = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let far = (len32(bytes.len()) - 8).to_le_bytes();
    for i in 0..n {
        let e = index + i * 16;
        if &bytes[e..e + 4] == b"RYAL" {
            bytes[e + 4..e + 8].copy_from_slice(&far);
        }
    }
    let m3 = parse(&bytes).unwrap();
    let r = m3.mat_layer_ref(0, "diff").unwrap();
    assert!(m3.read_layer_bitmap_ref(&r).is_none());
    assert!(m3.layer_color(0, "diff").is_none());
    assert_eq!(m3.read_layer_uv_tiling(&r), (1.0, 1.0));
}
