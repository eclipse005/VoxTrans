#include <stdint.h>

extern "C" void moe_gemm_wmma(
    const void*, const void*, const int32_t*, const int32_t*, const float*, void*, int32_t*,
    int32_t*, int32_t, int32_t, int32_t, int32_t, int32_t, int32_t, bool, int64_t) {}

extern "C" void moe_gemm_gguf(
    const float*, const void*, const int32_t*, const int32_t*, const float*, void*, int32_t,
    int32_t, int32_t, int32_t, int32_t, int32_t, int64_t) {}

extern "C" void moe_gemm_gguf_prefill(
    const void*, const uint8_t*, const int32_t*, const int32_t*, const float*, void*, int32_t,
    int32_t, int32_t, int32_t, int32_t, int32_t, int32_t, int64_t) {}
