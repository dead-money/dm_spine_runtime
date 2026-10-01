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

//! Skeleton loaders: turn a Spine 4.3 export into [`SkeletonData`].
//!
//! [`SkeletonBinary`] reads `.skel` files and [`SkeletonJson`] reads `.json`
//! files. Both create attachments through an [`AttachmentLoader`], which
//! resolves texture regions. [`AtlasAttachmentLoader`] resolves them against a
//! parsed [`Atlas`]; implement the trait yourself to resolve regions some other
//! way or to drop attachments.
//!
//! Load each asset once and share the result as `Arc<SkeletonData>` across
//! skeleton instances.
//!
//! ```no_run
//! use std::sync::Arc;
//! use spine_runtime::atlas::Atlas;
//! use spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
//!
//! let atlas = Atlas::parse(&std::fs::read_to_string("spineboy.atlas")?)?;
//! let mut loader = AtlasAttachmentLoader::new(&atlas);
//! let data = SkeletonBinary::with_loader(&mut loader)
//!     .with_scale(0.5)
//!     .read(&std::fs::read("spineboy-pro.skel")?)?;
//! let data = Arc::new(data);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! [`SkeletonData`]: crate::data::SkeletonData
//! [`Atlas`]: crate::atlas::Atlas

pub mod attachment_loader;
pub mod binary;
pub mod json;

pub use attachment_loader::{AtlasAttachmentLoader, AttachmentLoader, AttachmentLoaderError};
pub use binary::{BinaryError, SkeletonBinary};
pub use json::{JsonError, SkeletonJson};
