#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct NativeResult NativeResult;
extern NativeResult *g0_compiled_entry(const unsigned char *, size_t);
extern int32_t g0_runtime_status(const NativeResult *);
extern const unsigned char *g0_runtime_bytes(const NativeResult *, size_t *);
extern void g0_runtime_free(NativeResult *);

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    FILE *file = fopen(argv[1], "rb");
    if (!file) return 3;
    unsigned char *input = malloc(131073);
    if (!input) { fclose(file); return 4; }
    size_t length = fread(input, 1, 131073, file);
    int failed = ferror(file);
    fclose(file);
    if (failed || length > 131072) { free(input); return 5; }
    NativeResult *result = g0_compiled_entry(input, length);
    free(input);
    if (g0_runtime_status(result)) { g0_runtime_free(result); return 6; }
    size_t output_length = 0;
    const unsigned char *output = g0_runtime_bytes(result, &output_length);
    if (!output) { g0_runtime_free(result); return 7; }
    int ok = fwrite(output, 1, output_length, stdout) == output_length;
    g0_runtime_free(result);
    return ok ? 0 : 8;
}
