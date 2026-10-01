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

//! Loader for binary `.skel` exports.
//!
//! [`SkeletonBinary`] parses a `.skel` buffer into a
//! [`SkeletonData`](crate::data::SkeletonData), creating attachments through an
//! [`AttachmentLoader`](crate::load::AttachmentLoader). Failures are reported
//! as [`BinaryError`]. Only Spine 4.3 exports are accepted.
//!
//! ```no_run
//! use std::sync::Arc;
//! use spine_runtime::atlas::Atlas;
//! use spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let atlas = Atlas::parse(&std::fs::read_to_string("spineboy.atlas")?)?;
//! let mut loader = AtlasAttachmentLoader::new(&atlas);
//! let bytes = std::fs::read("spineboy-pro.skel")?;
//! let data = Arc::new(SkeletonBinary::with_loader(&mut loader).read(&bytes)?);
//! # let _ = data;
//! # Ok(())
//! # }
//! ```

mod parse;
mod reader;

pub use parse::SkeletonBinary;
pub(crate) use parse::{link_mesh, timeline_duration};
pub use reader::BinaryError;
