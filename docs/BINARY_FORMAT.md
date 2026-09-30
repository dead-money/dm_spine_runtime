# Spine 4.3 Binary `.skel` Format Reference

This document describes the wire format of the Spine editor's binary skeleton
export (`.skel`) for version 4.3.x, in enough detail to implement a loader
from scratch. It captures every structural decision, subtle encoding trick,
and gotcha surfaced while porting `spine-cpp/SkeletonBinary.cpp` to Rust.

The format is little-documented outside the code itself. The canonical
reference implementations are:

- `spine-cpp/src/spine/SkeletonBinary.cpp` (the C++ port the Spine team
  maintains — authoritative when in doubt).
- `spine-ts/spine-core/src/SkeletonBinary.ts` (TypeScript port; often easier
  to read).
- `dm_spine_runtime/src/load/binary/parse.rs` (Rust port, this project;
  verified against every 4.3 example export).

Wherever this document cites line numbers, they refer to the 4.3
`spine-cpp` / `spine-ts` sources.

## Changes from 4.2

For porters updating a 4.2 loader:

- Version string starts with `"4.3"`.
- Bones: `inherit` is a byte and precedes `length`; nonessential adds
  `iconSize` and `iconRotation` floats after `icon`.
- The four per-type constraint sections (IK, transform, path, physics) are
  replaced by **one ordered constraint list**; each entry has a type byte
  (`0` IK, `1` path, `2` transform, `3` physics, `4` slider). The per-constraint
  `order` field is gone — list order is update order.
- IK: flag bit `2` gates a `scaleYMode` byte; bend direction is now an
  *inverted* bit (`4` set → `-1`); the other bits shift up by one.
- Transform constraint rewritten: `source` bone, flags
  `localSource/localTarget/additive/clamp`, a list of from/to property
  records, then an offsets flag byte and a mix flag byte.
- Path: `skinRequired` moved into the flags byte; mode bits shifted up by one.
- Physics: a negative `scaleX` carries `scaleYMode`.
- New **slider** constraint; its animation indices are written **after the
  animations section**.
- Named skins have a single constraint-index list (into the unified list)
  instead of four.
- Weighted vertices are preceded by a varint giving the total length of the
  `bones` array.
- Meshes carry a `timelineSlots` list.
- Linked meshes write `sourceIndex` (slot) before `skinIndex`.
- Clipping attachments have `convex` / `inverse` flag bits.
- Every region and mesh has a `Sequence`; when the flag bit is clear it is an
  implicit 1-frame sequence with no path suffix.
- Animations: new slider timeline section (after physics), new draw-order
  folder timelines (after draw order), and a nonessential animation color
  `int` at the very end of each animation.
- Constraint timeline indices point into the unified constraint list.

## Conventions

Throughout: **big-endian** byte order; counts / indices are encoded as
variable-length unsigned integers ("unsigned varint"); signed values that
benefit from compact encoding use a zigzag-encoded varint.

Fixed-size primitives:

| Type         | Size  | Notes                                                      |
| ------------ | ----- | ---------------------------------------------------------- |
| `byte`       | 1 B   | `u8`.                                                      |
| `sbyte`      | 1 B   | `i8`. Used for curve-type discriminants and property types. |
| `bool`       | 1 B   | Stored as a byte; non-zero means true.                     |
| `int`        | 4 B   | Big-endian `i32`.                                          |
| `float`      | 4 B   | Big-endian IEEE-754 single precision (bit-reinterpreted `int`). |
| `rgba`       | 4 B   | Four bytes R, G, B, A. Each is `u8 / 255.0` as a float.    |

## Varints

A varint is 1–5 bytes. Each byte contributes its low 7 bits to the value, with
the MSB (`0x80`) acting as a continuation flag. The 5th byte's high bit is a
hard cap — anything beyond is a format error. (`spine-cpp` silently truncates;
stricter parsers should error.)

**Unsigned varint** (`readInt(true)`, "optimize positive"): bytes accumulate
in little-endian septets directly.

    byte 0: C b b b b b b b   ─► value[0..7]
    byte 1: C b b b b b b b   ─► value[7..14]
    byte 2: C b b b b b b b   ─► value[14..21]
    byte 3: C b b b b b b b   ─► value[21..28]
    byte 4: _ b b b b b b b   ─► value[28..35] (low 4 bits only used)

**Signed varint** (zigzag, `readInt(false)`): read the value as if unsigned,
then zigzag-decode:

    signed = (unsigned >> 1) ^ -(unsigned & 1)

Used for fields that are typically small but may be negative (event
`intValue`).

### "Unsigned-as-signed" trick (DrawOrder shifts)

One field — the per-slot `shift` in draw-order and draw-order-folder
timelines — is written as an unsigned varint but interpreted semantically as
signed via integer wraparound. `spine-cpp` reads the varint as `int`, casts
to `size_t`, and adds it to an index; on two's-complement hardware
`(size_t)(-2) + 9 == 7` at any pointer width, which produces the correct
target index. **Porters on languages without unsigned wrap-based arithmetic
(e.g. Rust's `usize` on 64-bit) must read the shift as signed `i32` and do
the addition in signed arithmetic before casting back.** See Gotchas.

## Strings

Every string is length-prefixed by an unsigned varint `n`:

- `n == 0` → the string is `None` / null. No payload follows.
- `n > 0` → `n - 1` bytes of UTF-8 follow (no trailing NUL on the wire).

Many strings appear repeatedly (attachment names, placeholder names, etc.)
and are deduplicated through a **string table** written early in the file.
A "string-ref" is an unsigned varint that either indexes the table or
encodes `None`:

- `0` → `None`.
- `n > 0` → `table[n - 1]`.

String-refs cannot resolve to the empty string; raw length-prefixed strings
must be used for payloads that may be empty.

## Colors

Four bytes R, G, B, A, each divided by 255 to produce an `f32` in `[0, 1]`.

The `SlotData` "dark color" is a 4-byte `int` where `-1` (all `0xFF`) means
"no dark color." See the Slots section.

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
6. [Constraints](#constraints) — one ordered list, all types.
7. [Default skin](#default-skin) (only written if non-empty).
8. [Named skins](#named-skins).
9. [Linked mesh resolution](#linked-mesh-resolution) — not a stream section,
   but must happen before animations are read.
10. [Events](#events).
11. [Animations](#animations).
12. [Slider animation indices](#slider-animation-indices) — one uvarint per
    slider constraint.

Parsing should consume the entire file; leftover bytes indicate a drift.

# Header

| Order | Field                | Encoding                      | Notes                                                              |
| ----- | -------------------- | ----------------------------- | ------------------------------------------------------------------ |
| 1     | `hashLow`            | `int`                         | Low 32 bits of skeleton hash.                                      |
| 2     | `hashHigh`           | `int`                         | High 32 bits of skeleton hash.                                     |
| 3     | `version`            | string                        | Editor version string, e.g. `"4.3.12"`. Must start with `"4.3"`.   |
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

Accept any version beginning with `"4.3"`; a mismatching major/minor is a
hard error.

# String table

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
    inherit    : byte enum (see Inherit below)
    length     : float (scaled)
    skinRequired : bool
    if nonessential:
        color        : rgba
        icon         : string
        iconSize     : float
        iconRotation : float
        visible      : bool
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

Encoded as a single byte both here and in the `Inherit` bone timeline.

# Slots

```
numSlots : uvarint
for i in 0..numSlots:
    name           : string
    boneIdx        : uvarint (into bones[])
    color          : rgba
    darkColor      : int, see below
    attachmentName : string-ref
    blendMode      : uvarint enum (see below)
    if nonessential:
        visible    : bool
```

Slots are written in setup-pose draw order.

## Dark color encoding

A 4-byte `int`. If it equals `-1` (all bytes `0xFF`), the slot has *no* dark
color. Otherwise it is read as `rgb888` from the low three bytes:
`r = (v >> 16) & 0xFF`, `g = (v >> 8) & 0xFF`, `b = v & 0xFF`, alpha `1.0`.
The high byte is only a sentinel.

## `BlendMode` enum

| Value | Name       |
| ----- | ---------- |
| 0     | `Normal`   |
| 1     | `Additive` |
| 2     | `Multiply` |
| 3     | `Screen`   |

# Constraints

All constraints share one list. Its order is the constraint update order;
there is no separate `order` field. Every constraint reference elsewhere in
the file (skins, animation timelines) indexes this list.

```
numConstraints : uvarint
for i in 0..numConstraints:
    name : string
    type : byte   ; ConstraintType
    body : depends on type (below)
```

| `type` | Constraint  |
| ------ | ----------- |
| 0      | IK          |
| 1      | Path        |
| 2      | Transform   |
| 3      | Physics     |
| 4      | Slider      |

Fields not present on the wire keep their constructor defaults, which are
`0` for every setup-pose mix, offset, position, spacing and softness.

## `ScaleYMode` enum

Used by IK and physics.

| Value | Name      |
| ----- | --------- |
| 0     | `None`    |
| 1     | `Uniform` |
| 2     | `Volume`  |

## `TransformProperty` enum

Used by transform-constraint from/to records and slider properties. Stored
as a byte.

| Value | Property | Scaled |
| ----- | -------- | ------ |
| 0     | `Rotate` | no     |
| 1     | `X`      | yes    |
| 2     | `Y`      | yes    |
| 3     | `ScaleX` | no     |
| 4     | `ScaleY` | no     |
| 5     | `ShearY` | no     |

## IK constraint (type 0)

```
numBones    : uvarint
bones[numBones] : uvarint each, into bones[]
target      : uvarint, into bones[]
flags       : byte (bit layout below)
if flags & 2:
    scaleYMode : byte   ; ScaleYMode
if flags & 32:
    mix     : float  (only if flags & 64; else mix = 1)
if flags & 128:
    softness : float (scaled)
```

| Bit   | Meaning                                                                          |
| ----- | -------------------------------------------------------------------------------- |
| `1`   | `skinRequired`.                                                                  |
| `2`   | `scaleYMode` byte follows.                                                       |
| `4`   | `bendDirection = -1` if set, else `1` (inverted relative to 4.2).                |
| `8`   | `compress`.                                                                      |
| `16`  | `stretch`.                                                                       |
| `32`  | `mix` is non-zero. If unset, `mix = 0`.                                          |
| `64`  | Only meaningful when bit 32 is set: if set, read `mix` as a float; else `mix = 1`. |
| `128` | `softness` is present. If unset, `softness = 0`.                                 |

There is no `uniform` bit; `scaleYMode` replaces it.

## Transform constraint (type 2)

```
numBones    : uvarint
bones[numBones] : uvarint each, into bones[]
source      : uvarint, into bones[]
flags       : byte
    skinRequired = flags & 1
    localSource  = flags & 2
    localTarget  = flags & 4
    additive     = flags & 8
    clamp        = flags & 16
    numFrom      = flags >> 5          ; 0..7
for _ in 0..numFrom:
    fromType : byte  ; TransformProperty
    offset   : float (× fromScale)
    numTo    : byte
    for _ in 0..numTo:
        toType : byte  ; TransformProperty
        offset : float (× toScale)
        max    : float (× toScale)
        scale  : float (× toScale / fromScale)
offsetFlags : byte
    if & 1:  offsetRotation : float
    if & 2:  offsetX        : float (scaled)
    if & 4:  offsetY        : float (scaled)
    if & 8:  offsetScaleX   : float
    if & 16: offsetScaleY   : float
    if & 32: offsetShearY   : float
mixFlags : byte
    if & 1:  mixRotate : float
    if & 2:  mixX      : float
    if & 4:  mixY      : float
    if & 8:  mixScaleX : float
    if & 16: mixScaleY : float
    if & 32: mixShearY : float
```

`fromScale` / `toScale` are the loader `scale` for `X` / `Y` properties and
`1` otherwise. The to-record `scale` is a ratio, so it is divided by the
from-scale.

## Path constraint (type 1)

```
numBones      : uvarint
bones[numBones] : uvarint each, into bones[]
target        : uvarint, into slots[]
flags         : byte
    skinRequired = flags & 1
    positionMode = (flags >> 1) & 1
    spacingMode  = (flags >> 2) & 3
    rotateMode   = (flags >> 4) & 3
if flags & 128: offsetRotation : float
position      : float  (scaled if positionMode == Fixed)
spacing       : float  (scaled if spacingMode in {Length, Fixed})
mixRotate     : float
mixX          : float
mixY          : float
```

| Value | `PositionMode` | `SpacingMode`   | `RotateMode`   |
| ----- | -------------- | --------------- | -------------- |
| 0     | `Fixed`        | `Length`        | `Tangent`      |
| 1     | `Percent`      | `Fixed`         | `Chain`        |
| 2     | —              | `Percent`       | `ChainScale`   |
| 3     | —              | `Proportional`  | —              |

## Physics constraint (type 3)

```
bone         : uvarint, into bones[]
flagsA       : byte
if flagsA & 2:  x       : float
if flagsA & 4:  y       : float
if flagsA & 8:  rotate  : float
if flagsA & 16: scaleX  : float   ; also carries scaleYMode, see below
if flagsA & 32: shearX  : float
limit       : float (scaled)   ; read if flagsA & 64; else 5000 (then scaled)
step        : byte              ; stored as fps; step = 1 / byte
inertia     : float
strength    : float
damping     : float
massInverse : float             ; read if flagsA & 128; else 1.0
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
mix         : float             ; read if flagsB & 128; else 1.0
```

`flagsA & 1` = `skinRequired`.

**`scaleX` sign encodes `scaleYMode`:**

| Raw value `v`  | `scaleYMode` | `scaleX`    |
| -------------- | ------------ | ----------- |
| `v >= 0`       | `None`       | `v`         |
| `-2 <= v < 0`  | `Uniform`    | `-1 - v`    |
| `v < -2`       | `Volume`     | `-2 - v`    |

The `*Global` booleans mean the parameter is driven by the skeleton-wide
value at runtime rather than this constraint's own field.

## Slider constraint (type 4)

```
flags : byte
if flags & 8:
    value : float   ; max if (nonessential && flags & 64), else setup time
if flags & 16:
    mix   : float   (only if flags & 32; else mix = 1)
if flags & 64:
    bone         : uvarint, into bones[]
    propOffset   : float   ; property offset (× propertyScale)
    propertyType : byte    ; TransformProperty
    offset       : float   ; slider offset
    scale        : float   ; ÷ propertyScale
```

| Bit   | Meaning                                                             |
| ----- | ------------------------------------------------------------------- |
| `1`   | `skinRequired`.                                                     |
| `2`   | `loop`.                                                             |
| `4`   | `additive`.                                                         |
| `8`   | A value float follows (meaning depends on bit 64 + nonessential).   |
| `16`  | `mix` is non-zero. If unset, `mix = 0`.                             |
| `32`  | Only meaningful with bit 16: read `mix` as a float; else `mix = 1`. |
| `64`  | Bone-driven: bone/property block follows.                           |
| `128` | `local` (only meaningful with bit 64).                              |

`propertyScale` is the loader `scale` for `X` / `Y`, else `1`. The slider's
animation is **not** in this record; see
[Slider animation indices](#slider-animation-indices).

# Default skin

```
slotCount : uvarint
if slotCount == 0:
    ; no default skin; skip the rest of this block
else:
    read attachment-block for each of slotCount slots (see below)
```

The default skin has no name, color, bone list or constraint list on the
wire.

# Named skins

```
numSkins : uvarint
for i in 0..numSkins:
    name             : string
    if nonessential:
        color        : rgba        ; informational only
    numSkinBones     : uvarint
    skinBones[]      : uvarint each, into bones[]
    numConstraints   : uvarint
    skinConstraints[]: uvarint each, into constraints[] (unified list)
    slotCount        : uvarint
    read attachment-block for each of slotCount slots (see below)
```

Skin indices: the default skin (if present) is index 0 and named skins
follow. A skin-required bone or constraint is only active when at least one
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

The `placeholderName` is the skin lookup key; it may differ from the
attachment's own `name` (which may be a texture path on the atlas).

## Attachment record

```
flags : byte
    type = flags & 0x7         ; AttachmentType enum
    nameOverride = flags & 8
name  : string-ref  (only if nameOverride; else name = placeholder)
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
| 5     | `Point`        | Oriented point.                                          |
| 6     | `Clipping`     | Polygonal mask. Applies until `endSlot` in draw order.   |

### `Region` attachment

```
path      : string-ref        ; only if flags & 16; else path = name
color     : rgba              ; only if flags & 32; else (1,1,1,1)
sequence  : Sequence record   ; fields only if flags & 64; see below
rotation  : float             ; only if flags & 128; else 0
x         : float (scaled)
y         : float (scaled)
scaleX    : float
scaleY    : float
width     : float (scaled)
height    : float (scaled)
```

### `BoundingBox` attachment

```
vertices  : Vertices record   ; flags & 16 → weighted
if nonessential:
    color : rgba
```

### `Mesh` attachment

```
path       : string-ref  ; only if flags & 16; else path = name
color      : rgba        ; only if flags & 32
sequence   : Sequence    ; fields only if flags & 64
hullLength : uvarint     ; hull vertex count (undoubled); stored ×2 at runtime
vertices   : Vertices record  ; flags & 128 → weighted
uvs        : float[verticesLength]     ; not scaled
triangles  : uvarint[(verticesLength - hullLength - 2) * 3]
numTimelineSlots : uvarint
timelineSlots    : uvarint[numTimelineSlots], into slots[]
if nonessential:
    edgesCount : uvarint
    edges      : uvarint[edgesCount]
    width      : float (scaled)
    height     : float (scaled)
```

`verticesLength` is `vertexCount * 2` from the Vertices record. See Gotchas
for the mixed units in the triangle count.

`timelineSlots` lists additional slots whose deform/sequence timelines this
mesh responds to. An empty list leaves the runtime default.

### `LinkedMesh` attachment

```
path            : string-ref       ; only if flags & 16; else path = name
color           : rgba             ; only if flags & 32
sequence        : Sequence         ; fields only if flags & 64
inheritTimelines: (flags & 128)    ; no bytes
sourceIndex     : uvarint          ; slot index of the source mesh
skinIndex       : uvarint          ; index into skins (default skin = 0)
source          : string-ref       ; placeholder name in that skin/slot
if nonessential:
    width       : float (scaled)
    height      : float (scaled)
```

No vertex data on the wire — resolved after all skins load. See
[Linked mesh resolution](#linked-mesh-resolution).

### `Path` attachment

```
closed        : (flags & 16)
constantSpeed : (flags & 32)
vertices      : Vertices record  ; flags & 64 → weighted
lengths       : float[verticesLength / 6]   ; per-segment arc length, scaled
if nonessential:
    color     : rgba
```

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
endSlotIdx : uvarint  ; into slots[]
vertices   : Vertices record  ; flags & 16 → weighted
if nonessential:
    color  : rgba
```

`convex = flags & 32`, `inverse = flags & 64`.

### Vertices record (shared)

Used by mesh, bounding box, path, and clipping attachments.

```
vertexCount : uvarint
verticesLength = vertexCount * 2

if unweighted:
    vertices : float[verticesLength]     ; interleaved xy, scaled
else (weighted):
    bonesLength : uvarint                 ; total length of the bones array
    while bones.len() < bonesLength:
        boneCount : uvarint               ; appended to `bones`
        for each of boneCount bones:
            boneIdx : uvarint             ; appended to `bones`
            bx      : float (scaled)
            by      : float (scaled)
            weight  : float
```

`bonesLength = vertexCount + Σ boneCount`, so the weighted `vertices` array
has `(bonesLength - vertexCount) * 3` floats. Loop on `bonesLength`, not on
`vertexCount` (the two terminate identically for valid files, but
`bonesLength` must be consumed).

Deform length: `vertices.len() / 3 * 2` for weighted, `vertices.len()` for
unweighted.

### Sequence record (Region, Mesh, LinkedMesh)

Every region and mesh has a sequence. The record's bytes are present only
when the attachment's sequence flag (`64`) is set:

```
if flag set:
    count      : uvarint    ; frame count
    start      : uvarint    ; first frame number
    digits     : uvarint    ; zero-pad width
    setupIndex : uvarint    ; frame shown in setup pose
    ; pathSuffix = true
else:
    ; implicit: count = 1, pathSuffix = false, no bytes read
```

**Frame path** (`Sequence::getPath`):

    frame_path(basePath, i) = pathSuffix
        ? basePath + format("{:0>digits$}", start + i)
        : basePath

No separator is inserted. `base = "left-wing"`, `start = 1`, `digits = 2`,
`i = 0` → `"left-wing01"`.

The atlas loader populates `sequence.regions[i]` from `frame_path(path, i)`
for every attachment; rendering reads the current frame's region.

# Linked mesh resolution

After all skins are read, for each recorded linked mesh:

1. `source = skins[skinIndex].getAttachment(sourceIndex, source)`. Must be a
   mesh; error if missing. (The slot is `sourceIndex` from the record, not
   the slot the linked mesh itself lives in.)
2. Set the timeline attachment to `source` if `inheritTimelines`, else to
   the linked mesh itself.
3. `setSourceMesh(source)` — copies bones, vertices, region UVs, triangles,
   hull length, edges, width/height.
4. `updateSequence()` so UVs pick up the region mapping.

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

Presence of `volume` / `balance` is gated by `audioPath` being non-empty.
Defaults: `volume = 1.0`, `balance = 0.0`.

# Animations

```
numAnimations : uvarint
for i in 0..numAnimations:
    name  : string
    body  : Animation record
```

Each animation record, in order:

```
numTimelines : uvarint   ; capacity hint only

; --- Slot timelines ---
; --- Bone timelines ---
; --- IK constraint timelines ---
; --- Transform constraint timelines ---
; --- Path constraint timelines ---
; --- Physics constraint timelines ---
; --- Slider timelines ---
; --- Attachment timelines (Deform and Sequence) ---
; --- Draw order timeline ---
; --- Draw order folder timelines ---
; --- Event timeline ---
if nonessential:
    color : rgba          ; animation color, after the event timeline
```

Every section starts with its own `uvarint` count. Sections with count `0`
consume only that byte. Duration is the max last-frame time over all
timelines (not on the wire).

## Curve encoding (shared by most timelines)

Each frame has `entries = 1 + channels` floats (time first). Read the first
frame's `(time, values...)`. For each subsequent frame read `(time,
values...)` then an `sbyte` **curve type** for the transition from the
previous frame:

| Curve type (`sbyte`) | Meaning                                                 |
| -------------------- | ------------------------------------------------------- |
| `0` `LINEAR`         | Linear interpolation; no extra data.                    |
| `1` `STEPPED`        | Step (hold previous value); no extra data.              |
| `2` `BEZIER`         | Cubic bezier: 4 floats `(cx1, cy1, cx2, cy2)` per channel. |

For `BEZIER`, `cy1` / `cy2` are multiplied by the channel's value scale
(e.g. loader `scale` for translate). `bezierCount` (read before the frames)
is the total number of bezier channel segments and sizes the runtime curve
table.

## Slot timelines

```
numSlots : uvarint
for _ in 0..numSlots:
    slotIdx : uvarint
    n       : uvarint
    for _ in 0..n:
        ttype      : byte  ; SlotTimelineType
        frameCount : uvarint
        body
```

| Value | Name           | Value channels     |
| ----- | -------------- | ------------------ |
| 0     | `Attachment`   | 0 (string-ref)     |
| 1     | `Rgba`         | 4 (byte channels)  |
| 2     | `Rgb`          | 3 (byte channels)  |
| 3     | `Rgba2`        | 7 (byte channels)  |
| 4     | `Rgb2`         | 6 (byte channels)  |
| 5     | `Alpha`        | 1 (byte channel)   |

### `Attachment`

```
for _ in 0..frameCount:
    time        : float
    attachment  : string-ref   ; None = hide
```

No curves, no `bezierCount`.

### Color (`Rgba`, `Rgb`, `Rgba2`, `Rgb2`, `Alpha`)

```
bezierCount : uvarint
first frame: time : float, then one byte per channel (byte / 255)
each subsequent frame:
    time : float
    one byte per channel
    curveType : sbyte
    if BEZIER: 4 floats per channel
```

## Bone timelines

```
numBones : uvarint
for _ in 0..numBones:
    boneIdx : uvarint
    n       : uvarint
    for _ in 0..n:
        ttype      : byte  ; BoneTimelineType
        frameCount : uvarint
        body
```

| Value | Name         | Value channels | Scaling  |
| ----- | ------------ | -------------- | -------- |
| 0     | `Rotate`     | 1              | none     |
| 1     | `Translate`  | 2              | scaled   |
| 2     | `TranslateX` | 1              | scaled   |
| 3     | `TranslateY` | 1              | scaled   |
| 4     | `Scale`      | 2              | none     |
| 5     | `ScaleX`     | 1              | none     |
| 6     | `ScaleY`     | 1              | none     |
| 7     | `Shear`      | 2              | none     |
| 8     | `ShearX`     | 1              | none     |
| 9     | `ShearY`     | 1              | none     |
| 10    | `Inherit`    | special        | none     |

Types 0–9: `bezierCount : uvarint`, then the shared curve encoding.

### `Inherit` (type 10)

```
for _ in 0..frameCount:
    time    : float
    inherit : byte   ; Inherit enum
```

No curves, no `bezierCount`.

## Constraint timelines

All constraint indices below index the **unified constraint list**. A
loader should check the referenced constraint has the expected type.

### IK constraint timelines

```
n : uvarint
for _ in 0..n:
    idx         : uvarint, into constraints[] (must be IK)
    frameCount  : uvarint
    bezierCount : uvarint
    first frame:
        flags : byte
        time  : float
        mix      = flags & 1 ? (flags & 2 ? float : 1) : 0
        softness = flags & 4 ? float (scaled) : 0
    each subsequent frame:
        flags : byte
        time2 : float
        mix2, softness2 as above
        if flags & 64:   STEPPED
        elif flags & 128: BEZIER — 4 floats for mix, 4 floats for softness
                          (softness cy values scaled)
        else:            LINEAR
```

Per-frame flags also carry `bendDirection` (`8` → `1`, else `-1`),
`compress` (`16`), `stretch` (`32`). Note the timeline bend bit is *not*
inverted, unlike the IK constraint data flags.

### Transform constraint timelines

```
n : uvarint
for _ in 0..n:
    idx         : uvarint, into constraints[] (must be transform)
    frameCount  : uvarint
    bezierCount : uvarint
    body        : curve timeline, entries = 7
```

Channels: `mixRotate`, `mixX`, `mixY`, `mixScaleX`, `mixScaleY`, `mixShearY`.

### Path constraint timelines

```
n : uvarint
for _ in 0..n:
    idx    : uvarint, into constraints[] (must be path)
    numSub : uvarint
    for _ in 0..numSub:
        ptype       : byte
        frameCount  : uvarint
        bezierCount : uvarint
        body        : curve timeline
```

| `ptype` | Timeline                   | Entries | Scaling                                      |
| ------- | -------------------------- | ------- | -------------------------------------------- |
| 0       | `PathConstraintPosition`   | 2       | scaled if data.positionMode == Fixed         |
| 1       | `PathConstraintSpacing`    | 2       | scaled if data.spacingMode ∈ {Length, Fixed} |
| 2       | `PathConstraintMix`        | 4       | none                                         |

### Physics constraint timelines

```
n : uvarint
for _ in 0..n:
    idxPlusOne : uvarint    ; 0 = all physics constraints, k > 0 = constraints[k - 1]
    numSub     : uvarint
    for _ in 0..numSub:
        ptype      : byte
        frameCount : uvarint
        if ptype == 8 (Reset):
            frameCount floats (time only)
        else:
            bezierCount : uvarint
            body        : curve timeline, entries = 2
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

Discriminant `3` is unused. `k - 1` indexes the unified constraint list.

### Slider timelines

```
n : uvarint
for _ in 0..n:
    idx    : uvarint, into constraints[] (must be slider)
    numSub : uvarint
    for _ in 0..numSub:
        stype       : byte   ; 0 = SliderTime, 1 = SliderMix
        frameCount  : uvarint
        bezierCount : uvarint
        body        : curve timeline, entries = 2, unscaled
```

## Attachment timelines (Deform and Sequence)

```
numSkins : uvarint
for _ in 0..numSkins:
    skinIdx  : uvarint       ; into skins[]
    numSlots : uvarint
    for _ in 0..numSlots:
        slotIdx : uvarint
        numAtts : uvarint
        for _ in 0..numAtts:
            attName    : string-ref   ; must resolve in this skin/slot
            ttype      : byte          ; 0 = Deform, 1 = Sequence
            frameCount : uvarint
            body
```

### Deform

```
bezierCount : uvarint
time : float                    ; first frame
for each frame:
    end : uvarint
    if end == 0:
        no data; weighted → zeros, unweighted → setup vertices
    else:
        start : uvarint
        end floats into deform[start .. start + end]   ; scaled
        ; unweighted: setup vertices are added to the frame at load time
    if not the last frame:
        time2     : float
        curveType : sbyte
        if BEZIER: 4 floats (one 0→1 percent channel)
```

### Sequence

```
for _ in 0..frameCount:
    time         : float
    modeAndIndex : int     ; mode = low 4 bits, index = value >> 4
    delay        : float
```

| Value | `SequenceMode`    |
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
frameCount : uvarint
for _ in 0..frameCount:
    time      : float
    drawOrder : draw-order record over slotCount = slots.len()
```

**Draw-order record** (shared with folder timelines):

```
changeCount : uvarint
if changeCount == 0:
    ; None — restore setup order
else:
    for _ in 0..changeCount:
        slotIdx : uvarint           ; ascending
        shift   : uvarint, signed via wraparound (see Gotchas)
```

Reconstruction (`readDrawOrder`): fill `drawOrder[slotCount]` with `-1`;
walk slots in original order, collecting skipped ones into `unchanged[]`;
for each change set `drawOrder[slotIdx + shift] = slotIdx`; append the
remaining slots to `unchanged[]`; then fill `-1` positions from the back
with `unchanged[]` popped from the back.

## Draw order folder timelines

```
numFolders : uvarint
for _ in 0..numFolders:
    folderSlotCount : uvarint
    folderSlots     : uvarint[folderSlotCount], into slots[]
    keyCount        : uvarint
    for _ in 0..keyCount:
        time      : float
        drawOrder : draw-order record over slotCount = folderSlotCount
```

Indices inside a folder's draw-order record are **folder-local** (positions
in `folderSlots`), not skeleton slot indices.

## Event timeline

```
eventCount : uvarint
for _ in 0..eventCount:
    time         : float
    eventIdx     : uvarint, into events[]
    intValue     : signed varint
    floatValue   : float
    stringValue  : string           ; None → inherit EventData.stringValue
    if EventData.audioPath is non-empty:
        volume  : float
        balance : float
```

# Slider animation indices

After the last animation, one `uvarint` is written **per slider
constraint**, in constraint-list order, giving the index into `animations[]`
that the slider drives:

```
for c in constraints where c.type == Slider:
    animationIdx : uvarint
```

Constraints of other types contribute nothing. This is the final section of
the file.

# Gotchas / porter's guide

Issues not obvious from the byte-layout tables alone.

## 1. Mesh triangle-count formula mixes units

The triangle array length is `(verticesLength - hullLength - 2) * 3`, where
`verticesLength = vertexCount * 2` but `hullLength` is the *undoubled* hull
vertex count as read from the wire. The runtime then stores
`hullLength << 1`. Compute the triangle count from the raw wire value
before doubling.

Sanity check, 10-vertex all-hull mesh: `(20 - 10 - 2) * 3 = 24` = 8
triangles. ✓

spine-cpp: `SkeletonBinary.cpp:653`. spine-ts: `SkeletonBinary.ts:530`.

## 2. DrawOrder `shift` is unsigned-as-signed

`shift` is an unsigned varint, but negative shifts are written as
two's-complement bit patterns (`-2` → `0xFFFFFFFE`, five varint bytes).
`spine-cpp` (`SkeletonBinary.cpp:1461`) does
`drawOrder[index + (size_t) readInt(true)]`, relying on unsigned wraparound.

In Rust, `u32 as usize` on 64-bit gives `0xFFFFFFFE`, not
`0xFFFF_FFFF_FFFF_FFFE`, and indexes out of bounds. **Read the shift as
`i32` and add in signed arithmetic.** The same record is used by
draw-order folder timelines, so fix it once in the shared helper.

Only animations that move a slot earlier exercise this path.

## 3. Slider animation indices trail the animations section

Slider constraints are read before animations exist, so their animation
reference is written after the animations section — one uvarint per slider,
in constraint order. A loader that stops after the animations leaves
trailing bytes and sliders without an animation. A loader that tries to
read the index inside the slider record desynchronises immediately.

spine-cpp: `SkeletonBinary.cpp:536`.

## 4. Constraint indices are into one unified list

Skins and all constraint timelines (IK, transform, path, physics, slider)
index the single constraint list, not a per-type list. A 4.2-style loader
that keeps separate per-type arrays will resolve the wrong constraint (or
go out of range) as soon as a rig mixes constraint types. Validate the type
at each reference.

## 5. Physics timeline index is 1-based with 0 as "all"

`0` → apply to every physics constraint (typical for `Reset`); `k > 0` →
`constraints[k - 1]`. spine-cpp (`SkeletonBinary.cpp:1189`) reads
`readInt(true) - 1` and treats `-1` as "all". Other constraint timelines
are 0-based.

## 6. Every region and mesh has a sequence

When flag `64` is clear no bytes are read, but the attachment still gets a
`Sequence(count = 1, pathSuffix = false)`, and its single region is looked
up via `frame_path`, which returns the base path unchanged. When the flag
is set, `path` is a **base**: the atlas contains only the numbered frames
(`"left-wing01"`, …), so looking up the base directly fails. Always resolve
regions through the sequence.

## 7. IK bend bit is inverted in data but not in timelines

IK constraint data: `flags & 4` set → `bendDirection = -1`. IK timeline
frames: `flags & 8` set → `bendDirection = +1`. Porting one from the other
flips every bend.

## 8. Physics `scaleX` carries `scaleYMode`

There is no separate byte. Negative `scaleX` values decode to `Uniform`
(`-2 <= v < 0`, `scaleX = -1 - v`) or `Volume` (`v < -2`,
`scaleX = -2 - v`). Reading the float as-is yields a negative scale mix.

## 9. Weighted vertices have a bones-length prefix

Weighted vertex data starts with an extra uvarint (total `bones` array
length) after `vertexCount`. Omitting it shifts every subsequent weighted
read by one varint.

## 10. Linked mesh source slot comes from the record

The source mesh is looked up in slot `sourceIndex` (read before
`skinIndex`), which may differ from the slot the linked mesh is attached
to.

## 11. Nonessential data appears in unexpected places

Beyond the header: bone `color/icon/iconSize/iconRotation/visible`, slot
`visible`, skin `color`, attachment colors, mesh `edges/width/height`,
linked-mesh `width/height`, the slider `max` (via flag bits 8 + 64), and the
animation `color` after each animation's event timeline. Missing any one of
these desynchronises the rest of the file.

# Verification checklist for new ports

Run against the full 4.3 example set (`spine-runtimes/examples/*/export/*.skel`):

- [ ] Every `.skel` parses to non-empty `bones`, `slots`, `animations`.
- [ ] `SkeletonData::version` starts with `"4.3"` for all of them.
- [ ] Bones are parent-first: for every bone `b` with a parent,
      `parent.index < b.index`.
- [ ] Total bytes consumed equals file size (includes the trailing slider
      animation indices).
- [ ] Both `-ess` and `-pro` exports load where a rig has both (exercises
      the nonessential paths).
- [ ] Spineboy (pro) has `root`, `hip`, `head` bones and `walk`, `run`,
      `jump`, `idle` animations with plausible durations.
- [ ] Dragon loads without "region not found" (sequence attachments).
- [ ] Celestial-circus loads (physics constraints + timelines).
- [ ] Spineboy-ess "run" loads (draw order with a negative shift — gotcha #2).

# References

- `spine-cpp/include/spine/SkeletonBinary.h` — discriminant constants (bone
  / slot / attachment / constraint / path / physics / slider timeline types).
- `spine-cpp/src/spine/SkeletonBinary.cpp` — authoritative loader.
- `spine-ts/spine-core/src/SkeletonBinary.ts` — TypeScript port.
- `dm_spine_runtime/src/load/binary/parse.rs` — this project's Rust port.
