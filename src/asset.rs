use crate::{
    config::WebAsset,
    hash::Hash,
    lockfile::LockfileEntry,
    util::{alpha_bleed::alpha_bleed, svg::svg_to_png},
};
use anyhow::{Context, bail};
use bytes::Bytes;
use image::DynamicImage;
use relative_path::RelativePathBuf;
use resvg::usvg::fontdb::{self};
use serde::Serialize;
use std::{
    collections::HashMap,
    ffi::OsStr,
    fmt,
    io::{Cursor, Read},
    sync::Arc,
};

type AssetCtor = fn(&[u8]) -> anyhow::Result<AssetType>;

const SUPPORTED_EXTENSIONS: &[(&str, AssetCtor)] = &[
    ("mp3", |_| Ok(AssetType::Audio(AudioType::Mp3))),
    ("ogg", |_| Ok(AssetType::Audio(AudioType::Ogg))),
    ("flac", |_| Ok(AssetType::Audio(AudioType::Flac))),
    ("wav", |_| Ok(AssetType::Audio(AudioType::Wav))),
    ("png", |_| Ok(AssetType::Image(ImageType::Png))),
    ("svg", |_| Ok(AssetType::Image(ImageType::Png))),
    ("jpg", |_| Ok(AssetType::Image(ImageType::Jpg))),
    ("jpeg", |_| Ok(AssetType::Image(ImageType::Jpg))),
    ("bmp", |_| Ok(AssetType::Image(ImageType::Bmp))),
    ("tga", |_| Ok(AssetType::Image(ImageType::Tga))),
    ("fbx", |_| Ok(AssetType::Model(ModelType::Fbx))),
    ("gltf", |_| Ok(AssetType::Model(ModelType::GltfJson))),
    ("glb", |_| Ok(AssetType::Model(ModelType::GltfBinary))),
    ("rbxm", |data| {
        let format = RobloxModelFormat::Binary;
        if is_animation(data, &format)? {
            Ok(AssetType::Animation)
        } else {
            Ok(AssetType::Model(ModelType::Roblox))
        }
    }),
    ("rbxmx", |data| {
        let format = RobloxModelFormat::Xml;
        if is_animation(data, &format)? {
            Ok(AssetType::Animation)
        } else {
            Ok(AssetType::Model(ModelType::Roblox))
        }
    }),
    ("mp4", |_| Ok(AssetType::Video(VideoType::Mp4))),
    ("mov", |_| Ok(AssetType::Video(VideoType::Mov))),
];

pub fn is_supported_extension(ext: &OsStr) -> bool {
    SUPPORTED_EXTENSIONS.iter().any(|(e, _)| *e == ext)
}

pub struct Asset {
    /// Relative to Input prefix
    pub path: RelativePathBuf,
    pub data: Bytes,
    pub ty: AssetType,
    pub ext: String,
    /// The hash before processing
    pub hash: Hash,
    is_svg: bool,
}

impl Asset {
    pub fn new(path: RelativePathBuf, data: Bytes) -> anyhow::Result<Self> {
        let mut ext = path
            .extension()
            .context("File has no extension")?
            .to_string();

        let ty = SUPPORTED_EXTENSIONS
            .iter()
            .find(|(e, _)| *e == ext)
            .map(|(_, func)| func(&data))
            .context("Unknown file type")??;

        let mut is_svg = false;
        if ext == "svg" {
            ext = "png".to_string();
            is_svg = true;
        }

        let hash = Hash::new_from_bytes(&data);

        Ok(Self {
            path,
            data,
            ty,
            ext,
            hash,
            is_svg,
        })
    }

    pub fn process(&mut self, font_db: Arc<fontdb::Database>, bleed: bool) -> anyhow::Result<()> {
        if self.is_svg {
            self.data = svg_to_png(&self.data, font_db)
                .context("Failed to convert to PNG")?
                .into();
        }

        if bleed && let AssetType::Image(_) = self.ty {
            let mut image: DynamicImage = image::load_from_memory(&self.data)?;
            alpha_bleed(&mut image);

            let mut writer = Cursor::new(Vec::new());
            image.write_to(&mut writer, image::ImageFormat::Png)?;
            self.data = writer.into_inner().into();
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AssetType {
    Model(ModelType),
    Animation,
    Image(ImageType),
    Audio(AudioType),
    Video(VideoType),
}

impl AssetType {
    // https://create.roblox.com/docs/cloud/guides/usage-assets#supported-asset-types-and-limits

    pub fn asset_type(&self) -> &'static str {
        match self {
            AssetType::Model(_) => "Model",
            AssetType::Animation => "Animation",
            AssetType::Image(_) => "Image",
            AssetType::Audio(_) => "Audio",
            AssetType::Video(_) => "Video",
        }
    }

    pub fn file_type(&self) -> &'static str {
        match self {
            AssetType::Animation => "model/x-rbxm",

            AssetType::Model(ModelType::Fbx) => "model/fbx",
            AssetType::Model(ModelType::GltfJson) => "model/gltf+json",
            AssetType::Model(ModelType::GltfBinary) => "model/gltf-binary",
            AssetType::Model(ModelType::Roblox) => "model/x-rbxm",

            AssetType::Image(ImageType::Png) => "image/png",
            AssetType::Image(ImageType::Jpg) => "image/jpeg",
            AssetType::Image(ImageType::Bmp) => "image/bmp",
            AssetType::Image(ImageType::Tga) => "image/tga",

            AssetType::Audio(AudioType::Mp3) => "audio/mpeg",
            AssetType::Audio(AudioType::Ogg) => "audio/ogg",
            AssetType::Audio(AudioType::Flac) => "audio/flac",
            AssetType::Audio(AudioType::Wav) => "audio/wav",

            AssetType::Video(VideoType::Mp4) => "video/mp4",
            AssetType::Video(VideoType::Mov) => "video/mov",
        }
    }
}

impl Serialize for AssetType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.asset_type())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AudioType {
    Mp3,
    Ogg,
    Flac,
    Wav,
}

#[derive(Debug, Clone, Copy)]
pub enum ImageType {
    Png,
    Jpg,
    Bmp,
    Tga,
}

#[derive(Debug, Clone, Copy)]
pub enum ModelType {
    Fbx,
    GltfJson,
    GltfBinary,
    Roblox,
}

#[derive(Debug, Clone, Copy)]
pub enum VideoType {
    Mp4,
    Mov,
}

pub fn is_animation(data: &[u8], format: &RobloxModelFormat) -> anyhow::Result<bool> {
    let first_class = match format {
        RobloxModelFormat::Binary => first_binary_root_class(data)?,
        RobloxModelFormat::Xml => {
            let dom = rbx_xml::from_reader(data, Default::default())?;
            let children = dom.root().children();

            let first_ref = *children.first().context("No children found in root")?;
            let first = dom
                .get_by_ref(first_ref)
                .context("Failed to get first child")?;

            first.class.to_string()
        }
    };

    Ok(is_animation_class(&first_class))
}

fn is_animation_class(class: &str) -> bool {
    class == "KeyframeSequence" || class == "CurveAnimation"
}

#[derive(Debug, Clone)]
pub enum RobloxModelFormat {
    Binary,
    Xml,
}

const RBXM_MAGIC_HEADER: &[u8; 8] = b"<roblox!";
const RBXM_SIGNATURE: &[u8; 6] = b"\x89\xff\x0d\x0a\x1a\x0a";
const RBXM_FILE_VERSION: u16 = 0;
const ZSTD_MAGIC_NUMBER: &[u8; 4] = &[0x28, 0xb5, 0x2f, 0xfd];

struct BinaryChunk {
    name: [u8; 4],
    data: Vec<u8>,
}

fn first_binary_root_class(data: &[u8]) -> anyhow::Result<String> {
    let mut reader = Cursor::new(data);
    read_binary_header(&mut reader)?;

    let mut classes_by_ref = HashMap::new();

    loop {
        let chunk = read_binary_chunk(&mut reader)?;

        match &chunk.name {
            b"INST" => read_inst_chunk(&chunk.data, &mut classes_by_ref)?,
            b"PRNT" => {
                if let Some(class) = read_first_root_class(&chunk.data, &classes_by_ref)? {
                    return Ok(class);
                }
            }
            b"END\0" => break,
            _ => {}
        }
    }

    bail!("No children found in root")
}

fn read_binary_header<R: Read>(reader: &mut R) -> anyhow::Result<()> {
    let magic_header = read_exact::<8, _>(reader)?;
    if &magic_header != RBXM_MAGIC_HEADER {
        bail!("Invalid Roblox binary model header");
    }

    let signature = read_exact::<6, _>(reader)?;
    if &signature != RBXM_SIGNATURE {
        bail!("Invalid Roblox binary model signature");
    }

    let version = read_le_u16(reader)?;
    if version != RBXM_FILE_VERSION {
        bail!("Unknown Roblox binary model version {version}");
    }

    let _num_types = read_le_u32(reader)?;
    let _num_instances = read_le_u32(reader)?;

    let reserved = read_exact::<8, _>(reader)?;
    if reserved != [0; 8] {
        bail!("Invalid Roblox binary model reserved header bytes");
    }

    Ok(())
}

fn read_binary_chunk<R: Read>(reader: &mut R) -> anyhow::Result<BinaryChunk> {
    let name = read_exact::<4, _>(reader)?;
    let compressed_len = read_le_u32(reader)?;
    let len = read_le_u32(reader)?;
    let reserved = read_le_u32(reader)?;

    if reserved != 0 {
        bail!("Invalid Roblox binary chunk reserved bytes");
    }

    let data = if compressed_len == 0 {
        read_bytes(reader, len as usize)?
    } else {
        let compressed_data = read_bytes(reader, compressed_len as usize)?;
        if compressed_data.starts_with(ZSTD_MAGIC_NUMBER) {
            zstd::bulk::decompress(&compressed_data, len as usize)?
        } else {
            lz4_flex::block::decompress(&compressed_data, len as usize)
                .map_err(|e| anyhow::anyhow!(e))?
        }
    };

    if data.len() != len as usize {
        bail!("Invalid Roblox binary chunk length");
    }

    Ok(BinaryChunk { name, data })
}

fn read_inst_chunk(data: &[u8], classes_by_ref: &mut HashMap<i32, String>) -> anyhow::Result<()> {
    let mut reader = Cursor::new(data);
    let _type_id = read_le_u32(&mut reader)?;
    let type_name = read_string(&mut reader)?;
    let _object_format = read_u8(&mut reader)?;
    let number_instances = read_le_u32(&mut reader)? as usize;
    let referents = read_referent_array(&mut reader, number_instances)?;

    for referent in referents {
        classes_by_ref.insert(referent, type_name.clone());
    }

    Ok(())
}

fn read_first_root_class(
    data: &[u8],
    classes_by_ref: &HashMap<i32, String>,
) -> anyhow::Result<Option<String>> {
    let mut reader = Cursor::new(data);
    let version = read_u8(&mut reader)?;
    if version != 0 {
        bail!("Unknown PRNT chunk version {version}");
    }

    let number_objects = read_le_u32(&mut reader)? as usize;
    let subjects = read_referent_array(&mut reader, number_objects)?;
    let parents = read_referent_array(&mut reader, number_objects)?;

    for (subject, parent) in subjects.into_iter().zip(parents) {
        if parent == -1 {
            let class = classes_by_ref
                .get(&subject)
                .with_context(|| format!("Root instance {subject} was not declared"))?;
            return Ok(Some(class.clone()));
        }
    }

    Ok(None)
}

fn read_exact<const N: usize, R: Read>(reader: &mut R) -> anyhow::Result<[u8; N]> {
    let mut bytes = [0; N];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_bytes<R: Read>(reader: &mut R, len: usize) -> anyhow::Result<Vec<u8>> {
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_u8<R: Read>(reader: &mut R) -> anyhow::Result<u8> {
    Ok(read_exact::<1, _>(reader)?[0])
}

fn read_le_u16<R: Read>(reader: &mut R) -> anyhow::Result<u16> {
    Ok(u16::from_le_bytes(read_exact(reader)?))
}

fn read_le_u32<R: Read>(reader: &mut R) -> anyhow::Result<u32> {
    Ok(u32::from_le_bytes(read_exact(reader)?))
}

fn read_string<R: Read>(reader: &mut R) -> anyhow::Result<String> {
    let len = read_le_u32(reader)? as usize;
    let bytes = read_bytes(reader, len)?;
    Ok(String::from_utf8(bytes)?)
}

fn read_referent_array<R: Read>(reader: &mut R, len: usize) -> anyhow::Result<Vec<i32>> {
    let byte_len = len
        .checked_mul(4)
        .context("Roblox binary referent array is too large")?;
    let buffer = read_bytes(reader, byte_len)?;
    let mut referents = Vec::with_capacity(len);
    let mut last = 0;

    for index in 0..len {
        let bytes = [
            buffer[index],
            buffer[index + len],
            buffer[index + len * 2],
            buffer[index + len * 3],
        ];
        let value = untransform_i32(i32::from_be_bytes(bytes)) + last;
        last = value;
        referents.push(value);
    }

    Ok(referents)
}

fn untransform_i32(value: i32) -> i32 {
    ((value as u32) >> 1) as i32 ^ -(value & 1)
}

#[derive(Debug, Clone)]
pub enum AssetRef {
    Cloud(u64),
    Studio(String),
}

impl fmt::Display for AssetRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetRef::Cloud(id) => write!(f, "rbxassetid://{id}"),
            AssetRef::Studio(name) => write!(f, "rbxasset://{name}"),
        }
    }
}

impl From<WebAsset> for AssetRef {
    fn from(value: WebAsset) -> Self {
        AssetRef::Cloud(value.id)
    }
}

impl From<&LockfileEntry> for AssetRef {
    fn from(value: &LockfileEntry) -> Self {
        AssetRef::Cloud(value.asset_id)
    }
}

#[cfg(test)]
mod tests {
    use super::{RobloxModelFormat, is_animation};
    use rbx_dom_weak::{
        InstanceBuilder, WeakDom,
        types::{Color3uint8, Tags},
    };

    fn serialize_binary_model(root: InstanceBuilder) -> Vec<u8> {
        let dom = WeakDom::new(root);
        let database = rbx_reflection::ReflectionDatabase::new();
        let mut output = Vec::new();

        rbx_binary::Serializer::new()
            .reflection_database(&database)
            .serialize(&mut output, &dom, &[dom.root_ref()])
            .unwrap();

        output
    }

    #[test]
    fn detects_binary_animation_with_tags_property() {
        let mut tags = Tags::new();
        tags.push("lava");

        let data = serialize_binary_model(
            InstanceBuilder::new("KeyframeSequence")
                .with_child(InstanceBuilder::new("Animation").with_property("Tags", tags)),
        );

        assert!(is_animation(&data, &RobloxModelFormat::Binary).unwrap());
    }

    #[test]
    fn detects_binary_model_with_color3uint8_property() {
        let data = serialize_binary_model(
            InstanceBuilder::new("Model").with_child(
                InstanceBuilder::new("MeshPart")
                    .with_property("Color3uint8", Color3uint8::new(255, 128, 0)),
            ),
        );

        assert!(!is_animation(&data, &RobloxModelFormat::Binary).unwrap());
    }
}
