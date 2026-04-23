// Spine Runtimes License Agreement
// Last updated April 5, 2025. Replaces all prior versions.
//
// Copyright (c) 2013-2025, Esoteric Software LLC
//
// Integration of the Spine Runtimes into software or otherwise creating
// derivative works of the Spine Runtimes is permitted under the terms and
// conditions of Section 2 of the Spine Editor License Agreement:
// http://esotericsoftware.com/spine-editor-license
//
// Otherwise, it is permitted to integrate the Spine Runtimes into software
// or otherwise create derivative works of the Spine Runtimes (collectively,
// "Products"), provided that each user of the Products must obtain their own
// Spine Editor license and redistribution of the Products in any form must
// include this license and copyright notice.
//
// THE SPINE RUNTIMES ARE PROVIDED BY ESOTERIC SOFTWARE LLC "AS IS" AND ANY
// EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
// WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL ESOTERIC SOFTWARE LLC BE LIABLE FOR ANY
// DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
// (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES,
// BUSINESS INTERRUPTION, OR LOSS OF USE, DATA, OR PROFITS) HOWEVER CAUSED AND
// ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
// THE SPINE RUNTIMES, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
//
// ============================================================================
//
// Reference implementation of a software renderer that consumes
// `SkeletonRenderer::commands()` and rasterizes each `RenderCommand` to a
// PNG — no GPU, no windowing, no render backend. Useful to confirm the
// runtime's output is structurally correct independent of any engine
// integration (e.g. dm_spine_bevy).
//
// Keep it simple:
// - Scanline barycentric triangle fill
// - Nearest-neighbour UV sample
// - Premultiplied-alpha blend in 0..1 float space
// - Only the `Normal` blend mode (enough for spineboy / most example rigs)
//
// Usage: `cargo run --example software_render`
// Env overrides:
//   SPINE_RIG             name under spine-runtimes/examples (default spineboy)
//   SPINE_SKEL            .skel filename stem (default spineboy-pro)
//   SPINE_ATLAS           .atlas filename stem, no extension (default spineboy-pma)
//   SPINE_ANIM            animation name or "" for setup pose (default walk)
//   SPINE_TIME            seconds of animation to advance (default 1.0)
//   SPINE_OUT             output PNG path (default software_render.png)
//   SPINE_W / SPINE_H     output dimensions (default 1280x720)
//   SPINE_CAM_Y           world Y placed at the vertical centre (default 300)

use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::{Rgba, RgbaImage};

use dm_spine_runtime::animation::{AnimationState, AnimationStateData};
use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::data::BlendMode;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::render::SkeletonRenderer;
use dm_spine_runtime::skeleton::{Physics, Skeleton};

const FIXED_STEP: f32 = 1.0 / 60.0;

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key).ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}

fn env_str(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn main() {
    let rig = env_str("SPINE_RIG", "spineboy");
    let skel_stem = env_str("SPINE_SKEL", "spineboy-pro");
    let atlas_stem = env_str("SPINE_ATLAS", "spineboy-pma");
    let anim = env_str("SPINE_ANIM", "walk");
    let total_time: f32 = env_or("SPINE_TIME", 1.0);
    let out_path = env_str("SPINE_OUT", "software_render.png");
    let width: u32 = env_or("SPINE_W", 1280);
    let height: u32 = env_or("SPINE_H", 720);
    let cam_y: f32 = env_or("SPINE_CAM_Y", 300.0);

    let export_dir: PathBuf = ["..", "spine-runtimes", "examples", &rig, "export"]
        .iter()
        .collect();

    let atlas_path = export_dir.join(format!("{atlas_stem}.atlas"));
    let skel_path = export_dir.join(format!("{skel_stem}.skel"));

    let atlas_src = std::fs::read_to_string(&atlas_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", atlas_path.display()));
    let atlas = Atlas::parse(&atlas_src).unwrap();
    let pages = load_atlas_pages(&atlas, &export_dir);

    let mut attachment_loader = AtlasAttachmentLoader::new(&atlas);
    let bytes = std::fs::read(&skel_path).unwrap();
    let data = SkeletonBinary::with_loader(&mut attachment_loader)
        .read(&bytes)
        .unwrap();
    let data = Arc::new(data);

    let mut skeleton = Skeleton::new(Arc::clone(&data));
    skeleton.update_cache();
    skeleton.set_to_setup_pose();
    skeleton.update_world_transform(Physics::None);

    let state_data = Arc::new(AnimationStateData::new(Arc::clone(&data)));
    let mut animation_state = AnimationState::new(state_data);

    if !anim.is_empty() {
        if let Err(e) = animation_state.set_animation_by_name(0, &anim, true) {
            panic!("set_animation_by_name({anim:?}): {e:?}");
        }
        // Advance in fixed steps so events / physics that depend on dt are
        // stable across runs.
        let mut remaining = total_time;
        while remaining > 0.0 {
            let step = remaining.min(FIXED_STEP);
            animation_state.update(step);
            let mut events = Vec::new();
            animation_state.apply(&mut skeleton, &mut events);
            skeleton.update_world_transform(Physics::Update);
            remaining -= step;
        }
    }

    let mut renderer = SkeletonRenderer::new();
    // Diagnostic: skip the batcher so one command = one slot. Makes bugs
    // that drop slots entirely visible via per-command counts / bounds.
    let unbatched = std::env::var("SPINE_UNBATCHED").is_ok();
    let cmds = if unbatched {
        renderer.render_unbatched(&skeleton).to_vec()
    } else {
        renderer.render(&skeleton).to_vec()
    };
    if unbatched {
        eprintln!("software_render: {} unbatched commands", cmds.len());
        for (i, c) in cmds.iter().enumerate() {
            let n = c.num_vertices();
            let mut xmin = f32::INFINITY;
            let mut xmax = f32::NEG_INFINITY;
            let mut ymin = f32::INFINITY;
            let mut ymax = f32::NEG_INFINITY;
            for k in 0..n {
                let x = c.positions[k * 2];
                let y = c.positions[k * 2 + 1];
                xmin = xmin.min(x);
                xmax = xmax.max(x);
                ymin = ymin.min(y);
                ymax = ymax.max(y);
            }
            eprintln!(
                "  cmd[{i:2}] verts={n:3} tris={:3} x=[{xmin:7.1}..{xmax:7.1}] y=[{ymin:7.1}..{ymax:7.1}]",
                c.indices.len() / 3
            );
        }
    }

    let mut img = RgbaImage::from_pixel(width, height, Rgba([30, 30, 30, 255]));

    let mut total_tris_drawn = 0usize;
    let mut total_tris_culled = 0usize;

    for cmd in &cmds {
        if cmd.blend_mode != BlendMode::Normal {
            eprintln!(
                "software_render: skipping {:?} blend-mode command ({} tris)",
                cmd.blend_mode,
                cmd.indices.len() / 3
            );
            continue;
        }
        let page = match pages.get(cmd.texture.0 as usize) {
            Some(p) => p,
            None => {
                eprintln!(
                    "software_render: no page for texture {:?} (commands will be dropped)",
                    cmd.texture
                );
                continue;
            }
        };

        // Runtime packs colors as 0xAARRGGBB. For spineboy setup the light
        // is 0xffffffff (premultiplied white).
        let light = cmd.colors.first().copied().unwrap_or(0xffff_ffff);
        let lc = unpack_argb(light);

        for tri in cmd.indices.chunks_exact(3) {
            let i0 = tri[0] as usize;
            let i1 = tri[1] as usize;
            let i2 = tri[2] as usize;
            let p0 = [cmd.positions[i0 * 2], cmd.positions[i0 * 2 + 1]];
            let p1 = [cmd.positions[i1 * 2], cmd.positions[i1 * 2 + 1]];
            let p2 = [cmd.positions[i2 * 2], cmd.positions[i2 * 2 + 1]];
            let uv0 = [cmd.uvs[i0 * 2], cmd.uvs[i0 * 2 + 1]];
            let uv1 = [cmd.uvs[i1 * 2], cmd.uvs[i1 * 2 + 1]];
            let uv2 = [cmd.uvs[i2 * 2], cmd.uvs[i2 * 2 + 1]];

            let drew = rasterize_triangle(
                &mut img, width, height, cam_y, p0, p1, p2, uv0, uv1, uv2, page, lc,
            );
            if drew {
                total_tris_drawn += 1;
            } else {
                total_tris_culled += 1;
            }
        }
    }

    img.save(&out_path).unwrap();
    eprintln!(
        "software_render: saved {} ({width}x{height}); {} commands, {} tris drawn, {} tris off-screen / degenerate",
        out_path, cmds.len(), total_tris_drawn, total_tris_culled,
    );
}

fn load_atlas_pages(atlas: &Atlas, dir: &Path) -> Vec<RgbaImage> {
    let mut pages = Vec::with_capacity(atlas.pages.len());
    for page in &atlas.pages {
        let path = dir.join(&page.name);
        let img = image::open(&path)
            .unwrap_or_else(|e| panic!("open atlas page {}: {e}", path.display()))
            .to_rgba8();
        if page.pma {
            pages.push(img);
        } else {
            // If the page is straight-alpha, premultiply on load so the
            // renderer can blend uniformly.
            let mut pma = img.clone();
            for p in pma.pixels_mut() {
                let a = p[3] as f32 / 255.0;
                p[0] = (p[0] as f32 * a) as u8;
                p[1] = (p[1] as f32 * a) as u8;
                p[2] = (p[2] as f32 * a) as u8;
            }
            pages.push(pma);
        }
    }
    pages
}

/// Barycentric scanline rasterizer for a single triangle. Samples the atlas
/// page at the interpolated UV (nearest-neighbour), multiplies by `light`
/// (premultiplied), blends with PMA OVER.
///
/// Returns `false` when the triangle is fully off-screen or degenerate
/// (zero-area) — lets the caller count those for diagnostic purposes.
#[allow(clippy::too_many_arguments)]
fn rasterize_triangle(
    img: &mut RgbaImage,
    width: u32,
    height: u32,
    cam_y: f32,
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
    uv0: [f32; 2],
    uv1: [f32; 2],
    uv2: [f32; 2],
    page: &RgbaImage,
    light: [f32; 4],
) -> bool {
    // World → screen: center X at width/2, flip Y (spine +Y up, image +Y down),
    // translate so cam_y sits at vertical centre of the frame.
    let wx = width as f32 * 0.5;
    let hy = height as f32 * 0.5;
    let to_screen = |p: [f32; 2]| [p[0] + wx, hy - (p[1] - cam_y)];

    let s0 = to_screen(p0);
    let s1 = to_screen(p1);
    let s2 = to_screen(p2);

    // Edge function for barycentric rasterization (Pineda's algorithm).
    let edge = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        (c[0] - a[0]) * (b[1] - a[1]) - (c[1] - a[1]) * (b[0] - a[0])
    };
    let area = edge(s0, s1, s2);
    if area.abs() < 1e-6 {
        return false;
    }
    let inv_area = 1.0 / area;

    let min_x = s0[0].min(s1[0]).min(s2[0]).floor().max(0.0) as u32;
    let max_x = s0[0].max(s1[0]).max(s2[0]).ceil().min(width as f32 - 1.0) as u32;
    let min_y = s0[1].min(s1[1]).min(s2[1]).floor().max(0.0) as u32;
    let max_y = s0[1].max(s1[1]).max(s2[1]).ceil().min(height as f32 - 1.0) as u32;
    if min_x > max_x || min_y > max_y {
        return false;
    }

    let tex_w = page.width() as f32;
    let tex_h = page.height() as f32;

    let mut any_drawn = false;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            // Sample at pixel centre.
            let pc = [x as f32 + 0.5, y as f32 + 0.5];
            let w0 = edge(s1, s2, pc) * inv_area;
            let w1 = edge(s2, s0, pc) * inv_area;
            let w2 = edge(s0, s1, pc) * inv_area;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let u = w0 * uv0[0] + w1 * uv1[0] + w2 * uv2[0];
            let v = w0 * uv0[1] + w1 * uv1[1] + w2 * uv2[1];
            let tx = (u * tex_w).clamp(0.0, tex_w - 1.0) as u32;
            let ty = (v * tex_h).clamp(0.0, tex_h - 1.0) as u32;
            let sample = page.get_pixel(tx, ty);
            let sr = sample[0] as f32 / 255.0;
            let sg = sample[1] as f32 / 255.0;
            let sb = sample[2] as f32 / 255.0;
            let sa = sample[3] as f32 / 255.0;
            // Already-PMA sample × already-PMA light → still PMA.
            let fr = sr * light[0];
            let fg = sg * light[1];
            let fb = sb * light[2];
            let fa = sa * light[3];
            if fa <= 0.0 {
                continue;
            }
            let dst = img.get_pixel_mut(x, y);
            // OVER (PMA): dst = src + dst * (1 - src.a)
            let dr = dst[0] as f32 / 255.0;
            let dg = dst[1] as f32 / 255.0;
            let db = dst[2] as f32 / 255.0;
            let da = dst[3] as f32 / 255.0;
            let inv_sa = 1.0 - fa;
            let out_r = (fr + dr * inv_sa).clamp(0.0, 1.0);
            let out_g = (fg + dg * inv_sa).clamp(0.0, 1.0);
            let out_b = (fb + db * inv_sa).clamp(0.0, 1.0);
            let out_a = (fa + da * inv_sa).clamp(0.0, 1.0);
            dst[0] = (out_r * 255.0) as u8;
            dst[1] = (out_g * 255.0) as u8;
            dst[2] = (out_b * 255.0) as u8;
            dst[3] = (out_a * 255.0) as u8;
            any_drawn = true;
        }
    }
    any_drawn
}

fn unpack_argb(v: u32) -> [f32; 4] {
    let a = ((v >> 24) & 0xff) as f32 / 255.0;
    let r = ((v >> 16) & 0xff) as f32 / 255.0;
    let g = ((v >> 8) & 0xff) as f32 / 255.0;
    let b = (v & 0xff) as f32 / 255.0;
    [r, g, b, a]
}
