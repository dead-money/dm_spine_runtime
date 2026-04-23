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

// spine_synthetic — runs hand-built skeleton configurations through spine-cpp
// and prints each bone's computed a/b/c/d/world to stdout. Used to seed
// Phase 2 unit-test golden values for Inherit modes that no example rig
// exercises (notably `NoScaleOrReflection`).
//
// Not wired into capture_all.sh; run manually when adding a new case.

#include <spine/spine.h>

#include <cstdio>

using namespace spine;

spine::SpineExtension *spine::getDefaultExtension() {
    return new DefaultSpineExtension();
}

static void dump(const char *label, const SkeletonData &sd, const Vector<Bone *> &bones) {
    printf("=== %s ===\n", label);
    for (size_t i = 0; i < bones.size(); ++i) {
        Bone *b = bones[i];
        printf("bone[%zu] %s inherit=%d active=%d\n", i,
               b->getData().getName().buffer(),
               (int) b->getData().getInherit(),
               (int) b->isActive());
        printf("  a=%.9g b=%.9g c=%.9g d=%.9g world=(%.9g,%.9g)\n",
               b->getA(), b->getB(), b->getC(), b->getD(),
               b->getWorldX(), b->getWorldY());
    }
    (void) sd;
}

// Two bones: root (reflected on X via scale_x=-1) + child with the given
// inherit mode, at a specific local rotation/translation. Runs the same
// bones-only pose the capture harness dumps for real rigs.
static void run_case(const char *label, Inherit child_inherit) {
    SkeletonData sd;
    sd.setDefaultSkin(NULL);

    BoneData *root = new BoneData(0, "root", NULL);
    root->setX(10.0f);
    root->setY(5.0f);
    root->setScaleX(-1.0f);
    root->setScaleY(1.0f);
    root->setRotation(30.0f);
    sd.getBones().add(root);

    BoneData *child = new BoneData(1, "child", root);
    child->setX(20.0f);
    child->setY(0.0f);
    child->setRotation(45.0f);
    child->setScaleX(2.0f);
    child->setScaleY(0.5f);
    child->setShearX(10.0f);
    child->setShearY(-5.0f);
    child->setInherit(child_inherit);
    sd.getBones().add(child);

    Skeleton skeleton(&sd);
    skeleton.setToSetupPose();
    Vector<Bone *> &bones = skeleton.getBones();

    for (size_t i = 0; i < bones.size(); ++i) {
        Bone *b = bones[i];
        b->setAX(b->getX());
        b->setAY(b->getY());
        b->setAppliedRotation(b->getRotation());
        b->setAScaleX(b->getScaleX());
        b->setAScaleY(b->getScaleY());
        b->setAShearX(b->getShearX());
        b->setAShearY(b->getShearY());
    }
    for (size_t i = 0; i < bones.size(); ++i) {
        if (bones[i]->isActive()) {
            bones[i]->updateWorldTransform();
        }
    }

    dump(label, sd, bones);
}

int main() {
    run_case("NoScale", Inherit_NoScale);
    run_case("NoScaleOrReflection", Inherit_NoScaleOrReflection);
    return 0;
}
