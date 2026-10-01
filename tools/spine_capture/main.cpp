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

// spine_capture: dumps spine-cpp 4.3 state as JSON for the spine_runtime
// goldens, and times spine-cpp for the benchmark comparison.
//
// usage:
//   spine_capture <atlas> <skel> <out.json>
//       Setup pose, constraints applied, Physics_None.
//   spine_capture --anim <atlas> <skel> <out.json> <anim> <time>
//       Setup pose, then <anim> applied at <time> from setup, then
//       updateWorldTransform(Physics_None).
//   spine_capture --render <atlas> <skel> <out.json>
//       SkeletonRenderer command summaries for the setup pose.
//   spine_capture --state <atlas> <skel> <out.json> <script>
//       Drives an AnimationState with a ';'-separated script and dumps
//       bone poses at each `dump` plus lifecycle events. Commands:
//       mix:D, set:T:ANIM:LOOP, add:T:ANIM:LOOP:DELAY, empty:T:DUR,
//       addempty:T:DUR:DELAY, additive:T:0|1, alpha:T:A,
//       interp:T:linear|smooth|slowFast|fastSlow|circle, step:N, dump.
//       Each step is update(1/60), apply, updateWorldTransform(Physics_None).
//   spine_capture --list <atlas> <skel>
//       Animation and skin names, one JSON line.
//   spine_capture --bench <atlas> <skel> <anim> <frames> [skin]
//       Times AnimationState update+apply, updateWorldTransform and render
//       over <frames> 60 Hz frames; <anim> "-" means the first animation.
//       Prints one JSON line to stdout.

#include <spine/spine.h>

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

using namespace spine;

spine::SpineExtension *spine::getDefaultExtension() {
    return new DefaultSpineExtension();
}

// Page textures are the page index so RenderCommand::texture matches the
// Rust TextureId(page_index).
class IndexTextureLoader : public TextureLoader {
    void load(AtlasPage &page, const String &path) override {
        (void) path;
        page.texture = reinterpret_cast<void *>(static_cast<intptr_t>(page.index));
    }
    void unload(void *texture) override { (void) texture; }
};

// "a.atlas+b.atlas" concatenates the files into one multi-page atlas, which is
// how hommlet pairs a rig atlas with its body atlas.
static Atlas *load_atlas(const char *spec, TextureLoader *loader) {
    if (!strchr(spec, '+')) return new Atlas(spec, loader, true);
    std::string text, dir, rest(spec);
    size_t start = 0;
    while (start <= rest.size()) {
        size_t end = rest.find('+', start);
        if (end == std::string::npos) end = rest.size();
        std::string path = rest.substr(start, end - start);
        if (dir.empty()) {
            size_t slash = path.find_last_of('/');
            dir = slash == std::string::npos ? "." : path.substr(0, slash);
        }
        FILE *f = fopen(path.c_str(), "rb");
        if (!f) {
            fprintf(stderr, "cannot read atlas %s\n", path.c_str());
            exit(1);
        }
        char buf[65536];
        size_t n;
        while ((n = fread(buf, 1, sizeof(buf), f)) > 0) text.append(buf, n);
        fclose(f);
        text += "\n";
        start = end + 1;
    }
    return new Atlas(text.c_str(), (int) text.size(), dir.c_str(), loader, true);
}

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

static std::string cache_entry_name(Skeleton &skeleton, Update *u) {
    const RTTI &rtti = u->getRTTI();
    if (rtti.isExactly(BonePose::rtti)) {
        // updateCache stores each bone's applied pose, which has no public
        // back-pointer to its bone.
        Array<Bone *> &bones = skeleton.getBones();
        for (size_t i = 0; i < bones.size(); ++i)
            if (static_cast<Update *>(&bones[i]->getAppliedPose()) == u)
                return std::string("bone:") + bones[i]->getData().getName().buffer();
    }
    if (rtti.isExactly(IkConstraint::rtti)) return std::string("ik:") + ((IkConstraint *) u)->getData().getName().buffer();
    if (rtti.isExactly(TransformConstraint::rtti))
        return std::string("transform:") + ((TransformConstraint *) u)->getData().getName().buffer();
    if (rtti.isExactly(PathConstraint::rtti)) return std::string("path:") + ((PathConstraint *) u)->getData().getName().buffer();
    if (rtti.isExactly(PhysicsConstraint::rtti))
        return std::string("physics:") + ((PhysicsConstraint *) u)->getData().getName().buffer();
    if (rtti.isExactly(Slider::rtti)) return std::string("slider:") + ((Slider *) u)->getData().getName().buffer();
    return "unknown";
}

static void write_render(FILE *out, Skeleton &skeleton, const char *skel_path, const char *atlas_path) {
    SkeletonRenderer renderer;
    RenderCommand *head = renderer.render(skeleton);
    fprintf(out, "{\n");
    fprintf(out, "  \"source_skel\": \"%s\",\n", json_escape(skel_path).c_str());
    fprintf(out, "  \"source_atlas\": \"%s\",\n", json_escape(atlas_path).c_str());
    fprintf(out, "  \"commands\": [\n");
    bool first = true;
    for (RenderCommand *cmd = head; cmd != NULL; cmd = cmd->next) {
        if (!first) fprintf(out, ",\n");
        first = false;
        bool any = cmd->numVertices > 0;
        intptr_t tex = reinterpret_cast<intptr_t>(cmd->texture);
        fprintf(out,
                "    {\"texture\": %zd, \"blend\": %d, "
                "\"num_vertices\": %d, \"num_indices\": %d, "
                "\"color\": %u, \"dark_color\": %u, "
                "\"first_pos\": [%.6g, %.6g], "
                "\"last_pos\": [%.6g, %.6g], "
                "\"first_uv\": [%.6g, %.6g]}",
                (ssize_t) tex, (int) cmd->blendMode, cmd->numVertices, cmd->numIndices,
                any ? cmd->colors[0] : 0u, any ? cmd->darkColors[0] : 0u,
                any ? cmd->positions[0] : 0.0f, any ? cmd->positions[1] : 0.0f,
                any ? cmd->positions[cmd->numVertices * 2 - 2] : 0.0f,
                any ? cmd->positions[cmd->numVertices * 2 - 1] : 0.0f,
                any ? cmd->uvs[0] : 0.0f, any ? cmd->uvs[1] : 0.0f);
    }
    fprintf(out, "\n  ]\n}\n");
}

static void write_bones(FILE *out, Skeleton &skeleton, const char *indent) {
    fprintf(out, "%s\"bones\": [\n", indent);
    Array<Bone *> &bones = skeleton.getBones();
    for (size_t i = 0; i < bones.size(); ++i) {
        Bone *b = bones[i];
        BoneData &bd = b->getData();
        BonePose &p = b->getAppliedPose();
        // Constraints that write the world matrix leave the local pose stale
        // until validated; the goldens compare the validated values.
        p.validateLocalTransform(skeleton);
        fprintf(out, "%s  {\"index\": %d, \"name\": \"%s\", \"parent\": ", indent, bd.getIndex(),
                json_escape(bd.getName().buffer()).c_str());
        if (bd.getParent())
            fprintf(out, "%d", bd.getParent()->getIndex());
        else
            fprintf(out, "null");
        fprintf(out,
                ", \"inherit\": \"%s\", \"active\": %s, \"a\": %.9g, \"b\": %.9g, \"c\": %.9g, \"d\": %.9g, "
                "\"world_x\": %.9g, \"world_y\": %.9g, \"ax\": %.9g, \"ay\": %.9g, \"a_rotation\": %.9g, "
                "\"a_scale_x\": %.9g, \"a_scale_y\": %.9g, \"a_shear_x\": %.9g, \"a_shear_y\": %.9g}%s\n",
                inherit_name(p.getInherit()), b->isActive() ? "true" : "false", p.getA(), p.getB(), p.getC(), p.getD(),
                p.getWorldX(), p.getWorldY(), p.getX(), p.getY(), p.getRotation(), p.getScaleX(), p.getScaleY(),
                p.getShearX(), p.getShearY(), i + 1 == bones.size() ? "" : ",");
    }
    fprintf(out, "%s]\n", indent);
}

static void write_pose(FILE *out, Skeleton &skeleton, const char *skel_path, const char *atlas_path,
                       const char *anim_name, float anim_time) {
    fprintf(out, "{\n");
    fprintf(out, "  \"source_skel\": \"%s\",\n", json_escape(skel_path).c_str());
    fprintf(out, "  \"source_atlas\": \"%s\",\n", json_escape(atlas_path).c_str());
    fprintf(out, "  \"physics\": \"none\",\n");
    if (anim_name) {
        fprintf(out, "  \"animation\": \"%s\",\n", json_escape(anim_name).c_str());
        fprintf(out, "  \"time\": %.9g,\n", anim_time);
    }
    fprintf(out, "  \"skeleton_x\": %.9g,\n", skeleton.getX());
    fprintf(out, "  \"skeleton_y\": %.9g,\n", skeleton.getY());
    fprintf(out, "  \"scale_x\": %.9g,\n", skeleton.getScaleX());
    fprintf(out, "  \"scale_y\": %.9g,\n", skeleton.getScaleY());

    Array<Update *> &cache = skeleton.getUpdateCache();
    fprintf(out, "  \"update_cache\": [");
    for (size_t i = 0; i < cache.size(); ++i)
        fprintf(out, "%s\"%s\"", i ? ", " : "", json_escape(cache_entry_name(skeleton, cache[i]).c_str()).c_str());
    fprintf(out, "],\n");

    write_bones(out, skeleton, "  ");
    fprintf(out, "}\n");
}


struct StateLog {
    std::string events;
    bool first = true;
};

static void state_listener(AnimationState *state, EventType type, TrackEntry *entry, Event *event, void *user) {
    (void) state;
    StateLog *log = (StateLog *) user;
    const char *kind = "";
    switch (type) {
        case EventType_Start: kind = "start"; break;
        case EventType_Interrupt: kind = "interrupt"; break;
        case EventType_End: kind = "end"; break;
        case EventType_Complete: kind = "complete"; break;
        case EventType_Dispose: kind = "dispose"; break;
        case EventType_Event: kind = "event"; break;
    }
    char buf[512];
    snprintf(buf, sizeof(buf), "%s\"%s:%d:%s%s%s\"", log->first ? "" : ", ", kind, entry->getTrackIndex(),
             json_escape(entry->getAnimation().getName().buffer()).c_str(), event ? ":" : "",
             event ? json_escape(event->getData().getName().buffer()).c_str() : "");
    log->events += buf;
    log->first = false;
}

static Interpolation &interpolation_named(const std::string &name) {
    if (name == "smooth") return Interpolation::smooth();
    if (name == "slowFast") return Interpolation::slowFast();
    if (name == "fastSlow") return Interpolation::fastSlow();
    if (name == "circle") return Interpolation::circle();
    return Interpolation::linear();
}

static int run_state(SkeletonData &data, FILE *out, const char *skel_path, const char *atlas_path, const char *script) {
    Skeleton skeleton(data);
    skeleton.setupPose();
    AnimationStateData state_data(data);
    AnimationState state(state_data);
    StateLog log;
    state.setListener(state_listener, &log);

    fprintf(out, "{\n");
    fprintf(out, "  \"source_skel\": \"%s\",\n", json_escape(skel_path).c_str());
    fprintf(out, "  \"source_atlas\": \"%s\",\n", json_escape(atlas_path).c_str());
    fprintf(out, "  \"script\": \"%s\",\n", json_escape(script).c_str());
    fprintf(out, "  \"frames\": [\n");
    bool first_frame = true;

    std::string all(script);
    size_t start = 0;
    while (start < all.size()) {
        size_t end = all.find(';', start);
        if (end == std::string::npos) end = all.size();
        std::string cmd = all.substr(start, end - start);
        start = end + 1;
        std::vector<std::string> f;
        size_t p = 0;
        while (true) {
            size_t q = cmd.find(':', p);
            f.push_back(cmd.substr(p, q == std::string::npos ? std::string::npos : q - p));
            if (q == std::string::npos) break;
            p = q + 1;
        }
        const std::string &op = f[0];
        if (op == "mix") {
            state_data.setDefaultMix((float) atof(f[1].c_str()));
        } else if (op == "set") {
            state.setAnimation(atoi(f[1].c_str()), String(f[2].c_str()), f[3] == "1");
        } else if (op == "add") {
            state.addAnimation(atoi(f[1].c_str()), String(f[2].c_str()), f[3] == "1", (float) atof(f[4].c_str()));
        } else if (op == "empty") {
            state.setEmptyAnimation(atoi(f[1].c_str()), (float) atof(f[2].c_str()));
        } else if (op == "addempty") {
            state.addEmptyAnimation(atoi(f[1].c_str()), (float) atof(f[2].c_str()), (float) atof(f[3].c_str()));
        } else if (op == "additive") {
            state.getTrack(atoi(f[1].c_str()))->setAdditive(f[2] == "1");
        } else if (op == "alpha") {
            state.getTrack(atoi(f[1].c_str()))->setAlpha((float) atof(f[2].c_str()));
        } else if (op == "interp") {
            state.getTrack(atoi(f[1].c_str()))->setMixInterpolation(interpolation_named(f[2]));
        } else if (op == "step") {
            for (int i = 0, n = atoi(f[1].c_str()); i < n; i++) {
                state.update(1.0f / 60.0f);
                state.apply(skeleton);
                skeleton.updateWorldTransform(Physics_None);
            }
        } else if (op == "dump") {
            fprintf(out, "%s    {\n", first_frame ? "" : ",\n");
            first_frame = false;
            write_bones(out, skeleton, "      ");
            fprintf(out, "      ,\"slots\": [");
            Array<Slot *> &slots = skeleton.getSlots();
            for (size_t i = 0; i < slots.size(); ++i) {
                SlotPose &sp = slots[i]->getAppliedPose();
                Attachment *att = sp.getAttachment();
                Color &c = sp.getColor();
                fprintf(out, "%s{\"attachment\": ", i ? ", " : "");
                if (att)
                    fprintf(out, "\"%s\"", json_escape(att->getName().buffer()).c_str());
                else
                    fprintf(out, "null");
                fprintf(out, ", \"color\": [%.9g, %.9g, %.9g, %.9g], \"sequence_index\": %d}", c.r, c.g, c.b, c.a,
                        sp.getSequenceIndex());
            }
            fprintf(out, "],\n      \"draw_order\": [");
            Array<Slot *> &order = skeleton.getDrawOrder().getAppliedPose();
            for (size_t i = 0; i < order.size(); ++i)
                fprintf(out, "%s%d", i ? ", " : "", order[i]->getData().getIndex());
            fprintf(out, "]\n    }");
        } else if (!op.empty()) {
            fprintf(stderr, "unknown state command '%s'\n", op.c_str());
            return 64;
        }
    }
    fprintf(out, "\n  ],\n  \"events\": [%s]\n}\n", log.events.c_str());
    return 0;
}

static void run_list(SkeletonData &data) {
    printf("{\"animations\": [");
    Array<Animation *> &anims = data.getAnimations();
    for (size_t i = 0; i < anims.size(); ++i)
        printf("%s\"%s\"", i ? ", " : "", json_escape(anims[i]->getName().buffer()).c_str());
    printf("], \"skins\": [");
    Array<Skin *> &skins = data.getSkins();
    for (size_t i = 0; i < skins.size(); ++i)
        printf("%s\"%s\"", i ? ", " : "", json_escape(skins[i]->getName().buffer()).c_str());
    printf("]}\n");
}

static int run_bench(SkeletonData &data, const char *skel_path, const char *anim_name, int frames, const char *skin) {
    typedef std::chrono::steady_clock clock;
    Skeleton skeleton(data);
    if (skin) {
        if (!data.findSkin(skin)) {
            fprintf(stderr, "no skin named '%s'\n", skin);
            return 3;
        }
        skeleton.setSkin(skin);
    }
    skeleton.setupPose();
    AnimationStateData state_data(data);
    AnimationState state(state_data);
    if (strcmp(anim_name, "-") == 0) {
        if (data.getAnimations().size() == 0) {
            fprintf(stderr, "no animations in %s\n", skel_path);
            return 3;
        }
        anim_name = data.getAnimations()[0]->getName().buffer();
    } else if (!data.findAnimation(anim_name)) {
        fprintf(stderr, "no animation named '%s'\n", anim_name);
        return 3;
    }
    state.setAnimation(0, anim_name, true);
    SkeletonRenderer renderer;
    const float dt = 1.0f / 60.0f;
    double anim_ns = 0, world_ns = 0, render_ns = 0;
    size_t sink = 0;
    for (int f = 0; f < frames; ++f) {
        clock::time_point t0 = clock::now();
        state.update(dt);
        state.apply(skeleton);
        clock::time_point t1 = clock::now();
        skeleton.updateWorldTransform(Physics_None);
        clock::time_point t2 = clock::now();
        for (RenderCommand *cmd = renderer.render(skeleton); cmd; cmd = cmd->next) sink += cmd->numVertices;
        clock::time_point t3 = clock::now();
        anim_ns += std::chrono::duration<double, std::nano>(t1 - t0).count();
        world_ns += std::chrono::duration<double, std::nano>(t2 - t1).count();
        render_ns += std::chrono::duration<double, std::nano>(t3 - t2).count();
    }
    printf("{\"runtime\": \"spine-cpp\", \"skel\": \"%s\", \"animation\": \"%s\", \"skin\": \"%s\", \"frames\": %d, "
           "\"anim_ns\": %.1f, \"world_ns\": %.1f, \"render_ns\": %.1f, \"vertices\": %zu}\n",
           json_escape(skel_path).c_str(), json_escape(anim_name).c_str(), skin ? json_escape(skin).c_str() : "",
           frames, anim_ns / frames,
           world_ns / frames, render_ns / frames, sink / (size_t) (frames > 0 ? frames : 1));
    return 0;
}

int main(int argc, char **argv) {
    enum { SETUP, ANIM, RENDER, LIST, BENCH, STATE } mode;
    const char *atlas_path, *skel_path, *out_path = nullptr, *anim_name = nullptr, *skin = nullptr, *script = nullptr;
    float anim_time = 0.0f;
    int frames = 0;

    if (argc == 4 && argv[1][0] != '-') {
        mode = SETUP;
        atlas_path = argv[1];
        skel_path = argv[2];
        out_path = argv[3];
    } else if (argc == 7 && strcmp(argv[1], "--anim") == 0) {
        mode = ANIM;
        atlas_path = argv[2];
        skel_path = argv[3];
        out_path = argv[4];
        anim_name = argv[5];
        anim_time = static_cast<float>(atof(argv[6]));
    } else if (argc == 5 && strcmp(argv[1], "--render") == 0) {
        mode = RENDER;
        atlas_path = argv[2];
        skel_path = argv[3];
        out_path = argv[4];
    } else if (argc == 6 && strcmp(argv[1], "--state") == 0) {
        mode = STATE;
        atlas_path = argv[2];
        skel_path = argv[3];
        out_path = argv[4];
        script = argv[5];
    } else if (argc == 4 && strcmp(argv[1], "--list") == 0) {
        mode = LIST;
        atlas_path = argv[2];
        skel_path = argv[3];
    } else if ((argc == 6 || argc == 7) && strcmp(argv[1], "--bench") == 0) {
        mode = BENCH;
        atlas_path = argv[2];
        skel_path = argv[3];
        anim_name = argv[4];
        frames = atoi(argv[5]);
        if (argc == 7) skin = argv[6];
    } else {
        fprintf(stderr,
                "usage:\n"
                "  %s <atlas> <skel> <out.json>\n"
                "  %s --anim <atlas> <skel> <out.json> <anim> <time>\n"
                "  %s --render <atlas> <skel> <out.json>\n"
                "  %s --state <atlas> <skel> <out.json> <script>\n"
                "  %s --list <atlas> <skel>\n"
                "  %s --bench <atlas> <skel> <anim> <frames> [skin]\n",
                argv[0], argv[0], argv[0], argv[0], argv[0], argv[0]);
        return 64;
    }

    // spine-cpp defaults to y-down; spine-ts, libgdx and this port are y-up.
    Bone::setYDown(false);

    IndexTextureLoader texture_loader;
    Atlas *atlas = load_atlas(atlas_path, &texture_loader);
    AtlasAttachmentLoader attachment_loader(*atlas);
    SkeletonBinary binary(attachment_loader);
    SkeletonData *data = binary.readSkeletonDataFile(skel_path);
    if (!data) {
        fprintf(stderr, "failed to load %s: %s\n", skel_path, binary.getError().buffer());
        return 1;
    }

    if (mode == LIST) {
        run_list(*data);
        delete data;
        delete atlas;
        return 0;
    }
    if (mode == BENCH) {
        int rc = run_bench(*data, skel_path, anim_name, frames, skin);
        delete data;
        delete atlas;
        return rc;
    }

    FILE *out = fopen(out_path, "w");
    if (!out) {
        fprintf(stderr, "cannot open %s for writing\n", out_path);
        delete data;
        delete atlas;
        return 2;
    }

    int rc = 0;
    if (mode == STATE) {
        rc = run_state(*data, out, skel_path, atlas_path, script);
        fclose(out);
        delete data;
        delete atlas;
        return rc;
    }
    {
        Skeleton skeleton(*data);
        skeleton.setupPose();
        if (mode == ANIM) {
            Animation *anim = data->findAnimation(anim_name);
            if (!anim) {
                fprintf(stderr, "no animation named '%s' in %s\n", anim_name, skel_path);
                rc = 3;
            } else {
                anim->apply(skeleton, -1.0f, anim_time, false, nullptr, 1.0f, MixFrom_Setup, false, false, false);
            }
        }
        if (rc == 0) {
            skeleton.updateWorldTransform(Physics_None);
            if (mode == RENDER)
                write_render(out, skeleton, skel_path, atlas_path);
            else
                write_pose(out, skeleton, skel_path, atlas_path, anim_name, anim_time);
        }
    }

    fclose(out);
    delete data;
    delete atlas;
    return rc;
}
