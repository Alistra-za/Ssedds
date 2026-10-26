// Tessalith DDS: the remembered Save settings in Photoshop's Descriptor Registry (PIActions.h), stored as seven
// integers in one persistent descriptor (eight since Note DH). MIT licence (see photoshop/LICENSE).
#include "settings_store.h"

#include "PIActions.h"
#include "SPBasic.h"

namespace {

// Registry key, unique to this plug-in. Generated once; never change it, or users lose their settings.
const char kRegistryKey[] = "3845e6ae-fbf3-46dc-9b93-f505f11cd107";

// Descriptor keys: four-character codes read back only by this plug-in. The first six are required; 'disc' (DC36)
// came later, so settings stored before it load with discard_alpha 0; likewise 'alph' (Note DH) loads as alpha_mode 0.
constexpr int kCount = 8;
constexpr DescriptorKeyID kKeys[kCount] = {'fmt ', 'type', 'srgb', 'filt', 'mips', 'qual', 'disc', 'alph'};
constexpr int kRequired = 6;

uint32_t *field(PsddsSettings &s, int i) {
    uint32_t *f[kCount] = {&s.format, &s.texture_type, &s.srgb,         &s.filter,
                           &s.mips,   &s.quality,      &s.discard_alpha, &s.alpha_mode};
    return f[i];
}

// Both suites, released on scope exit. Usable only when both were acquired.
struct Suites {
    SPBasicSuite *basic;
    PSDescriptorRegistryProcs *registry = nullptr;
    PSActionDescriptorProcs *descriptor = nullptr;

    explicit Suites(SPBasicSuite *b) : basic(b) {
        if (!basic) return;
        const void *p = nullptr;
        if (basic->AcquireSuite(kPSDescriptorRegistrySuite, kPSDescriptorRegistrySuiteVersion, &p) == 0)
            registry = static_cast<PSDescriptorRegistryProcs *>(const_cast<void *>(p));
        p = nullptr;
        if (basic->AcquireSuite(kPSActionDescriptorSuite, kPSActionDescriptorSuiteVersion, &p) == 0)
            descriptor = static_cast<PSActionDescriptorProcs *>(const_cast<void *>(p));
    }
    ~Suites() {
        if (registry) basic->ReleaseSuite(kPSDescriptorRegistrySuite, kPSDescriptorRegistrySuiteVersion);
        if (descriptor) basic->ReleaseSuite(kPSActionDescriptorSuite, kPSActionDescriptorSuiteVersion);
    }
    Suites(const Suites &) = delete;
    Suites &operator=(const Suites &) = delete;
    bool ok() const { return registry && descriptor; }
};

} // namespace

PsddsSettings load_settings(SPBasicSuite *basic, uint32_t has_alpha) {
    const PsddsSettings defaults = psdds_defaults(PSDDS_COLOUR);
    Suites suites(basic);
    if (!suites.ok()) return defaults;

    PIActionDescriptor d = nullptr;
    if (suites.registry->Get(kRegistryKey, &d) != 0 || !d) { // first run
        if (d) suites.descriptor->Free(d); // a host that set one despite the error
        return defaults;
    }
    PsddsSettings s = defaults;
    bool complete = true;
    for (int i = 0; i < kCount && complete; i++) {
        int32 v = 0;
        const bool got = suites.descriptor->GetInteger(d, kKeys[i], &v) == 0;
        complete = got || i >= kRequired;
        *field(s, i) = got ? uint32_t(v) : 0; // a negative value becomes out of range, which psdds_resolve refuses
    }
    suites.descriptor->Free(d); // Get returns a copy owned by the caller

    // psdds_resolve validates the stored values; an out-of-range quality is fine (it is replaced when shown).
    PsddsSettings effective;
    PsddsControls controls;
    if (!complete || psdds_resolve(&s, has_alpha, &effective, &controls) != 0) return defaults;
    return s;
}

void save_settings(SPBasicSuite *basic, const PsddsSettings &s) {
    Suites suites(basic);
    if (!suites.ok()) return;
    PIActionDescriptor d = nullptr;
    if (suites.descriptor->Make(&d) != 0 || !d) return;
    PsddsSettings copy = s;
    bool ok = true;
    for (int i = 0; i < kCount && ok; i++) ok = suites.descriptor->PutInteger(d, kKeys[i], int32(*field(copy, i))) == 0;
    if (ok) suites.registry->Register(kRegistryKey, d, true);
    suites.descriptor->Free(d); // the caller frees its descriptor after Register
}
