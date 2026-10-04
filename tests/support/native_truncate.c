#include <stdint.h>
#include <stdio.h>
#include <string.h>

static uint64_t reference(int64_t value, unsigned bits, unsigned is_signed) {
    uint64_t mask = bits == 64 ? UINT64_MAX : (UINT64_C(1) << bits) - 1;
    uint64_t low = (uint64_t)value & mask;
    if (is_signed && (low & (UINT64_C(1) << (bits - 1)))) {
        low |= ~mask;
    }
    return low;
}

static int compare(uint64_t (*function)(int64_t), int64_t value,
                   unsigned bits, unsigned is_signed) {
    uint64_t result = function(value);
    uint64_t expected = reference(value, bits, is_signed);
    if (result != expected) {
        fprintf(stderr, "bits=%u signed=%u input=%lld result=%llu expected=%llu\n",
                bits, is_signed, (long long)value,
                (unsigned long long)result, (unsigned long long)expected);
        return 1;
    }
    return 0;
}

static int check(uint64_t (*function)(int64_t), uint64_t (*folded)(void),
                 unsigned bits, unsigned is_signed) {
    const int64_t values[] = {INT64_MIN, INT64_MAX, -1, 0, 1, -257, 257};
    for (unsigned i = 0; i < sizeof(values) / sizeof(values[0]); ++i) {
        if (compare(function, values[i], bits, is_signed)) return 1;
    }
    // Probe each sign boundary and each wrap boundary as raw bit patterns.
    uint64_t boundaries[] = {UINT64_C(1) << (bits - 1),
                             bits == 64 ? 0 : UINT64_C(1) << bits};
    for (unsigned i = 0; i < 2; ++i) {
        for (unsigned offset = 0; offset < 3; ++offset) {
            uint64_t raw = boundaries[i] + offset - 1;
            int64_t value;
            memcpy(&value, &raw, sizeof(value));
            if (compare(function, value, bits, is_signed)) return 1;
        }
    }
    // Reproducible samples spanning the entire input domain.
    uint64_t state = UINT64_C(0x9e3779b97f4a7c15);
    for (unsigned i = 0; i < 1024; ++i) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        int64_t value;
        memcpy(&value, &state, sizeof(value));
        if (compare(function, value, bits, is_signed)) return 1;
    }
    if (folded() != reference(-1, bits, is_signed)) {
        fprintf(stderr, "folded mismatch: bits=%u signed=%u\n", bits, is_signed);
        return 1;
    }
    return 0;
}
