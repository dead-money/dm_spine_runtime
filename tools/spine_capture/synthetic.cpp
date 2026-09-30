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

// spine_synthetic: runs hand-built skeletons through spine-cpp and prints
// each bone's world transform, for cases no example rig exercises (notably
// Inherit NoScaleOrReflection). Run manually when adding a case.

#include <spine/spine.h>

#include <cstdio>

using namespace spine;

spine::SpineExtension *spine::getDefaultExtension() {
    return new DefaultSpineExtension();
}

static void dump(const char *label, Array<Bone *> &bones) {
    printf("=== %s ===\n", label);
    for (size_t i = 0; i < bones.size(); ++i) {
        Bone *b = bones[i];
        BonePose &p = b->getAppliedPose();
        printf("bone[%zu] %s inherit=%d active=%d\n", i, b->getData().getName().buffer(), (int) p.getInherit(),
               (int) b->isActive());
        printf("  a=%.9g b=%.9g c=%.9g d=%.9g world=(%.9g,%.9g)\n", p.getA(), p.getB(), p.getC(), p.getD(),
               p.getWorldX(), p.getWorldY());
    }
}

// Root reflected on X, plus a child with the given inherit mode.
static void run_case(const char *label, Inherit child_inherit) {
    SkeletonData sd;

    BoneData *root = new BoneData(0, "root", NULL);
    BonePose &rp = root->getSetupPose();
    rp.setX(10.0f);
    rp.setY(5.0f);
    rp.setScaleX(-1.0f);
    rp.setScaleY(1.0f);
    rp.setRotation(30.0f);
    sd.getBones().add(root);

    BoneData *child = new BoneData(1, "child", root);
    BonePose &cp = child->getSetupPose();
    cp.setX(20.0f);
    cp.setY(0.0f);
    cp.setRotation(45.0f);
    cp.setScaleX(2.0f);
    cp.setScaleY(0.5f);
    cp.setShearX(10.0f);
    cp.setShearY(-5.0f);
    cp.setInherit(child_inherit);
    sd.getBones().add(child);

    Skeleton skeleton(sd);
    skeleton.setupPose();
    skeleton.updateWorldTransform(Physics_None);
    dump(label, skeleton.getBones());
}

int main() {
    Bone::setYDown(false);
    run_case("NoScale", Inherit_NoScale);
    run_case("NoScaleOrReflection", Inherit_NoScaleOrReflection);
    return 0;
}
