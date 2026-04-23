# Phase 7 Plan — `dm_spine_bevy` (sub-phases 7a–7d)

**Reader:** the future coding session. You have read `CLAUDE.md` + `NEXT_SESSION.md`. You have not touched Bevy 0.18 in this codebase yet. This plan commits to specific APIs; deviate only if an API call actually rejects the shape below when you try it.

**Crate layout (both `~/deadmoney/dm_spine_bevy/`):**

```
src/
  lib.rs            # SpinePlugin, re-exports
  asset/            # 7a
    mod.rs
    atlas_loader.rs # .atlas loader + SpineAtlasAsset
    skel_loader.rs  # .skel loader + SpineSkeletonAsset
  components.rs     # 7b SpineSkeleton component + helpers
  systems.rs        # 7b scheduling + tick system
  mesh.rs           # 7c RenderCommand -> Mesh conversion
  material/         # 7d
    mod.rs
    spine_material.rs
    spine.wgsl
```

`Cargo.toml` gains `bevy = "0.18.1"`, `dm_spine_runtime = { path = "../dm_spine_runtime" }`, `bytemuck = "1.25"` (for packed color → vec4 conversion if desired), `thiserror = "2"`.

---

## 7a — Asset layer (`.atlas` + `.skel` loaders, page texture resolution)

### Goals

One `asset_server.load::<SpineSkeletonAsset>("rigs/spineboy/spineboy-pro.skel")` call yields a handle whose dependencies (the `.atlas` plus every `.png` page) are auto-loaded by the same asset system. The `TextureId(u32) → Handle<Image>` table is embedded in the atlas asset itself — no separate resource, no manual registration by the app.

### Public types

```rust
// SpineAtlasAsset
#[derive(Asset, TypePath, Debug)]
pub struct SpineAtlasAsset {
    pub atlas: Arc<dm_spine_runtime::atlas::Atlas>,
    // Index-parallel with `atlas.pages`. pages[page_index] is the GPU image.
    pub pages: Vec<Handle<Image>>,
}
```

Rationale for `Arc<Atlas>`: the skeleton loader needs the atlas to resolve attachments, and the same atlas can be referenced by multiple skeletons. `Arc` so the loader can borrow it without cloning the region table. (`Atlas` is a plain owned struct, not already `Arc`-shared; we add the `Arc` wrap at the plugin boundary.)

```rust
// SpineSkeletonAsset
#[derive(Asset, TypePath, Debug)]
pub struct SpineSkeletonAsset {
    // The already-`Arc`-shared core runtime data.
    pub data: Arc<dm_spine_runtime::data::SkeletonData>,
    // The atlas this skeleton was loaded against. Holds the Handle<Image>
    // vec that RenderCommand::TextureId maps into.
    pub atlas: Handle<SpineAtlasAsset>,
}
```

### Loaders

Both implement `bevy_asset::AssetLoader`. Trait shape in 0.18.1:

```rust
pub trait AssetLoader: TypePath + Send + Sync + 'static {
    type Asset: Asset;
    type Settings: Settings + Default + Serialize + for<'a> Deserialize<'a>;
    type Error: Into<BevyError>;
    fn load(&self, reader: &mut dyn Reader, settings: &Self::Settings,
            load_context: &mut LoadContext) -> impl ConditionalSendFuture<...>;
    fn extensions(&self) -> &[&str];
}
```

**`SpineAtlasLoader`** (`extensions() == &["atlas"]`):

1. Read the whole file as UTF-8 via `reader.read_to_end`.
2. Parse: `dm_spine_runtime::atlas::Atlas::parse(&text)`.
3. For each `page` in `atlas.pages`, resolve the PNG path. `AtlasPage::name` is a bare filename (e.g. `"spineboy.png"`). Use `load_context.path()` (the `.atlas` file's `AssetPath`) and strip the last segment, then append `page.name`. Use `load_context.load::<Image>(page_path)` — this returns a `Handle<Image>` and registers the PNG as a dependency so Bevy loads it in parallel.
4. Return `SpineAtlasAsset { atlas: Arc::new(atlas), pages }`.

**`SpineSkeletonLoader`** (`extensions() == &["skel"]`):

1. Read bytes via `reader.read_to_end`.
2. Derive the atlas path from the skeleton path: same stem + `.atlas` extension (`spineboy-pro.skel` → `spineboy.atlas`). This is not what spine-cpp does (spine-cpp hands atlas + skel separately at the API level), but it's the shape every Spine example on disk uses and matches the Unity integration's convention. **Decision: use filename-stem convention by default.** Settings struct can override later.
   - Problem: `spineboy-pro.skel` and `spineboy-ess.skel` both want `spineboy.atlas`. Stem-with-suffix-stripped handling: strip trailing `-pro`, `-ess`, or take everything before the last `-`. Safest is to look for exactly one `.atlas` sibling in the directory. **Flag this as an open question, pick one below.**
3. Call `load_context.loader().direct().load::<SpineAtlasAsset>(atlas_path).await?` to get the atlas synchronously within the load (we need the `Arc<Atlas>` *now* to pass into `AtlasAttachmentLoader`, not later via handle resolution). Use `load_context.load::<SpineAtlasAsset>(atlas_path)` if deferred (dependency-only) is sufficient — actually no: we must construct `SkeletonData` *during* the load because the attachment loader runs there. Use `NestedLoader::direct` to get the actual `SpineAtlasAsset` value, not just a handle.
   - `load_context.loader().direct().load(atlas_path).await` returns a `LoadedAsset<SpineAtlasAsset>`; call `.take()` or equivalent to get the value.
4. With the atlas in hand: `let mut loader = AtlasAttachmentLoader::new(&atlas_asset.atlas);` then `SkeletonBinary::with_loader(&mut loader).read(&bytes)` → `SkeletonData`.
5. Wrap in `Arc` and return `SpineSkeletonAsset { data: Arc::new(skel_data), atlas: <handle from deferred load> }`.

**Subtlety:** step 3's direct-load returns the owned asset by value, but we also want a `Handle<SpineAtlasAsset>` stored on the `SpineSkeletonAsset` so systems can resolve `TextureId → Handle<Image>` later. Use `load_context.load::<SpineAtlasAsset>(atlas_path)` for the handle, *and* `load_context.loader().direct().load(atlas_path).await` for the value. The asset system dedupes — same path, one load.

### Registration (`SpinePlugin::build`)

```rust
app.init_asset::<SpineAtlasAsset>()
   .init_asset::<SpineSkeletonAsset>()
   .register_asset_loader(SpineAtlasLoader)
   .register_asset_loader(SpineSkeletonLoader);
```

### The `TextureId → Handle<Image>` resolution table

- **Location:** `SpineAtlasAsset::pages: Vec<Handle<Image>>`, index-parallel with `atlas.pages`.
- **Populated by:** `SpineAtlasLoader::load` — at asset-load time, once.
- **Consulted by:** the mesh/material system in 7c, at `Update` / `PostUpdate` time, via `Assets<SpineAtlasAsset>::get(handle).unwrap().pages[cmd.texture.0 as usize].clone()`.

Rationale for putting it on the atlas asset instead of a global resource: colocating handles with the atlas makes hot-reload + multi-atlas scenes correct automatically; a global `HashMap<AtlasId, Vec<Handle>>` duplicates state the asset system already tracks. The `TextureId(u32)` sentinel `MISSING = u32::MAX` must be handled — in practice the runtime never emits it for visible attachments, but the material system should skip the command rather than index-oob.

### Data flow (7a)

1. App code: `let skel: Handle<SpineSkeletonAsset> = asset_server.load("rigs/spineboy-pro.skel");`
2. Bevy's asset pipeline invokes `SpineSkeletonLoader::load`.
3. The loader directly loads `spineboy.atlas` → invokes `SpineAtlasLoader::load`.
4. The atlas loader triggers `load_context.load::<Image>("spineboy.png")` for each page. PNGs go through the standard Bevy image loader on a parallel task.
5. Atlas loader finishes with `SpineAtlasAsset { atlas, pages }`. Skeleton loader uses that atlas to build `SkeletonData`, finishes with `SpineSkeletonAsset { data, atlas: Handle }`.
6. When the app later builds a `SpineSkeleton` component (7b), it pulls `SkeletonData` out via `assets.get(skel_handle).data.clone()` — one `Arc::clone`, no deep copy.

### Open questions (7a)

- **Atlas path derivation:** stem-strip `-pro`/`-ess` suffix, *or* scan for the sibling `.atlas`, *or* require an explicit settings field on the loader. Default-stem-strip works for every rig in `spine-runtimes/examples/` except where both `-pro` and `-ess` coexist. **Recommendation: accept a `SpineSkeletonLoaderSettings { atlas_path: Option<String> }` with stem-strip-then-sibling fallback as default.**
- **Binary reader API mismatch:** `reader.read_to_end(&mut Vec)` is async in Bevy 0.18's `Reader` trait (it's `futures_io::AsyncRead`-shaped). Verify whether a straight `.await` works or if we need `futures_lite::AsyncReadExt`. Coding session: if the straight call fails, `use bevy_asset::io::AsyncReadExt;` or equivalent.
- **Processed-assets mode:** the `.skel` binary files are already compact, so `type Settings = ();` with no processing is fine. Skip custom processor for now.

---

## 7b — Component + tick system

### Goals

One entity per skeleton instance. Fields = (runtime `Skeleton`, `AnimationState`, `SkeletonRenderer`, plus handles + a small runtime cache). Systems in `Update` drive animation time and rebuild render commands.

### Public types

```rust
#[derive(Component)]
pub struct SpineSkeleton {
    // Handle to the shared asset. Kept alive by this component.
    pub asset: Handle<SpineSkeletonAsset>,
    // The instance state. `None` until the asset finishes loading
    // (two-stage init, see below).
    pub state: Option<SpineSkeletonState>,
    // User-controllable playback parameters.
    pub time_scale: f32,         // defaults 1.0
    pub physics: Physics,        // defaults Physics::Update
    pub paused: bool,
}

pub struct SpineSkeletonState {
    pub skeleton: dm_spine_runtime::skeleton::Skeleton,
    pub animation_state: dm_spine_runtime::animation::AnimationState,
    pub renderer: dm_spine_runtime::render::SkeletonRenderer,
    // Reusable event buffer, drained each frame.
    pub events: Vec<dm_spine_runtime::animation::Event>,
}
```

**Why two-stage:** the asset is async-loaded. The component can be spawned before `SpineSkeletonAsset` is `Loaded`. A dedicated system watches for the asset becoming available and constructs `SpineSkeletonState` in-place. This is the standard Bevy pattern (`AssetEvent::Added`).

**Bundle ergonomics:** add a `SpineSkeletonBundle` that bundles `SpineSkeleton`, `Transform`, `GlobalTransform`, `Visibility`, `InheritedVisibility`, `ViewVisibility`. The `Transform` places the skeleton in world space; 7c will use the entity transform when building child mesh entities.

### Systems (7b)

All in `Update` schedule, ordered with a `SystemSet`:

- **`initialize_spine_skeletons`** — `in_set(SpineSet::Init)`. Queries `(Entity, &mut SpineSkeleton)` with `state.is_none()`. On each matching entity, checks if the `Handle<SpineSkeletonAsset>` has finished loading (via `Assets<SpineSkeletonAsset>::get(&handle)`). If yes, builds the state:

  ```rust
  let data = Arc::clone(&asset.data);
  let mut sk = Skeleton::new(Arc::clone(&data));
  sk.update_cache();
  sk.set_to_setup_pose();
  sk.update_world_transform(Physics::None);
  let state_data = Arc::new(AnimationStateData::new(Arc::clone(&data)));
  let animation_state = AnimationState::new(state_data);
  ```

  Stores the state. Does *not* start any animation — that's the app's job via a helper method `SpineSkeleton::play(&mut self, track, anim_name, loop)` exposed as component methods once state exists.

- **`tick_spine_skeletons`** — `in_set(SpineSet::Tick)`, `after(SpineSet::Init)`. Queries `&mut SpineSkeleton` with `state.is_some()`. Reads `Res<Time>`.

  ```rust
  let dt = time.delta_secs() * sk.time_scale;
  if sk.paused { continue; }
  let state = sk.state.as_mut().unwrap();
  state.animation_state.update(dt);
  state.events.clear();
  state.animation_state.apply(&mut state.skeleton, &mut state.events);
  state.skeleton.update_world_transform(sk.physics);
  let _cmds = state.renderer.render(&state.skeleton);
  ```

  The `render` call produces `&[RenderCommand]` held by `state.renderer`. 7c's mesh system runs after this, in the same frame, with read-only access to `state`.

- **`drain_spine_events`** — `after(SpineSet::Tick)`. Pulls `state.animation_state.drain_events()` (the state-level events, distinct from per-timeline `Event`s) and emits them as Bevy events (`EventWriter<SpineStateEvent>`). Also optionally bridges per-timeline `Event` via `EventWriter<SpineEvent>`. Exposes both so gameplay code can listen.

### `SpineSet` — public `SystemSet`

```rust
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpineSet { Init, Tick, BuildMeshes }
```

Apps can order their logic `.before(SpineSet::Tick)` to mutate `time_scale` / queue animations on the same frame.

### Data flow (7b)

```
[asset loader] → Assets<SpineSkeletonAsset> 
     ↓ (AssetEvent::Added)
SpineSet::Init     (construct Skeleton + AnimationState, attach state)
     ↓
SpineSet::Tick     (advance time, apply, update_world_transform, render → &[RenderCommand])
     ↓
SpineSet::BuildMeshes  (7c)
```

### Open questions (7b)

- **Animation triggering API:** `SpineSkeleton::play_animation(...)` as a method that requires `&mut self` plus an `AnimationId` lookup by name — the state-machine wrapper. Alternatively expose the raw `&mut AnimationState` via a method. **Recommendation: expose both — a thin `play(&mut self, track, anim_name, looping)` for the 90% case, plus `animation_state_mut(&mut self) -> Option<&mut AnimationState>` for escape.**
- **Asset-reload behavior:** if the `.skel` reloads, do we rebuild state? Simplest: watch `AssetEvent::Modified` and reset `state = None`. Reasonable default, can be gated behind a feature later. **Decision: do it, it's cheap.**
- **Parallelism:** `tick_spine_skeletons` is currently a sequential query. With `par_iter_mut` it's parallel per-skeleton. `Skeleton`/`AnimationState` contain no shared mutable state except the shared `Arc<SkeletonData>` which is read-only. Safe to parallelize. Don't bother until multiple skeletons actually exist in a real scene.

---

## 7c — Mesh building

### Goals

Every frame, convert `&[RenderCommand]` into one Bevy `Mesh` + one `MeshMaterial2d<SpineMaterial>` per command. Each command becomes a child entity of the skeleton entity, carrying the world transform up automatically.

### Strategy — child entity per command, meshes recycled

Strawman A (one mesh per command per frame, recreated): clean but allocates on every command every frame. Rejected — skeletons emit ~5–20 commands, at 60Hz that's 1200 mesh allocations per second per rig.

Strawman B (one mesh asset per command index, mutated in place): store a `Vec<Handle<Mesh>>` on the skeleton component, reuse slot-0 for command-0, etc. Mutate via `Assets<Mesh>::get_mut`. Pay one allocation only when command-count grows.

**Pick B.** Materials similarly kept in a `Vec<Handle<SpineMaterial>>` on the skeleton.

### Public types

Extension to `SpineSkeletonState`:

```rust
pub struct SpineSkeletonState {
    // ... (7b fields)
    pub meshes: Vec<Handle<Mesh>>,
    pub materials: Vec<Handle<SpineMaterial>>,
    pub children: Vec<Entity>,  // one per slot; parallel to meshes/materials
}
```

### Mesh layout

Per command:

- `Mesh::ATTRIBUTE_POSITION`: `Vec<[f32; 3]>` with `z = 0.0`, length = `num_vertices`. Converted from the interleaved `positions: Vec<f32>` (pairs → triples, z=0).
- `Mesh::ATTRIBUTE_UV_0`: `Vec<[f32; 2]>`, length = `num_vertices`. Converted from `cmd.uvs`.
- `Mesh::ATTRIBUTE_COLOR`: `Vec<[f32; 4]>`, length = `num_vertices`. Unpacked from `cmd.colors[i]` (packed `0xAARRGGBB`). These are already premultiplied — the shader does **not** multiply again.
- Custom attribute for dark color: `const ATTRIBUTE_DARK_COLOR: MeshVertexAttribute = MeshVertexAttribute::new("Vertex_DarkColor", 60001, VertexFormat::Float32x4);` — unpacked from `cmd.dark_colors[i]`.
- `Mesh::insert_indices(Indices::U16(cmd.indices.clone()))`.

Primitive topology: `PrimitiveTopology::TriangleList`. Asset usage: `RenderAssetUsages::RENDER_WORLD` (not needed on CPU after upload).

### System — `build_spine_meshes`

`in_set(SpineSet::BuildMeshes)`, `after(SpineSet::Tick)`. Queries `&SpineSkeleton`. Needs `ResMut<Assets<Mesh>>`, `ResMut<Assets<SpineMaterial>>`, `Res<Assets<SpineAtlasAsset>>`, `Res<Assets<SpineSkeletonAsset>>`, and `Commands` for spawning child entities the first time.

Per-frame algorithm per skeleton entity:

1. Get `cmds = state.renderer.render(&state.skeleton)` — already computed in 7b's tick. (Either expose `SkeletonRenderer::last_commands(&self) -> &[RenderCommand]` in the runtime crate — there's no such accessor today; `render` returns the slice directly but we've dropped that borrow by now. **Decision: have 7b store the commands as a `Vec<RenderCommand>` clone in `state`, OR re-call `render` in 7c. Cloning ~20 commands per frame is cheap; re-calling `render` is also cheap but duplicates work. Clone is simpler. Or better: expose a getter.** See open question below — confirmed fix is a one-line `commands(&self) -> &[RenderCommand]` accessor on `SkeletonRenderer`.)
2. Resolve atlas: `let atlas = atlases.get(&skel_asset.atlas).unwrap();`
3. Grow `state.meshes` / `state.materials` / `state.children` to `cmds.len()`, spawning new child entities as needed with `commands.spawn((Mesh2d(...), MeshMaterial2d(...), Transform::default())).set_parent(entity)`.
4. For each command `i`:
   - `let tex = atlas.pages.get(cmd.texture.0 as usize).cloned().unwrap_or(Handle::default());`
   - `let mesh = meshes.get_mut(&state.meshes[i]).unwrap();` — clear + re-insert attributes.
   - `let mat = materials.get_mut(&state.materials[i]).unwrap();` — update `mat.texture = tex; mat.blend_mode = cmd.blend_mode;`.
   - Toggle child entity visibility if any leftover slots beyond `cmds.len()`.
5. Hide trailing children (set `Visibility::Hidden`) if `children.len() > cmds.len()`.

### The `TextureId → Handle<Image>` resolution table — usage in 7c

This is the *consumption point* of the table set up in 7a. `cmd.texture.0 as usize` indexes into `SpineAtlasAsset::pages`. That's the only consultation site in the entire plugin. Everything else — materials, mesh, color — doesn't know textures exist.

### Data flow (7c)

```
tick system: state.renderer.render() → stored in state
     ↓
build_spine_meshes:
  for each cmd[i]:
    mesh[i].insert_attrs(cmd.positions, cmd.uvs, cmd.colors, cmd.dark_colors)
    mesh[i].insert_indices(cmd.indices)
    material[i].texture = atlas.pages[cmd.texture.0]
    material[i].blend_mode = cmd.blend_mode
```

### Open questions (7c)

- **Who holds the commands between 7b's tick and 7c's build:** store them as `Vec<RenderCommand>` on `state` (clone the slice in the tick system), or expose a `fn commands(&self) -> &[RenderCommand]` on `SkeletonRenderer` in the runtime crate (small non-breaking addition). **Recommendation: add `SkeletonRenderer::commands(&self) -> &[RenderCommand]` in the runtime crate — trivial accessor, avoids the clone. One-line runtime change.** (Confirmed: no such accessor exists today. `render()` currently returns `&[RenderCommand]` directly.)
- **Z-ordering / draw order:** all child entities share `Transform::default()` parented to the skeleton entity, so they render at the same Z. Bevy's `Transparent2d` phase sorts by Z — every command at Z=0 ties. The runtime's draw-order walker already emits commands in correct back-to-front order; we need Bevy to *preserve* that order. **Solution: set each child's `Transform::from_xyz(0.0, 0.0, i as f32 * 0.001)` — a small per-command Z offset. Or use `depth_bias` on the material (set via `Material2d::depth_bias`). Z offset is simpler, no material specialization key bloat.** Flag: confirm the sort-key direction on `Transparent2d` — back-to-front means "deeper Z = drawn first"; if so we want `i` positive (nearer = drawn later). If wrong, flip the sign.
- **Mesh removal on unload:** if the component is despawned, Bevy's parent/child relation cleans up children. The `Handle<Mesh>` / `Handle<SpineMaterial>` refcounts drop, Bevy drops the assets. Fine.
- **`Vec<[f32; 4]>` color unpacking cost:** ~60 verts × 4 floats × 2 color attrs × 60fps × 20 skeletons = ~576K ops/sec, trivial. Don't bother with bytemuck cast tricks initially.

---

## 7d — PMA-aware custom material

### Goals

Correct premultiplied-alpha blending, with per-command blend mode (Normal / Additive / Multiply / Screen), plus tint-black (dark-color) shader path. `Material2d` + `Material2dPlugin<SpineMaterial>`; blend state overridden via `Material2d::specialize`.

### Public type

```rust
#[derive(Asset, AsBindGroup, TypePath, Clone)]
#[bind_group_data(SpineMaterialKey)]
pub struct SpineMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
    pub blend_mode: SpineBlendMode,  // becomes the bind-group-data key
}

#[repr(u8)]
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub enum SpineBlendMode { Normal, Additive, Multiply, Screen }

#[repr(C)]
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct SpineMaterialKey {
    blend_mode: SpineBlendMode,
}

impl From<&SpineMaterial> for SpineMaterialKey { /* copy blend_mode */ }
```

Note: `SpineBlendMode` is the Bevy-side enum mirroring `dm_spine_runtime::data::BlendMode`. Trivial `From` impl both ways. Done in plugin crate so runtime crate doesn't gain a Bevy dep.

### Material2d impl

```rust
impl Material2d for SpineMaterial {
    fn fragment_shader() -> ShaderRef { "embedded://dm_spine_bevy/spine.wgsl".into() }
    fn vertex_shader() -> ShaderRef { "embedded://dm_spine_bevy/spine.wgsl".into() }
    // We're always blending (PMA alpha). Return Blend so the material
    // routes to Transparent2d phase — we override the actual BlendState
    // in specialize() below.
    fn alpha_mode(&self) -> AlphaMode2d { AlphaMode2d::Blend }
    fn specialize(descriptor: &mut RenderPipelineDescriptor,
                  _layout: &MeshVertexBufferLayoutRef,
                  key: Material2dKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(fragment) = descriptor.fragment.as_mut() {
            if let Some(Some(target)) = fragment.targets.get_mut(0) {
                target.blend = Some(blend_state_for(key.bind_group_data.blend_mode));
            }
        }
        Ok(())
    }
}
```

### Blend state table (`blend_state_for`)

From spine-cpp `SkeletonRenderer.cpp` and the Spine PMA blending table:

| `BlendMode` | Color `src_factor, dst_factor, op` | Alpha |
|---|---|---|
| Normal   | `One, OneMinusSrcAlpha, Add`     | `One, OneMinusSrcAlpha, Add` |
| Additive | `One, One, Add`                   | `One, One, Add` |
| Multiply | `DstColor, OneMinusSrcAlpha, Add` | `OneMinusSrcAlpha, OneMinusSrcAlpha, Add` |
| Screen   | `One, OneMinusSrcColor, Add`     | `OneMinusSrcColor, OneMinusSrcColor, Add` |

These are the **PMA variants** (atlases are premultiplied; colors in `cmd.colors` are already premultiplied). All are `BlendOperation::Add`.

`BlendState::PREMULTIPLIED_ALPHA_BLENDING` from wgpu (both components = `BlendComponent::OVER`, which is `One, OneMinusSrcAlpha, Add`) is the same as our Normal case — use it for Normal to stay aligned with wgpu's canonical constant.

### Shader (`spine.wgsl`)

Inputs from Bevy's Mesh2d vertex pipeline: `@location(0) position: vec3<f32>`, `@location(1) normal: vec3<f32>` (unused but required by default layout — or override layout in specialize), `@location(2) uv: vec2<f32>`, `@location(5) color: vec4<f32>` (the light color, premultiplied), plus the custom dark color at the attribute ID we picked in 7c.

Fragment:

```wgsl
let sample = textureSample(spine_texture, spine_sampler, in.uv);
// Spine tint-black formula: ((sample.rgb - 1.0) * dark.rgb + sample.rgb) * light.rgb
// plus the light alpha multiplied in (since everything is PMA).
let rgb = ((sample.rgb - vec3(1.0)) * in.dark_color.rgb + sample.rgb) * in.light_color.rgb;
let a   = sample.a * in.light_color.a;
return vec4(rgb, a);
```

The exact tint-black formula is from `spine-cpp` / spine-ts shaders. Confirm against `~/deadmoney/spine-runtimes/spine-ts/spine-webgl/src/shaders/` when you're writing it — don't trust this plan's math.

Note re: **normals attribute** — the default Mesh2d vertex layout in Bevy 0.18 is position-only by default but adds UV/color when those attributes exist on the mesh. Normals are not required. Skip `ATTRIBUTE_NORMAL`.

### Registration in `SpinePlugin::build`

```rust
app.add_plugins(Material2dPlugin::<SpineMaterial>::default());
// Embed the shader
embedded_asset!(app, "material/spine.wgsl");
```

### Data flow (7d)

```
7c writes cmd.blend_mode → material.blend_mode
     ↓
Material2dPlugin's prepare_asset path:
  material.bind_group_data() → SpineMaterialKey { blend_mode }
     ↓
specialize_material2d_meshes: calls Material2d::specialize with key
     ↓
  specialize() mutates descriptor.fragment.targets[0].blend
     ↓
wgpu builds a specialized pipeline, cached by (key, mesh key) tuple
```

Four blend modes → four cached pipelines after first frame, hits warm cache thereafter.

### Open questions (7d)

- **Alpha-mode routing:** `AlphaMode2d::Blend` forces `Transparent2d` phase, which sorts by Z. That's what we want. Don't use `Opaque` — spine sprites are rarely opaque and sorting matters.
- **Multiply/Screen numerical correctness against spine-cpp references:** the Multiply equation in particular depends on whether the atlas page alpha is pre-multiplied into the destination. Verify by eye against a rig that uses Multiply slots (raptor? celestial-circus?) and iterate if wrong. No golden-parity test is feasible at the visual layer.
- **Shader attribute IDs:** the dark-color attribute ID `60001` is arbitrary; just needs to not collide with Bevy's reserved IDs (0–6 for stock attributes). Confirm by grep against `bevy_mesh` constants. If 60001 conflicts, pick another number in the >1000 range.
- **Bindless / multi-texture batching:** not in scope for 7d. Each skeleton has only 1–2 atlas pages in practice; one draw call per command is fine. If/when the core batcher is extended to merge across pages via a bindless texture array, revisit.

---

## Dependencies & sequencing

- **7a blocks everything** — no skeleton asset, nothing to do.
- **7b depends on 7a** — needs `SpineSkeletonAsset` to initialize state.
- **7c depends on 7a + 7b** — needs the atlas page handles and the per-frame render commands.
- **7d depends on 7c's material construction, but its shader + `Material2d` impl can be drafted in parallel with 7c.** Recommend: write 7d's types and shader stub (with Normal-only blend) concurrently with 7c, then add the `specialize` branch table as the last step.

**Commit boundaries:** one commit per sub-phase, matching Phase 0–6 cadence.

## Critical files for implementation

- `/home/brandon/deadmoney/dm_spine_bevy/src/lib.rs`
- `/home/brandon/deadmoney/dm_spine_bevy/src/asset/skel_loader.rs`
- `/home/brandon/deadmoney/dm_spine_bevy/src/asset/atlas_loader.rs`
- `/home/brandon/deadmoney/dm_spine_bevy/src/systems.rs`
- `/home/brandon/deadmoney/dm_spine_bevy/src/material/spine_material.rs`

**Runtime-crate reference files (read-only during Phase 7):**

- `/home/brandon/deadmoney/dm_spine_runtime/src/render/renderer.rs` (line 87 `render` signature; consider adding `pub fn commands(&self) -> &[RenderCommand]`)
- `/home/brandon/deadmoney/dm_spine_runtime/src/render/mod.rs` (`RenderCommand`, `TextureId`)
- `/home/brandon/deadmoney/dm_spine_runtime/src/skeleton/skeleton.rs` (Skeleton pub API)
- `/home/brandon/deadmoney/dm_spine_runtime/src/animation/state.rs` (AnimationState pub API)
- `/home/brandon/deadmoney/dm_spine_runtime/src/atlas.rs` (Atlas parse + page data)
- `/home/brandon/deadmoney/dm_spine_runtime/src/data/slot.rs` (BlendMode enum)
- `/home/brandon/deadmoney/dm_spine_runtime/tests/render_smoke.rs` (end-to-end flow reference)
