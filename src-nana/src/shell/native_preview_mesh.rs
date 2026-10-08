//! 从 OBJ、STL、glTF/GLB、3MF 和 VRM 里取出三角网格并光栅化。
//!
//! Nana 的 GpuView 只有内置调色板着色器，没有网格管线，所以画进预览纹理。
//! FBX 和 BLEND 解析不了网格，只保留文件头摘要。不嵌入 Three.js。

use super::super::bridge::{MeshData, NativeLoad, PageFrame};

const VIEW_W: u32 = 320;
const VIEW_H: u32 = 240;

/// 在结构摘要之外，能解析出三角形就附上一帧画面。
pub(super) fn load(bytes: &[u8]) -> Result<NativeLoad, String> {
    let mut text = super::read_model(bytes)?;
    let mesh = parse_mesh(bytes);
    if mesh.is_none() && (is_fbx(bytes) || is_blend(bytes)) {
        text.push_str("\n只报告文件头");
    } else if mesh.is_none() {
        eprintln!("Nana 模型没有可绘制的三角形，保留结构摘要");
    }
    let frames = mesh.as_ref().map(|mesh| vec![raster(mesh, 0.0, 1.0)]).unwrap_or_default();
    Ok(NativeLoad { text, frames, mesh, paged: false })
}

pub(super) fn raster(mesh: &MeshData, yaw: f32, zoom: f32) -> PageFrame {
    let mut rgba = vec![244u8; (VIEW_W * VIEW_H * 4) as usize];
    for x in 0..VIEW_W {
        put(&mut rgba, x, 0, 30, 30, 30, 255);
        put(&mut rgba, x, VIEW_H - 1, 30, 30, 30, 255);
    }
    for y in 0..VIEW_H {
        put(&mut rgba, 0, y, 30, 30, 30, 255);
        put(&mut rgba, VIEW_W - 1, y, 30, 30, 30, 255);
    }
    let (min, max) = bounds(mesh);
    let center = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5, (min[2] + max[2]) * 0.5];
    let span = (max[0] - min[0]).max(max[1] - min[1]).max(max[2] - min[2]).max(1e-4);
    let scale = (VIEW_W.min(VIEW_H) as f32 * 0.72 / span) * zoom;
    let cos = yaw.cos();
    let sin = yaw.sin();
    let mut painted = Vec::new();
    for tri in &mesh.triangles {
        let verts: Vec<[f32; 3]> = tri.iter().filter_map(|index| mesh.vertices.get(*index as usize).copied()).collect();
        if verts.len() != 3 {
            continue;
        }
        let projected: Vec<[f32; 3]> = verts
            .iter()
            .map(|vertex| {
                let x = vertex[0] - center[0];
                let y = vertex[1] - center[1];
                let z = vertex[2] - center[2];
                let xr = x * cos - z * sin;
                let zr = x * sin + z * cos;
                [VIEW_W as f32 * 0.5 + xr * scale, VIEW_H as f32 * 0.5 - y * scale, zr]
            })
            .collect();
        let depth = (projected[0][2] + projected[1][2] + projected[2][2]) / 3.0;
        painted.push((depth, projected));
    }
    painted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, tri) in painted {
        fill_triangle(&mut rgba, &tri);
    }
    PageFrame {
        width: VIEW_W,
        height: VIEW_H,
        rgba,
        text: String::new(),
        error: None,
    }
}

fn parse_mesh(bytes: &[u8]) -> Option<MeshData> {
    if bytes.len() >= 4 && &bytes[..4] == b"glTF" {
        return mesh_from_glb(bytes);
    }
    if looks_like_zip(bytes) {
        return mesh_from_3mf(bytes);
    }
    if is_fbx(bytes) || is_blend(bytes) {
        return None;
    }
    let head = bytes.iter().position(|byte| !byte.is_ascii_whitespace()).map(|index| &bytes[index..]).unwrap_or(bytes);
    if head.first() == Some(&b'{') {
        return mesh_from_gltf(bytes, None);
    }
    mesh_from_stl(bytes).or_else(|| mesh_from_obj(bytes))
}

fn is_fbx(bytes: &[u8]) -> bool {
    bytes.starts_with(b"Kaydara FBX Binary")
        || bytes.iter().position(|byte| !byte.is_ascii_whitespace()).is_some_and(|index| {
            bytes[index..].starts_with(b";") || bytes[index..].starts_with(b"FBX")
        })
}

fn is_blend(bytes: &[u8]) -> bool {
    bytes.starts_with(b"BLENDER")
}

fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && matches!(&bytes[..4], b"PK\x03\x04" | b"PK\x05\x06" | b"PK\x07\x08")
}

fn mesh_from_obj(bytes: &[u8]) -> Option<MeshData> {
    let text = String::from_utf8_lossy(bytes);
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("v ") {
            let mut nums = rest.split_whitespace();
            let x = nums.next()?.parse().ok()?;
            let y = nums.next()?.parse().ok()?;
            let z = nums.next()?.parse().ok()?;
            vertices.push([x, y, z]);
        } else if let Some(rest) = line.strip_prefix("f ") {
            let indices: Vec<i32> = rest
                .split_whitespace()
                .filter_map(|token| token.split('/').next()?.parse().ok())
                .collect();
            if indices.len() < 3 {
                continue;
            }
            for offset in 1..indices.len() - 1 {
                if let (Some(a), Some(b), Some(c)) = (
                    obj_index(indices[0], vertices.len()),
                    obj_index(indices[offset], vertices.len()),
                    obj_index(indices[offset + 1], vertices.len()),
                ) {
                    triangles.push([a, b, c]);
                }
            }
        }
    }
    finish(vertices, triangles)
}

fn obj_index(index: i32, len: usize) -> Option<u32> {
    let resolved = if index > 0 { index as usize - 1 } else if index < 0 { len.wrapping_add(index as usize) } else { return None };
    u32::try_from(resolved).ok().filter(|index| (*index as usize) < len)
}

fn mesh_from_stl(bytes: &[u8]) -> Option<MeshData> {
    if bytes.len() >= 84 {
        let count = u32::from_le_bytes(bytes[80..84].try_into().ok()?) as usize;
        if count > 0 && bytes.len() >= 84 + count * 50 {
            let mut vertices = Vec::new();
            let mut triangles = Vec::new();
            for index in 0..count.min(20_000) {
                let start = 84 + index * 50 + 12;
                for vertex in 0..3 {
                    let at = start + vertex * 12;
                    let x = f32::from_le_bytes(bytes[at..at + 4].try_into().ok()?);
                    let y = f32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?);
                    let z = f32::from_le_bytes(bytes[at + 8..at + 12].try_into().ok()?);
                    vertices.push([x, y, z]);
                }
                let base = (index * 3) as u32;
                triangles.push([base, base + 1, base + 2]);
            }
            if let Some(mesh) = finish(vertices, triangles) {
                return Some(mesh);
            }
        }
    }
    let text = String::from_utf8_lossy(bytes);
    if !text.trim_start().to_ascii_lowercase().starts_with("solid") {
        return None;
    }
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let mut current = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("vertex ") {
            let mut nums = rest.split_whitespace();
            let x = nums.next()?.parse().ok()?;
            let y = nums.next()?.parse().ok()?;
            let z = nums.next()?.parse().ok()?;
            current.push([x, y, z]);
            if current.len() == 3 {
                let base = vertices.len() as u32;
                vertices.extend(current.drain(..));
                triangles.push([base, base + 1, base + 2]);
            }
        }
    }
    finish(vertices, triangles)
}

fn mesh_from_glb(bytes: &[u8]) -> Option<MeshData> {
    if bytes.len() < 20 || &bytes[..4] != b"glTF" {
        return None;
    }
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
    if 20 + json_len > bytes.len() {
        return None;
    }
    let json = &bytes[20..20 + json_len];
    let bin_at = 20 + json_len;
    let bin = if bin_at + 8 <= bytes.len() {
        let bin_len = u32::from_le_bytes(bytes[bin_at..bin_at + 4].try_into().ok()?) as usize;
        let bin_type = u32::from_le_bytes(bytes[bin_at + 4..bin_at + 8].try_into().ok()?);
        if bin_type == 0x004E_4942 && bin_at + 8 + bin_len <= bytes.len() {
            Some(&bytes[bin_at + 8..bin_at + 8 + bin_len])
        } else {
            None
        }
    } else {
        None
    };
    mesh_from_gltf(json, bin)
}

fn mesh_from_gltf(json: &[u8], bin: Option<&[u8]>) -> Option<MeshData> {
    let value: serde_json::Value = serde_json::from_slice(json).ok()?;
    let primitive = value.pointer("/meshes/0/primitives/0")?;
    let position = primitive.pointer("/attributes/POSITION")?.as_u64()? as usize;
    let indices = primitive.get("indices").and_then(|item| item.as_u64()).map(|item| item as usize);
    let buffers = gltf_buffer(&value, bin)?;
    let vertices = read_vec3(&value, &buffers, position)?;
    let triangles = if let Some(indices) = indices {
        read_indices(&value, &buffers, indices, vertices.len())?
    } else {
        (0..vertices.len() / 3).map(|index| [(index * 3) as u32, (index * 3 + 1) as u32, (index * 3 + 2) as u32]).collect()
    };
    finish(vertices, triangles)
}

fn gltf_buffer(value: &serde_json::Value, bin: Option<&[u8]>) -> Option<Vec<u8>> {
    if let Some(bin) = bin {
        return Some(bin.to_vec());
    }
    let uri = value.pointer("/buffers/0/uri")?.as_str()?;
    let data = uri.strip_prefix("data:application/octet-stream;base64,").or_else(|| uri.strip_prefix("data:application/gltf-buffer;base64,"))?;
    base64_decode(data)
}

fn read_vec3(value: &serde_json::Value, buffer: &[u8], accessor_index: usize) -> Option<Vec<[f32; 3]>> {
    let accessor = value.pointer(&format!("/accessors/{accessor_index}"))?;
    if accessor.get("type")?.as_str()? != "VEC3" || accessor.get("componentType")?.as_u64()? != 5126 {
        eprintln!("Nana glTF 位置不是 32 位浮点 VEC3");
        return None;
    }
    let count = accessor.get("count")?.as_u64()? as usize;
    let view = accessor.get("bufferView")?.as_u64()? as usize;
    let (offset, length) = view_range(value, view)?;
    let extra = accessor.get("byteOffset").and_then(|item| item.as_u64()).unwrap_or(0) as usize;
    let start = offset + extra;
    if start + count * 12 > length.min(buffer.len()) && start + count * 12 > buffer.len() {
        return None;
    }
    let mut vertices = Vec::with_capacity(count.min(20_000));
    for index in 0..count.min(20_000) {
        let at = start + index * 12;
        if at + 12 > buffer.len() {
            break;
        }
        vertices.push([
            f32::from_le_bytes(buffer[at..at + 4].try_into().ok()?),
            f32::from_le_bytes(buffer[at + 4..at + 8].try_into().ok()?),
            f32::from_le_bytes(buffer[at + 8..at + 12].try_into().ok()?),
        ]);
    }
    Some(vertices)
}

fn read_indices(value: &serde_json::Value, buffer: &[u8], accessor_index: usize, vertex_count: usize) -> Option<Vec<[u32; 3]>> {
    let accessor = value.pointer(&format!("/accessors/{accessor_index}"))?;
    let component = accessor.get("componentType")?.as_u64()?;
    let count = accessor.get("count")?.as_u64()? as usize;
    let view = accessor.get("bufferView")?.as_u64()? as usize;
    let (offset, _) = view_range(value, view)?;
    let extra = accessor.get("byteOffset").and_then(|item| item.as_u64()).unwrap_or(0) as usize;
    let start = offset + extra;
    let width = match component {
        5121 => 1,
        5123 => 2,
        5125 => 4,
        other => {
            eprintln!("Nana glTF 索引类型不支持：{other}");
            return None;
        }
    };
    let mut indices = Vec::new();
    for index in 0..count.min(60_000) {
        let at = start + index * width;
        if at + width > buffer.len() {
            break;
        }
        let value = match width {
            1 => u32::from(buffer[at]),
            2 => u32::from(u16::from_le_bytes(buffer[at..at + 2].try_into().ok()?)),
            _ => u32::from_le_bytes(buffer[at..at + 4].try_into().ok()?),
        };
        if value as usize >= vertex_count {
            continue;
        }
        indices.push(value);
    }
    let triangles = indices.chunks_exact(3).map(|chunk| [chunk[0], chunk[1], chunk[2]]).collect();
    Some(triangles)
}

fn view_range(value: &serde_json::Value, index: usize) -> Option<(usize, usize)> {
    let view = value.pointer(&format!("/bufferViews/{index}"))?;
    let offset = view.get("byteOffset").and_then(|item| item.as_u64()).unwrap_or(0) as usize;
    let length = view.get("byteLength")?.as_u64()? as usize;
    Some((offset, offset + length))
}

fn mesh_from_3mf(bytes: &[u8]) -> Option<MeshData> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let mut xml = String::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).ok()?;
        let name = file.name().replace('\\', "/");
        if name.to_ascii_lowercase().ends_with("3dmodel.model") {
            use std::io::Read;
            file.read_to_string(&mut xml).ok()?;
            break;
        }
    }
    if xml.is_empty() {
        return None;
    }
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for tag in xml.split('<').skip(1) {
        let token = tag.split(|ch: char| ch == '>' || ch.is_whitespace()).next().unwrap_or("");
        let local = token.rsplit(':').next().unwrap_or(token);
        if local == "vertex" {
            let x = attr(tag, "x")?;
            let y = attr(tag, "y")?;
            let z = attr(tag, "z")?;
            vertices.push([x, y, z]);
        } else if local == "triangle" {
            let v1 = attr_u32(tag, "v1")?;
            let v2 = attr_u32(tag, "v2")?;
            let v3 = attr_u32(tag, "v3")?;
            if (v1 as usize) < vertices.len() && (v2 as usize) < vertices.len() && (v3 as usize) < vertices.len() {
                triangles.push([v1, v2, v3]);
            }
        }
    }
    finish(vertices, triangles)
}

fn attr(tag: &str, name: &str) -> Option<f32> {
    let key = format!("{name}=\"");
    let rest = tag.split(&key).nth(1)?;
    let value = rest.split('"').next()?;
    value.parse().ok()
}

fn attr_u32(tag: &str, name: &str) -> Option<u32> {
    attr(tag, name).map(|value| value as u32)
}

fn finish(vertices: Vec<[f32; 3]>, triangles: Vec<[u32; 3]>) -> Option<MeshData> {
    if vertices.is_empty() || triangles.is_empty() {
        return None;
    }
    Some(MeshData { vertices, triangles })
}

fn bounds(mesh: &MeshData) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for vertex in &mesh.vertices {
        for axis in 0..3 {
            min[axis] = min[axis].min(vertex[axis]);
            max[axis] = max[axis].max(vertex[axis]);
        }
    }
    (min, max)
}

fn fill_triangle(rgba: &mut [u8], tri: &[[f32; 3]]) {
    let xs = [tri[0][0], tri[1][0], tri[2][0]];
    let ys = [tri[0][1], tri[1][1], tri[2][1]];
    let min_x = xs.iter().cloned().fold(f32::MAX, f32::min).floor().max(0.0) as i32;
    let max_x = xs.iter().cloned().fold(f32::MIN, f32::max).ceil().min(VIEW_W as f32 - 1.0) as i32;
    let min_y = ys.iter().cloned().fold(f32::MAX, f32::min).floor().max(0.0) as i32;
    let max_y = ys.iter().cloned().fold(f32::MIN, f32::max).ceil().min(VIEW_H as f32 - 1.0) as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if inside(x as f32 + 0.5, y as f32 + 0.5, tri) {
                put(rgba, x as u32, y as u32, 70, 110, 160, 255);
            }
        }
    }
}

fn inside(x: f32, y: f32, tri: &[[f32; 3]]) -> bool {
    let area = edge(tri[0], tri[1], tri[2]);
    if area.abs() < 1e-4 {
        return false;
    }
    let w0 = edge(tri[1], tri[2], [x, y, 0.0]) / area;
    let w1 = edge(tri[2], tri[0], [x, y, 0.0]) / area;
    let w2 = edge(tri[0], tri[1], [x, y, 0.0]) / area;
    w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
}

fn edge(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    (c[0] - a[0]) * (b[1] - a[1]) - (c[1] - a[1]) * (b[0] - a[0])
}

fn put(rgba: &mut [u8], x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
    if x >= VIEW_W || y >= VIEW_H {
        return;
    }
    let index = ((y * VIEW_W + x) * 4) as usize;
    rgba[index] = r;
    rgba[index + 1] = g;
    rgba[index + 2] = b;
    rgba[index + 3] = a;
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in text.bytes() {
        if byte == b'=' {
            break;
        }
        let Some(value) = TABLE.iter().position(|item| *item == byte) else {
            if byte.is_ascii_whitespace() {
                continue;
            }
            eprintln!("Nana glTF base64 无法解码");
            return None;
        };
        buffer = (buffer << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}
