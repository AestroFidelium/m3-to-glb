//! Texture lookup by the paths M3 files actually store: any directory, any
//! case, any extension — only the file stem decides.

use bolero::{TypeGenerator, check};
use m3_to_glb::TextureCache;

const INDEXED: &str = "Unit_Diff.png";

/// The directory part of an M3 texture path.
#[derive(Debug, Clone, Copy, TypeGenerator)]
enum Dir {
    None,
    AssetsTexturesBackslash,
    AssetsTexturesSlash,
    TexturesUpper,
    Nested,
}

#[derive(Debug, Clone, Copy, TypeGenerator)]
enum Case {
    Lower,
    Upper,
    AsIndexed,
}

#[derive(Debug, Clone, Copy, TypeGenerator)]
enum Ext {
    Dds,
    DdsUpper,
    Png,
    Tga,
    None,
}

/// Whether the path names the indexed texture or one that is not there.
#[derive(Debug, Clone, Copy, TypeGenerator)]
enum Stem {
    Indexed,
    Missing,
}

#[derive(Debug, TypeGenerator)]
struct M3Path {
    dir: Dir,
    stem: Stem,
    case: Case,
    ext: Ext,
}

impl M3Path {
    fn render(&self) -> String {
        let dir = match self.dir {
            Dir::None => "",
            Dir::AssetsTexturesBackslash => "Assets\\Textures\\",
            Dir::AssetsTexturesSlash => "assets/textures/",
            Dir::TexturesUpper => "TEXTURES\\",
            Dir::Nested => "Assets/Textures/Units/Hero/",
        };
        let stem = match self.stem {
            Stem::Indexed => "Unit_Diff",
            Stem::Missing => "Unit_Norm",
        };
        let stem = match self.case {
            Case::Lower => stem.to_lowercase(),
            Case::Upper => stem.to_uppercase(),
            Case::AsIndexed => stem.to_owned(),
        };
        let ext = match self.ext {
            Ext::Dds => ".dds",
            Ext::DdsUpper => ".DDS",
            Ext::Png => ".png",
            Ext::Tga => ".tga",
            Ext::None => "",
        };
        format!("{dir}{stem}{ext}")
    }
}

#[test]
fn a_texture_is_found_by_its_stem_alone() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(INDEXED), b"x").unwrap();
    std::fs::write(dir.path().join("notes.txt"), b"not a texture").unwrap();
    let cache = TextureCache::build(dir.path().to_str().unwrap()).unwrap();
    let indexed = dir.path().join(INDEXED);

    check!().with_type::<M3Path>().for_each(|path| {
        let m3_path = path.render();
        let found = cache.find(&m3_path);
        match path.stem {
            Stem::Indexed => {
                assert_eq!(found, Some(&indexed), "{m3_path}");
                // The MIME type follows the file on disk, not the M3 path.
                let mime = cache.find_with_mime(&m3_path).map(|(_, mime)| mime);
                assert_eq!(mime, Some("image/png"), "{m3_path}");
            }
            Stem::Missing => assert_eq!(found, None, "{m3_path}"),
        }
    });
}
