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

// spine_capture — dumps spine-cpp's setup-pose bone transforms as JSON so
// the dm_spine_runtime Rust port can compare against bit-for-bit goldens.
//
// usage: spine_capture <atlas_path> <skel_path> <out_json>

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
    if (argc != 4) {
        fprintf(stderr, "usage: %s <atlas> <skel> <out.json>\n", argv[0]);
        return 64;
    }
    const char *atlas_path = argv[1];
    const char *skel_path = argv[2];
    const char *out_path = argv[3];

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
        skeleton.updateWorldTransform(Physics_None);

        fprintf(out, "{\n");
        fprintf(out, "  \"source_skel\": \"%s\",\n",
                json_escape(skel_path).c_str());
        fprintf(out, "  \"source_atlas\": \"%s\",\n",
                json_escape(atlas_path).c_str());
        fprintf(out, "  \"physics\": \"none\",\n");
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
