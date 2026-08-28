#include "TruvixxAssimp/base_type.h"

static_assert(
    sizeof(TruvixxFloat4x4) == sizeof(float) * 16 && alignof(TruvixxFloat4x4) == sizeof(float),
    "TruvixxFloat4x4 size mismatch"
);

static_assert(
    sizeof(TruvixxFloat3x3) == sizeof(float) * 9 && alignof(TruvixxFloat3x3) == sizeof(float),
    "TruvixxFloat3x3 size mismatch"
);

static_assert(
    sizeof(TruvixxFloat4) == sizeof(float) * 4 && alignof(TruvixxFloat4) == sizeof(float),
    "TruvixxFloat4 size mismatch"
);

static_assert(
    sizeof(TruvixxFloat3) == sizeof(float) * 3 && alignof(TruvixxFloat3) == sizeof(float),
    "TruvixxFloat3 size mismatch"
);

static_assert(
    sizeof(TruvixxFloat2) == sizeof(float) * 2 && alignof(TruvixxFloat2) == sizeof(float),
    "TruvixxFloat2 size mismatch"
);
