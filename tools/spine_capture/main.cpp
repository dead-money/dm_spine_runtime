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

// spine_capture — dumps spine-cpp's bone transforms as JSON so the
// dm_spine_runtime Rust port can compare against bit-for-bit goldens.
//
// Phase 5 upgrade: the full constraint pipeline is now enabled (IK,
// Transform, Path, Physics) via Skeleton::updateWorldTransform. Phase
// 2 / 3 goldens were captured without this — 5e re-captures them.
//
// usage:
//   spine_capture <atlas> <skel> <out.json>
//       Setup-pose snapshot with constraints applied.
//
//   spine_capture --anim <atlas> <skel> <out.json> <anim> <time>
//       Animation sample: set to setup pose, apply <anim> at <time>
//       seconds, then updateWorldTransform(Physics_None). Matches the
//       dm_spine_runtime Phase 5 pipeline end-to-end.

#include <spine/spine.h>

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

using namespace spine;

// spine-cpp leaves one symbol for integrators to define — the allocator hook.
// Default implementation (malloc/free) is provided by DefaultSpineExtension.
spine::SpineExtension *spine::getDefaultExtension() {
    return new DefaultSpineExtension();
}

// Atlas construction requires a TextureLoader, but we never touch pixels here —
// setup-pose bone transforms are derived from SkeletonData alone. Return a
// stub page so Atlas construction succeeds.
class NullTextureLoader : public TextureLoader {
    void load(AtlasPage &page, const String &path) override {
        (void)page;
        (void)path;
    }
    void unload(void *texture) override { (void)texture; }
};

static const char *inherit_name(Inherit i) {
    switch (i) {
        case Inherit_Normal: return "normal";
        case Inherit_OnlyTranslation: return "onlyTranslation";
        case Inherit_NoRotationOrReflection: return "noRotationOrReflection";
        case Inherit_NoScale: return "noScale";
        case Inherit_NoScaleOrReflection: return "noScaleOrReflection";
    }
    return "unknown";
}

static std::string json_escape(const char *s) {
    std::string out;
    for (; *s; ++s) {
        unsigned char c = static_cast<unsigned char>(*s);
        switch (c) {
            case '"': out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\b': out += "\\b"; break;
            case '\f': out += "\\f"; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default:
                if (c < 0x20) {
                    char buf[8];
                    snprintf(buf, sizeof(buf), "\\u%04x", c);
                    out += buf;
                } else {
                    out += static_cast<char>(c);
                }
        }
    }
    return out;
}

int main(int argc, char **argv) {
    bool anim_mode = false;
    const char *atlas_path = nullptr;
    const char *skel_path = nullptr;
    const char *out_path = nullptr;
    const char *anim_name = nullptr;
    float anim_time = 0.0f;

    if (argc == 4) {
        atlas_path = argv[1];
        skel_path = argv[2];
        out_path = argv[3];
    } else if (argc == 7 && strcmp(argv[1], "--anim") == 0) {
        anim_mode = true;
        atlas_path = argv[2];
        skel_path = argv[3];
        out_path = argv[4];
        anim_name = argv[5];
        anim_time = static_cast<float>(atof(argv[6]));
    } else {
        fprintf(stderr,
                "usage:\n"
                "  %s <atlas> <skel> <out.json>\n"
                "  %s --anim <atlas> <skel> <out.json> <anim> <time>\n",
                argv[0], argv[0]);
        return 64;
    }

    NullTextureLoader texture_loader;
    Atlas atlas(atlas_path, &texture_loader, false);
    AtlasAttachmentLoader attachment_loader(&atlas);

    SkeletonBinary binary(&attachment_loader);
    SkeletonData *data = binary.readSkeletonDataFile(skel_path);
    if (!data) {
        fprintf(stderr, "failed to load %s: %s\n",
                skel_path, binary.getError().buffer());
        return 1;
    }

    FILE *out = fopen(out_path, "w");
    if (!out) {
        fprintf(stderr, "cannot open %s for writing\n", out_path);
        delete data;
        return 2;
    }

    {
        Skeleton skeleton(data);
        skeleton.setToSetupPose();

        // Animation mode: apply the named animation at `anim_time` against
        // setup pose (MixBlend_Setup, alpha 1, direction In) so timelines
        // overwrite local TRS before the constraint pipeline runs.
        if (anim_mode) {
            Animation *anim = data->findAnimation(anim_name);
            if (!anim) {
                fprintf(stderr, "no animation named '%s' in %s\n",
                        anim_name, skel_path);
                delete data;
                fclose(out);
                return 3;
            }
            anim->apply(skeleton, -1.0f, anim_time, false, nullptr,
                        1.0f, MixBlend_Setup, MixDirection_In);
        }

        // Full constraint pipeline: IK, Transform, Path, Physics all run.
        // Phase 5 goldens diff against this output.
        skeleton.updateWorldTransform(Physics_None);
        Vector<Bone *> &bones_for_pose = skeleton.getBones();

        fprintf(out, "{\n");
        fprintf(out, "  \"source_skel\": \"%s\",\n",
                json_escape(skel_path).c_str());
        fprintf(out, "  \"source_atlas\": \"%s\",\n",
                json_escape(atlas_path).c_str());
        fprintf(out, "  \"physics\": \"none\",\n");
        if (anim_mode) {
            fprintf(out, "  \"animation\": \"%s\",\n",
                    json_escape(anim_name).c_str());
            fprintf(out, "  \"time\": %.9g,\n", anim_time);
        }
        fprintf(out, "  \"skeleton_x\": %.9g,\n", skeleton.getX());
        fprintf(out, "  \"skeleton_y\": %.9g,\n", skeleton.getY());
        fprintf(out, "  \"scale_x\": %.9g,\n", skeleton.getScaleX());
        fprintf(out, "  \"scale_y\": %.9g,\n", skeleton.getScaleY());
        fprintf(out, "  \"bones\": [\n");

        Vector<Bone *> &bones = skeleton.getBones();
        for (size_t i = 0; i < bones.size(); ++i) {
            Bone *b = bones[i];
            BoneData &bd = b->getData();
            fprintf(out, "    {\n");
            fprintf(out, "      \"index\": %d,\n", bd.getIndex());
            fprintf(out, "      \"name\": \"%s\",\n",
                    json_escape(bd.getName().buffer()).c_str());
            fprintf(out, "      \"parent\": ");
            if (bd.getParent()) {
                fprintf(out, "%d,\n", bd.getParent()->getIndex());
            } else {
                fprintf(out, "null,\n");
            }
            fprintf(out, "      \"inherit\": \"%s\",\n",
                    inherit_name(bd.getInherit()));
            fprintf(out, "      \"active\": %s,\n",
                    b->isActive() ? "true" : "false");
            fprintf(out, "      \"a\": %.9g,\n", b->getA());
            fprintf(out, "      \"b\": %.9g,\n", b->getB());
            fprintf(out, "      \"c\": %.9g,\n", b->getC());
            fprintf(out, "      \"d\": %.9g,\n", b->getD());
            fprintf(out, "      \"world_x\": %.9g,\n", b->getWorldX());
            fprintf(out, "      \"world_y\": %.9g,\n", b->getWorldY());
            fprintf(out, "      \"ax\": %.9g,\n", b->getAX());
            fprintf(out, "      \"ay\": %.9g,\n", b->getAY());
            fprintf(out, "      \"a_rotation\": %.9g,\n", b->getAppliedRotation());
            fprintf(out, "      \"a_scale_x\": %.9g,\n", b->getAScaleX());
            fprintf(out, "      \"a_scale_y\": %.9g,\n", b->getAScaleY());
            fprintf(out, "      \"a_shear_x\": %.9g,\n", b->getAShearX());
            fprintf(out, "      \"a_shear_y\": %.9g\n", b->getAShearY());
            fprintf(out, "    }%s\n", i + 1 == bones.size() ? "" : ",");
        }

        fprintf(out, "  ]\n");
        fprintf(out, "}\n");
    }

    fclose(out);
    delete data;
    return 0;
}
