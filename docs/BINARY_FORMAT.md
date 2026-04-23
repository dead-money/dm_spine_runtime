# Spine 4.2 Binary `.skel` Format Reference

This document describes the wire format of the Spine editor's binary skeleton
export (`.skel`) for version 4.2.x, in enough detail to implement a loader
from scratch. It was produced while porting `spine-cpp/SkeletonBinary.cpp` to
Rust and captures every structural decision, subtle encoding trick, and gotcha
surfaced along the way.

The format is little-documented outside the code itself. The canonical
reference implementations are:

- `spine-cpp/spine-cpp/src/spine/SkeletonBinary.cpp` (the C++ port the Spine
  team maintains — authoritative when in doubt).
- `spine-runtimes/spine-ts/spine-core/src/SkeletonBinary.ts` (TypeScript port;
  often easier to read.)
- `dm_spine_runtime/src/load/binary/parse.rs` (Rust port, this project).

Wherever this document cites line numbers, they refer to the `spine-cpp`
sources as of the 4.2 branch.

## Conventions

Throughout: **big-endian** byte order; counts / indices are encoded as
variable-length unsigned integers ("unsigned varint"); signed values that
benefit from compact encoding use a zigzag-encoded varint.

Fixed-size primitives:

| Type         | Size  | Notes                                                      |
| ------------ | ----- | ---------------------------------------------------------- |
| `byte`       | 1 B   | `u8`.                                                      |
| `sbyte`      | 1 B   | `i8`. Used for curve-type discriminants on timelines.      |
| `bool`       | 1 B   | Stored as a byte; non-zero means true.                     |
| `int`        | 4 B   | Big-endian `i32`.                                          |
| `float`      | 4 B   | Big-endian IEEE-754 single precision (bit-reinterpreted `int`). |
| `rgba`       | 4 B   | Four bytes R, G, B, A. Each is `u8 / 255.0` as a float.    |

## Varints

A varint is 1–5 bytes. Each byte contributes its low 7 bits to the value, with
the MSB (`0x80`) acting as a continuation flag. The 5th byte's high bit is a
hard cap — anything beyond is a format error. (`spine-cpp` silently truncates;
more strict parsers should error.)

**Unsigned varint** (also called "optimize positive" in the C++ code): bytes
accumulate in little-endian septets directly.

    byte 0: C b b b b b b b   ─► value[0..7]
    byte 1: C b b b b b b b   ─► value[7..14]
    byte 2: C b b b b b b b   ─► value[14..21]
    byte 3: C b b b b b b b   ─► value[21..28]
    byte 4: _ b b b b b b b   ─► value[28..35] (low 4 bits only used)

**Signed varint** (zigzag, when `optimize_positive = false`): read the value
as if unsigned, then zigzag-decode:

    signed = (unsigned >> 1) ^ -(unsigned & 1)

Used for fields that are typically small but may be negative (`EventData::int_value`).

### "Unsigned-as-signed" trick (DrawOrder shifts)

One field — the DrawOrder timeline's per-slot `shift` — is written as an
unsigned varint but interpreted semantically as signed via integer
wraparound. `spine-cpp` reads the varint as `int`, casts to `size_t`, and
adds it to an index; on two's-complement hardware `(size_t)(-2) + 9 == 7` at
any pointer width, which produces the correct target index. **Porters on
languages without unsigned wrap-based arithmetic (e.g. Rust's `usize` on
64-bit) must read the shift as signed `i32` and do the addition in signed
arithmetic before casting back.** See the Gotchas section.

## Strings

Every string is length-prefixed by an unsigned varint `n`:

- `n == 0` → the string is `None` / null. No payload follows.
- `n > 0` → `n - 1` bytes of UTF-8 follow (no trailing NUL on the wire).

Many strings appear twice in a skeleton (attachment names, slot names, bone
names, etc.) and are deduplicated through a **string table** written early in
the file. A "string-ref" is a one-byte-plus unsigned varint that either
indexes the table or encodes `None`:

- `0` → `None`.
- `n > 0` → `table[n - 1]`.

String-refs cannot resolve to the empty string; raw length-prefixed strings
must be used for payloads that may be empty.

## Colors

Four bytes R, G, B, A, each divided by 255 to produce an `f32` in `[0, 1]`.

The `SlotData` "dark color" has a special 4-byte layout where all-`0xFF`
means "no dark color." See the Slots section.

## Scale

The loader accepts an optional world-space `scale` factor (default `1.0`)
applied to every position / length at load time. Any field marked
**"scaled"** below is multiplied by this factor during load.

# Top-level structure

Read in strict order:

1. [Header](#header) — 2 hashes, version, dimensions, reference scale, flags.
2. [Optional non-essential header fields](#header) — fps, images path, audio path.
3. [String table](#string-table).
4. [Bones](#bones).
5. [Slots](#slots).
6. [IK constraints](#ik-constraints).
7. [Transform constraints](#transform-constraints).
8. [Path constraints](#path-constraints).
9. [Physics constraints](#physics-constraints).
10. [Default skin](#default-skin) (only written if non-empty).
11. [Named skins](#named-skins).
12. [Linked mesh resolution](#linked-mesh-resolution) — not a stream section,
    but must happen before animations are read.
13. [Events](#events).
14. [Animations](#animations).

Parsing should consume the entire file; leftover bytes indicate a drift.

# Header

| Order | Field                | Encoding                      | Notes                                                              |
| ----- | -------------------- | ----------------------------- | ------------------------------------------------------------------ |
| 1     | `hashLow`            | `int`                         | Low 32 bits of skeleton hash.                                      |
| 2     | `hashHigh`           | `int`                         | High 32 bits of skeleton hash.                                     |
| 3     | `version`            | string                        | Editor version string, e.g. `"4.2.41"`. Must start with `"4.2"`.   |
| 4     | `x`                  | `float`                       | Skeleton AABB origin X.                                            |
| 5     | `y`                  | `float`                       | Skeleton AABB origin Y.                                            |
| 6     | `width`              | `float`                       | Skeleton AABB width.                                               |
| 7     | `height`             | `float`                       | Skeleton AABB height.                                              |
| 8     | `referenceScale`     | `float` (scaled)              | Editor-authored unit hint; used by physics.                        |
| 9     | `nonessential`       | `bool`                        | If `true`, extra fields present throughout the file.               |

The hash is presented to users as `format!("{hashHigh:x}{hashLow:x}")` —
i.e. the *high* word printed first.

If `nonessential == true`, three more header fields follow:

| Order | Field         | Encoding | Notes                                     |
| ----- | ------------- | -------- | ----------------------------------------- |
| 10    | `fps`         | `float`  | Spine editor dopesheet frames per second. |
| 11    | `imagesPath`  | string   | Editor hint for texture folder.           |
| 12    | `audioPath`   | string   | Editor hint for audio folder.             |

Runtime code should tolerate a file whose version begins with `"4.2"` but has
a different patch version. A mismatching major/minor should be a hard error.

# String table

An unsigned varint count followed by that many strings:

```
numStrings : uvarint
strings[numStrings]  : string (length-prefixed, see above)
```

After this block, `string-ref` references throughout the file are resolved
against this table.

# Bones

```
numBones : uvarint
for i in 0..numBones:
    name       : string
    parentIdx  : uvarint  ; only present when i > 0 (index into bones so far)
    rotation   : float
    x          : float (scaled)
    y          : float (scaled)
    scaleX     : float
    scaleY     : float
    shearX     : float
    shearY     : float
    length     : float (scaled)
    inherit    : uvarint enum (see Inherit below)
    skinRequired : bool
    if nonessential:
        color   : rgba
        icon    : string
        visible : bool
```

Bones are stored in parent-first order; `parentIdx` for bone `i` is always
an index `< i`. The root bone (index 0) has no parent field on the wire.

## `Inherit` enum

| Value | Name                      | Notes                                           |
| ----- | ------------------------- | ----------------------------------------------- |
| 0     | `Normal`                  | Default: inherit TRS + shear.                   |
| 1     | `OnlyTranslation`         | Inherit only translation.                       |
| 2     | `NoRotationOrReflection`  | Inherit TS; drop parent rotation / reflection.  |
| 3     | `NoScale`                 | Inherit TR; drop parent scale.                  |
| 4     | `NoScaleOrReflection`     | Inherit TR; drop parent scale and reflection.   |

**Encoding note:** In the BoneData block above, `inherit` is an **unsigned
varint**. In the `InheritTimeline` (under Bone timelines), the same enum is
encoded as a **single byte**. See Gotchas.

# Slots

```
numSlots : uvarint
for i in 0..numSlots:
    name           : string
    boneIdx        : uvarint (into bones[])
    color          : rgba
    darkColor      : 4 bytes, see below
    attachmentName : string-ref
    blendMode      : uvarint enum (see below)
    if nonessential:
        visible    : bool
```

Slots are written in setup-pose draw order.

## Dark color encoding

Four raw bytes in the order `a, r, g, b` (note: alpha first). If all four
bytes equal `0xFF`, the slot has *no* dark color. Otherwise the slot has a
dark color of `(r / 255, g / 255, b / 255, 1.0)`. The 4th byte is always
treated as if it were `1.0` alpha — the alpha channel here is a sentinel,
not a color value.

## `BlendMode` enum

| Value | Name       |
| ----- | ---------- |
| 0     | `Normal`   |
| 1     | `Additive` |
| 2     | `Multiply` |
| 3     | `Screen`   |

# IK constraints

```
numIk : uvarint
for i in 0..numIk:
    name        : string
    order       : uvarint  ; update-cache order key
    numBones    : uvarint
    bones[numBones] : uvarint each, into bones[]
    target      : uvarint, into bones[]
    flags       : byte (bit layout below)
    if flags & 32:
        mix     : float  (or 1.0 when flags & 64 == 0 — see below)
    if flags & 128:
        softness : float (scaled)
```

**Flag bits:**

| Bit   | Meaning                                                                          |
| ----- | -------------------------------------------------------------------------------- |
| `1`   | `skinRequired`.                                                                  |
| `2`   | `bendDirection`: 1 if set, else -1.                                              |
| `4`   | `compress`.                                                                      |
| `8`   | `stretch`.                                                                       |
| `16`  | `uniform`.                                                                       |
| `32`  | `mix` is present. If unset, `mix = 0`.                                           |
| `64`  | Only meaningful when bit 32 is set: if set, read `mix` as a float; else `mix = 1`. |
| `128` | `softness` is present. If unset, `softness = 0`.                                 |

The `bendDirection` is always ±1 — the bit decides which.

# Transform constraints

```
numTc : uvarint
for i in 0..numTc:
    name        : string
    order       : uvarint
    numBones    : uvarint
    bones[numBones] : uvarint each, into bones[]
    target      : uvarint, into bones[]
    flagsA      : byte
    ; conditionally read offset*:
    if flagsA & 8:   offsetRotation : float
    if flagsA & 16:  offsetX        : float (scaled)
    if flagsA & 32:  offsetY        : float (scaled)
    if flagsA & 64:  offsetScaleX   : float
    if flagsA & 128: offsetScaleY   : float

    flagsB      : byte
    ; conditionally read remaining mix* / offset*:
    if flagsB & 1:  offsetShearY : float
    if flagsB & 2:  mixRotate    : float
    if flagsB & 4:  mixX         : float
    if flagsB & 8:  mixY         : float
    if flagsB & 16: mixScaleX    : float
    if flagsB & 32: mixScaleY    : float
    if flagsB & 64: mixShearY    : float
```

**`flagsA` bits:**

| Bit | Meaning                                      |
| --- | -------------------------------------------- |
| 1   | `skinRequired`.                              |
| 2   | `local`.                                     |
| 4   | `relative`.                                  |
| 8   | `offsetRotation` present.                    |
| 16  | `offsetX` present (scaled).                  |
| 32  | `offsetY` present (scaled).                  |
| 64  | `offsetScaleX` present.                      |
| 128 | `offsetScaleY` present.                      |

**`flagsB` bits:** as annotated above.

Fields that are not flagged retain their setup-pose defaults (1.0 for every
`mix*` and 0.0 for every `offset*`; `offsetScaleX/Y` default to 0.0 despite
`scaleX/Y` being multiplicative, because a value of "0" in the offset is
the additive identity for Spine's scale math — the factor is `1 + offset`).

# Path constraints

```
numPc : uvarint
for i in 0..numPc:
    name          : string
    order         : uvarint
    skinRequired  : bool
    numBones      : uvarint
    bones[numBones] : uvarint each, into bones[]
    target        : uvarint, into slots[]
    flags         : byte
        positionMode = flags & 1
        spacingMode  = (flags >> 1) & 3
        rotateMode   = (flags >> 3) & 3
    if flags & 128: offsetRotation : float
    position      : float  (scaled if positionMode == Fixed)
    spacing       : float  (scaled if spacingMode in {Length, Fixed})
    mixRotate     : float
    mixX          : float
    mixY          : float
```

## Mode enums

| Bits       | `PositionMode` | `SpacingMode`   | `RotateMode`   |
| ---------- | -------------- | --------------- | -------------- |
| 0          | `Fixed`        | `Length`        | `Tangent`      |
| 1          | `Percent`      | `Fixed`         | `Chain`        |
| 2          | —              | `Percent`       | `ChainScale`   |
| 3          | —              | `Proportional`  | —              |

`PositionMode` is stored as a 1-bit value; the remaining modes share the
upper bits of the same flags byte.

# Physics constraints

```
numPhys : uvarint
for i in 0..numPhys:
    name         : string
    order        : uvarint
    bone         : uvarint, into bones[]
    flagsA       : byte
    if flagsA & 2:  x       : float  (mix fraction for x-axis)
    if flagsA & 4:  y       : float
    if flagsA & 8:  rotate  : float
    if flagsA & 16: scaleX  : float
    if flagsA & 32: shearX  : float
    limit       : float (scaled)   ; if flagsA & 64, read; else default 5000
    step        : byte              ; stored as 1 / step_raw  (e.g. 60 → 1/60)
    inertia     : float
    strength    : float
    damping     : float
    massInverse : float             ; if flagsA & 128, read; else default 1.0
    wind        : float
    gravity     : float
    flagsB      : byte
        inertiaGlobal  = flagsB & 1
        strengthGlobal = flagsB & 2
        dampingGlobal  = flagsB & 4
        massGlobal     = flagsB & 8
        windGlobal     = flagsB & 16
        gravityGlobal  = flagsB & 32
        mixGlobal      = flagsB & 64
    mix         : float             ; if flagsB & 128, read; else default 1.0
```

**`flagsA` bit 1** = `skinRequired`.

The `*Global` booleans indicate that the corresponding dynamic parameter
draws its value from a skeleton-wide setting at runtime rather than from
this constraint's own field.

**Physics timelines** (later in animation blocks) reference constraints by
a 1-indexed reference: value `0` means "all physics constraints in the
skeleton" and `n > 0` means `physics_constraints[n - 1]`. See Gotchas.

# Default skin

The default skin is written even if empty; its slot count serves as an
"is there a default skin?" gate.

```
slotCount : uvarint
if slotCount == 0:
    ; no default skin; skip the rest of this block
else:
    read attachment-block for each of slotCount slots (see below)
```

# Named skins

```
numSkins : uvarint
for i in 0..numSkins:
    name             : string
    if nonessential:
        color        : rgba        ; skin color — informational only
    numSkinBones     : uvarint
    skinBones[]      : uvarint each, into bones[]
    numIk            : uvarint
    skinIk[]         : uvarint each, into ikConstraints[]
    numTc            : uvarint
    skinTc[]         : uvarint each, into transformConstraints[]
    numPc            : uvarint
    skinPc[]         : uvarint each, into pathConstraints[]
    numPhys          : uvarint
    skinPhys[]       : uvarint each, into physicsConstraints[]
    slotCount        : uvarint
    read attachment-block for each of slotCount slots (see below)
```

Each skin carries its own membership lists for skin-required bones and
constraints. A skin-required element is only active when at least one
applied skin lists it.

## Attachment block (shared between default and named skins)

```
for each of slotCount slots:
    slotIdx   : uvarint, into slots[]
    numAttachments : uvarint
    for j in 0..numAttachments:
        placeholderName : string-ref    ; key under which this attachment is stored
        attachment      : attachment-record (see below)
```

The `placeholderName` is the name used to look the attachment up at render
time — it may differ from the attachment's own `name` (which may be a
texture path on the atlas). For example, a skin called `goggles` might map
placeholder `"head-accessory"` to an attachment named `"goggles/gold-goggles"`.

## Attachment record

Every attachment begins with a byte of flags:

```
flags : byte
    type = flags & 0x7         ; AttachmentType enum
    nameOverride = flags & 8   ; if set, next field overrides the placeholder name
name  : string-ref  (only if nameOverride; else falls back to the placeholder)
```

The meaning of the remaining `flags` bits depends on `type`.

### `AttachmentType` enum

| Value | Name           | Notes                                                    |
| ----- | -------------- | -------------------------------------------------------- |
| 0     | `Region`       | Single textured quad.                                    |
| 1     | `BoundingBox`  | Polygon used for hit detection.                          |
| 2     | `Mesh`         | Arbitrary triangle mesh with own vertex data.            |
| 3     | `LinkedMesh`   | Mesh that inherits vertex data from another mesh.        |
| 4     | `Path`         | Cubic bezier path (for path-constrained bones).          |
| 5     | `Point`        | Oriented point (hitspawn location).                      |
| 6     | `Clipping`     | Polygonal mask. Applies until `endSlot` in draw order.   |

### `Region` attachment

```
path      : string-ref        ; only if flags & 16; else path = name
color     : rgba              ; only if flags & 32; else (1,1,1,1)
sequence  : Sequence record   ; only if flags & 64; see below
rotation  : float             ; only if flags & 128; else 0
x         : float (scaled)
y         : float (scaled)
scaleX    : float
scaleY    : float
width     : float (scaled)
height    : float (scaled)
```

When `sequence` is present, the loader should populate its `regions[]`
array via atlas lookups rather than looking up `path` as a single region.
See the Sequence attachment section below and Gotchas.

### `BoundingBox` attachment

```
vertices  : Vertices record (see below)
                 ; flags & 16 → weighted, else unweighted
if nonessential:
    color : rgba
```

### `Mesh` attachment

```
path       : string-ref  ; only if flags & 16; else path = name
color      : rgba        ; only if flags & 32
sequence   : Sequence    ; only if flags & 64
hullLength : uvarint     ; hull vertex count (raw, in vertex units)
vertices   : Vertices record  ; flags & 128 → weighted
uvs        : float[verticesLength]     ; one f32 per vertex axis; not scaled
triangles  : ushort[(verticesLength - hullLength - 2) * 3]
                              ; each element is an unsigned varint
                              ; index into this mesh's vertex list
if nonessential:
    edgesCount : uvarint
    edges      : ushort[edgesCount]     ; varints, pairs of vertex indices
    width      : float       ; display-only; not scaled in core
    height     : float
```

`verticesLength` in the triangle-count formula is the "doubled vertex count"
returned by the Vertices record (see below) — i.e. `vertexCount * 2`. The
hullLength is the *undoubled* vertex count of the mesh's hull. Both units
mixing in one formula looks wrong but produces the correct triangle count.
See Gotchas.

### `LinkedMesh` attachment

```
path            : string-ref       ; only if flags & 16; else path = name
color           : rgba             ; only if flags & 32
sequence        : Sequence         ; only if flags & 64
inheritTimeline : bool             ; flags & 128
parentSkinIdx   : uvarint          ; index into skeleton's skin list
parentName      : string-ref       ; placeholder name in parent skin
if nonessential:
    width       : float (scaled)
    height      : float (scaled)
```

LinkedMesh attachments have no vertex data on the wire — they're resolved
into full mesh attachments by a second pass after all skins have loaded.
See [Linked mesh resolution](#linked-mesh-resolution).

### `Path` attachment

```
closed        : bool             ; flags & 16
constantSpeed : bool             ; flags & 32
vertices      : Vertices record  ; flags & 64 → weighted
lengths       : float[verticesLength / 6]     ; per-cubic-segment arc length
                                              ; each value is scaled
if nonessential:
    color     : rgba
```

The path has `verticesLength / 6` cubic segments because each segment uses
3 control points (= 6 floats). When `closed`, the last segment wraps to
the first control point.

### `Point` attachment

```
rotation : float
x        : float (scaled)
y        : float (scaled)
if nonessential:
    color : rgba
```

### `Clipping` attachment

```
endSlotIdx : uvarint  ; index into slots[]; clipping is active until this
                      ; slot is rendered
vertices   : Vertices record  ; flags & 16 → weighted
if nonessential:
    color  : rgba
```

### Vertices record (shared)

Used by mesh, bounding box, path, and clipping attachments.

```
vertexCount : uvarint
verticesLength = vertexCount * 2   ; semantic: 2D vertex count × 2 axes

if unweighted (flag bit clear):
    vertices : float[verticesLength]     ; interleaved xy, scaled
else (weighted):
    for each of vertexCount vertices:
        boneCount : uvarint               ; emitted into `bones`
        for each of boneCount bones:
            boneIdx : uvarint             ; emitted into `bones`
            bx      : float (scaled)       ; vertex pos in bone local space
            by      : float (scaled)
            weight  : float                ; not scaled
```

When weighted, `bones[]` is a flattened run-length stream: for each vertex
one count followed by that many bone indices. `vertices[]` is correspondingly
a flattened stream of `(bx, by, weight)` triples (3 floats per bone per
vertex). This encoding is what the `Deform` animation timeline expects to
see unchanged on the wire.

Consumers that need per-vertex-axis counts (e.g. deform timelines) compute
`deformLength = vertices.len() / 3 * 2` for weighted and
`deformLength = vertices.len()` for unweighted.

### Sequence record (used by Region and Mesh)

```
count      : uvarint    ; frame count
start      : uvarint    ; first frame number (typically 1)
digits     : uvarint    ; zero-pad width for filenames
setupIndex : uvarint    ; frame shown in setup pose
```

**Frame path formatting** (replicates `spine-cpp/Sequence::getPath`):

    frame_path(basePath, i) = basePath + format("{:0>digits$}", start + i)

No separator is inserted between `basePath` and the frame number — the
editor already includes any delimiter in the base. For example,
`base = "left-wing"`, `start = 1`, `digits = 2`, `i = 0` produces
`"left-wing01"`.

A loader backed by an atlas should resolve each frame's region individually
and populate the sequence's `regions[]` array. The attachment's own
`region` reference is left unset when a sequence is present — rendering
pulls from `sequence.regions[current_frame]` instead.

# Linked mesh resolution

After all skins have been read, iterate recorded linked-mesh entries and
attach them to their parents:

1. Look up `parentMesh = skins[parentSkinIdx].getAttachment(slotIdx, parentName)`.
   Must be a `Mesh` attachment; error if missing.
2. Copy the parent's `bones`, `vertices`, `regionUVs`, `triangles`,
   `hullLength`, `edges` into the linked mesh.
3. Record `linkedMesh.parentMesh = parentMeshId`.
4. Set `linkedMesh.timelineAttachment` to either the parent (if
   `inheritTimeline` was true) or the linked mesh itself (if false). This
   controls which attachment's deform keyframes the linked mesh samples at
   runtime.
5. If the linked mesh carries an atlas region (no sequence), run its
   `updateRegion` recomputation so its UVs pick up the parent's region
   mapping.

# Events

```
numEvents : uvarint
for i in 0..numEvents:
    name        : string
    intValue    : signed varint (zigzag)
    floatValue  : float
    stringValue : string
    audioPath   : string
    if audioPath not empty:
        volume  : float
        balance : float
```

The presence of `volume` and `balance` is gated by the *presence* of
`audioPath`, not a flag bit. Events without audio default to `volume = 1.0`,
`balance = 0.0`.

# Animations

```
numAnimations : uvarint
for i in 0..numAnimations:
    name  : string
    body  : Animation record
```

Each animation record:

```
numTimelines : uvarint   ; hint; unused (timelines are counted implicitly
                         ; via the per-section counts below)

; --- Slot timelines ---
numSlotTimelines : uvarint
for _ in 0..numSlotTimelines:
    slotIdx : uvarint
    n       : uvarint
    for _ in 0..n:
        ttype : byte  ; SlotTimelineType
        frameCount : uvarint
        body depends on ttype (see below)

; --- Bone timelines ---
numBoneTimelines : uvarint
for _ in 0..numBoneTimelines:
    boneIdx : uvarint
    n       : uvarint
    for _ in 0..n:
        ttype : byte  ; BoneTimelineType
        frameCount : uvarint
        body depends on ttype (see below)

; --- IK constraint timelines ---
; --- Transform constraint timelines ---
; --- Path constraint timelines ---
; --- Physics constraint timelines ---
; --- Attachment timelines (Deform and Sequence) ---
; --- Draw order timeline ---
; --- Event timeline ---
```

Every section starts with its own `uvarint` count. Sections with count `0`
consume only that single byte.

## Curve encoding (shared by most timelines)

A "curve timeline" consists of frames and per-frame curve data. Each frame
has a fixed number of `f32` entries determined by the timeline type
("entries" = `1 + value channels`, always including time at index 0).

The layout is: read the first frame's `(time, values...)` unconditionally.
For each subsequent frame, read `(time, values...)` for that frame and a
1-byte **curve type** that describes the transition between the previous
frame and this one:

| Curve type (`sbyte`) | Meaning                                                 |
| -------------------- | ------------------------------------------------------- |
| `0` `LINEAR`         | Linear interpolation; no extra data.                    |
| `1` `STEPPED`        | Step (hold previous value); no extra data.              |
| `2` `BEZIER`         | Cubic bezier with 4-float control points per channel.   |

For `BEZIER`, one 4-float `(cx1, cy1, cx2, cy2)` block is read **per value
channel**. A `RotateTimeline` (1 channel) reads 4 floats; a
`TranslateTimeline` (2 channels) reads 8 floats; a `Rgba2Timeline`
(7 channels) reads 28 floats.

The `bezierCount` read before each curve timeline is a hint indicating the
total number of bezier segments across all value channels and all frames —
useful for pre-allocating the runtime segmentation table but not strictly
needed for parsing.

## Slot timelines

### `SlotTimelineType` enum

| Value | Name           | Value channels     |
| ----- | -------------- | ------------------ |
| 0     | `Attachment`   | 0 (string-ref)     |
| 1     | `Rgba`         | 4 (byte channels)  |
| 2     | `Rgb`          | 3 (byte channels)  |
| 3     | `Rgba2`        | 7 (byte channels)  |
| 4     | `Rgb2`         | 6 (byte channels)  |
| 5     | `Alpha`        | 1 (byte channel)   |

### `Attachment` slot timeline

```
for _ in 0..frameCount:
    time        : float
    attachment  : string-ref   ; None = hide the slot's attachment
```

No bezier curves — every frame is a step.

### Color slot timelines (`Rgba`, `Rgb`, `Rgba2`, `Rgb2`, `Alpha`)

```
bezierCount : uvarint
for first frame:
    time   : float
    for each color channel:
        value : byte   ; byte / 255.0

for each subsequent frame:
    time : float
    for each color channel:
        value : byte
    curveType : sbyte
    if curveType == BEZIER:
        for each color channel:
            4 floats (cx1, cy1, cx2, cy2)
```

The number of channels depends on the timeline variant (see table above).

## Bone timelines

### `BoneTimelineType` enum

| Value | Name         | Value channels | Scaling  |
| ----- | ------------ | -------------- | -------- |
| 0     | `Rotate`     | 1              | none     |
| 1     | `Translate`  | 2              | world    |
| 2     | `TranslateX` | 1              | world    |
| 3     | `TranslateY` | 1              | world    |
| 4     | `Scale`      | 2              | none     |
| 5     | `ScaleX`     | 1              | none     |
| 6     | `ScaleY`     | 1              | none     |
| 7     | `Shear`      | 2              | none     |
| 8     | `ShearX`     | 1              | none     |
| 9     | `ShearY`     | 1              | none     |
| 10    | `Inherit`    | special        | none     |

Types 0–9 are standard curve timelines: read `bezierCount : uvarint`, then
use the shared curve encoding with `entries = 1 + channels`. Scaled types
multiply value channels (not time) by the loader's `scale`.

### `Inherit` bone timeline (type 10)

```
for _ in 0..frameCount:
    time    : float
    inherit : byte   ; Inherit enum value, 0..4
```

**No bezier curves, no `bezierCount` prefix.** The inherit value is a plain
byte per frame. See Gotchas.

## Constraint timelines

### IK constraint timeline

```
idx         : uvarint, into ikConstraints[]
frameCount  : uvarint
bezierCount : uvarint
flags (first frame) : byte
time (first frame)  : float
if flags & 1:
    if flags & 2: mix = float
    else:         mix = 1
else:             mix = 0
if flags & 4: softness = float (scaled)
else:         softness = 0

for each subsequent frame:
    flags : byte
    time2 : float
    (mix2, softness2 read same as above)
    if flags & 64:  ; STEPPED
        no curve data
    else if flags & 128:  ; BEZIER
        4 floats for mix channel
        4 floats for softness channel
    else:  ; LINEAR
        no curve data
```

Flags also encode per-frame `bendDirection` (bit 8 → ±1), `compress`
(bit 16), `stretch` (bit 32).

### Transform constraint timeline

```
idx         : uvarint
frameCount  : uvarint
bezierCount : uvarint
```

Standard curve timeline with `entries = 7` (time + 6 mix channels:
`mixRotate`, `mixX`, `mixY`, `mixScaleX`, `mixScaleY`, `mixShearY`).

### Path constraint timelines

Each path-constraint-indexed group contains multiple per-property
timelines:

```
idx   : uvarint
numSub : uvarint
for _ in 0..numSub:
    ptype      : byte   ; PathTimelineType
    frameCount : uvarint
    bezierCount : uvarint
    body : standard curve timeline
```

| `ptype` | Timeline                   | Entries | Scaling                                     |
| ------- | -------------------------- | ------- | ------------------------------------------- |
| 0       | `PathConstraintPosition`   | 2       | scaled if data.positionMode == Fixed        |
| 1       | `PathConstraintSpacing`    | 2       | scaled if data.spacingMode ∈ {Length, Fixed} |
| 2       | `PathConstraintMix`        | 4       | none                                        |

### Physics constraint timelines

```
idxPlusOne : uvarint    ; 0 = "all physics constraints", n > 0 = constraint n-1
numSub     : uvarint
for _ in 0..numSub:
    ptype : byte  ; PhysicsTimelineType
    frameCount : uvarint
    if ptype == 8 (RESET):
        frames[frameCount] : each a float (time)
    else:
        bezierCount : uvarint
        body : standard curve timeline, entries = 2
```

| `ptype` | Property      |
| ------- | ------------- |
| 0       | `Inertia`     |
| 1       | `Strength`    |
| 2       | `Damping`     |
| 4       | `Mass`        |
| 5       | `Wind`        |
| 6       | `Gravity`     |
| 7       | `Mix`         |
| 8       | `Reset`       |

Note that discriminant `3` is unused (reserved). See Gotchas for the
`idxPlusOne` encoding.

## Attachment timelines (Deform and Sequence)

```
numSkinGroups : uvarint
for _ in 0..numSkinGroups:
    skinIdx : uvarint       ; into skins[]
    numSlotGroups : uvarint
    for _ in 0..numSlotGroups:
        slotIdx : uvarint
        numAtts : uvarint
        for _ in 0..numAtts:
            attName : string-ref   ; must resolve to an attachment in this skin/slot
            ttype   : byte          ; 0 = Deform, 1 = Sequence
            frameCount : uvarint
            body depends on ttype
```

### Deform timeline

```
bezierCount : uvarint
time (first) : float
for each frame:
    end : uvarint
    if end == 0:
        no vertex data this frame; resolve to weighted-zero or unweighted
        setup-pose values at apply time
    else:
        start : uvarint
        read `end` floats into deform[start..start+end]   ; scaled

    if frame is not the last:
        time2 : float
        curveType : sbyte
        if curveType == BEZIER:
            4 floats (single channel)
```

`deformLength` is `vertices.len() / 3 * 2` for weighted mesh attachments and
`vertices.len()` otherwise.

### Sequence timeline

```
for _ in 0..frameCount:
    time         : float
    modeAndIndex : int   ; packed: mode = low 4 bits, frameIndex = upper bits
    delay        : float
```

`SequenceMode` values (low 4 bits of `modeAndIndex`):

| Value | Mode              |
| ----- | ----------------- |
| 0     | `Hold`            |
| 1     | `Once`            |
| 2     | `Loop`            |
| 3     | `PingPong`        |
| 4     | `OnceReverse`     |
| 5     | `LoopReverse`     |
| 6     | `PingPongReverse` |

## Draw order timeline

```
drawOrderCount : uvarint
if drawOrderCount == 0: ; skip
else:
    for _ in 0..drawOrderCount:
        time        : float
        offsetCount : uvarint
        if offsetCount == 0:
            no changes this frame (keep setup-pose draw order)
        else:
            for _ in 0..offsetCount:
                slotIdx : uvarint          ; next modified slot
                shift   : varint-signed    ; see Gotchas
                ; The slot at `slotIdx` moves to position `slotIdx + shift`
                ; in the new draw order.
```

The reconstruction algorithm (from `spine-cpp`): build a `-1`-sentinel
`drawOrder[slotCount]`, then for each offset place
`drawOrder[slotIdx + shift] = slotIdx`. Unchanged slots fill remaining
`-1` positions in their original order.

## Event timeline

```
eventCount : uvarint
if eventCount == 0: ; skip
else:
    for _ in 0..eventCount:
        time         : float
        eventIdx     : uvarint, into events[]
        intValue     : signed varint    ; overrides EventData.intValue
        floatValue   : float            ; overrides EventData.floatValue
        stringValue  : string           ; None → inherit from EventData
        if EventData.audioPath is non-empty:
            volume  : float
            balance : float
```

# Gotchas / porter's guide

Four issues surfaced during the Rust port that are not obvious from the
byte-layout tables alone. Any porter should check these early.

## 1. Mesh triangle-count formula mixes units

The count passed to `readShortArray(triangles)` is `(verticesLength - hullLength - 2) * 3`.

- `verticesLength` is `vertexCount * 2` — the "doubled" vertex count used for
  2-float interleaved arrays.
- `hullLength` is the *undoubled* vertex count of the hull boundary.
- The formula therefore mixes units but produces the correct triangle count
  because a simple hull of `H` vertices yields `H - 2` triangles and interior
  vertices each add 2 triangles.

Sanity-check: for a 10-vertex all-hull mesh: `(20 - 10 - 2) * 3 = 24` =
8 triangles × 3 indices. ✓

spine-cpp: `SkeletonBinary.cpp:636`. spine-ts: `SkeletonBinary.ts:414`.

## 2. `Inherit` has two different encodings

In `BoneData` it's an **unsigned varint**. In the `InheritTimeline`
(bone timeline type 10) each frame's value is a **single byte**.

- `spine-cpp/SkeletonBinary.cpp:166`: `static_cast<Inherit>(readVarint(input, true))` (bone header)
- `spine-cpp/SkeletonBinary.cpp:1092`: `Inherit inherit = (Inherit) readByte(input)` (timeline frame)

A naive port that reuses the same "read inherit" helper for both contexts
reads multi-byte garbage in the timeline path. Symptom: animation parsing
drifts a few bytes and later sections explode at apparently-random offsets.

## 3. DrawOrder `shift` is unsigned-as-signed

Spine writes the `shift` as an unsigned varint, but the editor emits
negative values (e.g. "slot moves 2 positions earlier") as two's-complement
bit patterns — so `-2` on the wire is `0xFFFFFFFE` (five varint bytes).

`spine-cpp/SkeletonBinary.cpp:1440` reads this with `(size_t) readVarint(input, true)`
and does `drawOrder[index + (size_t) shift] = ...`. The `size_t(-2) + 9`
wraps in unsigned arithmetic to `7`, producing the correct target index
at any pointer width.

In a language that doesn't widen the unsigned conversion the same way
(Rust's `u32 as u64 as usize` on 64-bit is `0xFFFFFFFE`, not
`0xFFFFFFFFFFFFFFFE`), this naive approach crashes with an out-of-bounds
index of ~4×10⁹. **Fix: read the shift as signed `i32` and do the
addition in signed arithmetic before casting back.**

Only animations that actually move at least one slot earlier in draw order
exercise this path. Animations with empty or forward-only draw-order
changes silently pass a buggy implementation.

## 4. Sequence attachments resolve per-frame paths, not a single base

When a region or mesh attachment carries a `Sequence`, its `path` is a
**base** — actual atlas regions are looked up per-frame using
`frame_path(base, i) = base + zero_padded(start + i, digits)`.

A loader that calls `atlas.find_region(path)` directly on the attachment's
path fails for any sequence-backed attachment because the base ("left-wing")
doesn't exist in the atlas — only its numbered frames ("left-wing01",
"left-wing02", …) do.

The correct pattern:

- If the attachment record has a sequence: populate `sequence.regions[i]` by
  looking up `frame_path(path, i)` for each frame; leave the attachment's
  direct region unset.
- Otherwise: resolve `path` as a single region and store it on the attachment.

## 5. Physics timeline constraint index is 1-indexed with 0 as sentinel

Physics timelines encode the constraint reference as an unsigned varint
that is one greater than the actual index:

- `0` on the wire → `None` / "apply to all physics constraints" (used for
  `PhysicsReset` timelines).
- `n > 0` on the wire → `physicsConstraints[n - 1]`.

`spine-cpp/SkeletonBinary.cpp:1288` uses `int index = readVarint(true) - 1`,
accepting `-1` as the "all constraints" marker. Ports using unsigned
indices throughout need an `Option` or sentinel to handle this case.

# Verification checklist for new ports

Run these against a full Spine 4.2 example set
(`spine-runtimes/examples/*/export/*.skel`, 25+ rigs):

- [ ] Every `.skel` parses to non-empty `bones`, `slots`, `animations`.
- [ ] `SkeletonData::version` starts with `"4.2"` for all of them.
- [ ] Bones are emitted parent-first: for every bone `b` with a parent,
      `parent.index < b.index`.
- [ ] The total bytes consumed equals the file size (no trailing garbage,
      no early EOF).
- [ ] Spineboy (pro export) has `root`, `hip`, `head` bones and
      `walk`, `run`, `jump`, `idle` animations with plausible durations.
- [ ] Dragon loads without a "region not found" error (exercises sequence
      attachments).
- [ ] Celestial-circus loads (exercises physics constraints + timelines).
- [ ] Spineboy-ess's "run" animation loads (exercises DrawOrder with a
      negative shift — catches gotcha #3).

# References

- `spine-cpp/spine-cpp/include/spine/SkeletonBinary.h` — constant definitions
  (discriminants for bone / slot / attachment / path / physics timeline types).
- `spine-cpp/spine-cpp/src/spine/SkeletonBinary.cpp` — authoritative loader
  implementation.
- `spine-runtimes/spine-ts/spine-core/src/SkeletonBinary.ts` — TypeScript
  port; often easier to read than the C++.
- `dm_spine_runtime/src/load/binary/parse.rs` — this project's Rust port.
