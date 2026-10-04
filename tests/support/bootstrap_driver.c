#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct NativeResult NativeResult;
extern NativeResult *g0_compiled_entry(const unsigned char *, size_t);
typedef struct { uint64_t max_steps, max_value_bytes, max_call_depth; } NativeLimits;
extern NativeResult *g0_compiled_entry_with_limits(const unsigned char *, size_t, const NativeLimits *);
extern int32_t g0_runtime_status(const NativeResult *);
extern int32_t g0_runtime_failure_kind(const NativeResult *);
extern const unsigned char *g0_runtime_bytes(const NativeResult *, size_t *);
extern void g0_runtime_free(NativeResult *);

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    FILE *file = fopen(argv[1], "rb");
    if (!file) return 3;
    unsigned char *input = malloc(4194305);
    if (!input) { fclose(file); return 4; }
    size_t length = fread(input, 1, 4194305, file);
    int failed = ferror(file);
    fclose(file);
    if (failed || length > 4194304) { free(input); return 5; }
    const NativeLimits limits = { 64000000, UINT64_C(16) * 1024 * 1024 * 1024, 128 };
    NativeResult *result = g0_compiled_entry_with_limits(input, length, &limits);
    free(input);
    if (g0_runtime_status(result)) {
        fprintf(stderr, "G0 compiler runtime failure kind: %d\n", g0_runtime_failure_kind(result));
        g0_runtime_free(result);
        return 6;
    }
    size_t output_length = 0;
    const unsigned char *output = g0_runtime_bytes(result, &output_length);
    if (!output) { g0_runtime_free(result); return 7; }
    // This driver hosts the compiler's Text protocol, including diagnostics.
    // A diagnostic must never be published as successful assembly.
    if (output_length < 6 || memcmp(output, ".text\n", 6) != 0) {
        g0_runtime_free(result);
        return 9;
    }
    int ok = fwrite(output, 1, output_length, stdout) == output_length;
    g0_runtime_free(result);
    return ok ? 0 : 8;
}
