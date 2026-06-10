#define _USE_MATH_DEFINES
#include<math.h>
#include<stdint.h>
#include "cuda_utils.cuh"

#define UNARY_OP(TYPENAME, FN_NAME, FUNC) \
extern "C" __global__ void FN_NAME( \
    const size_t numel, \
    const size_t num_dims, \
    const size_t *info, \
    const TYPENAME *inp, \
    TYPENAME *out \
) { \
    const size_t *dims = info; \
    const size_t *strides = info + num_dims; \
    if (info == nullptr || is_contiguous(num_dims, dims, strides)) { \
        for (unsigned int i = blockIdx.x * blockDim.x + threadIdx.x; i < numel; i += blockDim.x * gridDim.x) { \
            TYPENAME x = inp ? inp[i] : out[i]; \
            out[i] = FUNC; \
        } \
    } \
    else { \
        for (unsigned int i = blockIdx.x * blockDim.x + threadIdx.x; i < numel; i += blockDim.x * gridDim.x) { \
            unsigned strided_i = get_strided_index(i, num_dims, dims, strides); \
            TYPENAME x = inp ? inp[strided_i] : out[i]; \
            out[i] = FUNC; \
        } \
    } \
} \

template<typename T>
__device__ __forceinline__ T gelu_erf_fwd(T x) {
  return x * normcdfg(x);
}

template<typename T>
__device__ __forceinline__ T gelu_fwd(T x) {
    T x_sq = x * x;
    T x_cube = x_sq * x;
    T alpha = x + static_cast<T>(0.044715) * x_cube;
    return static_cast<T>(0.5) * x * (static_cast<T>(1.0) + tanhg(static_cast<T>(M_2_SQRTPI * M_SQRT1_2) * alpha));
}

template<typename T>
__device__ __forceinline__ T elu_fwd(T x, T alpha) {
  if (x > static_cast<T>(0)) {
    return x;
  }
  return alpha * (expg(x) - static_cast<T>(1));
}

template<typename T>
__device__ __forceinline__ T relu_fwd(T x) {
    T zero = 0.;
    return maxg(x, zero);
}

template<typename T>
__device__ __forceinline__ T silu_fwd(T x) {
    return x / (static_cast<T>(1) + expg(-x));
}

#define FUSED_BIAS_SILU(TYPENAME, FN_NAME) \
extern "C" __global__ void FN_NAME( \
    const TYPENAME *inp, \
    const TYPENAME *bias, \
    TYPENAME *out, \
    const int cols, \
    const int total \
) { \
    for (unsigned int i = blockIdx.x * blockDim.x + threadIdx.x; i < total; i += blockDim.x * gridDim.x) { \
        TYPENAME x = inp[i] + bias[i % cols]; \
        out[i] = silu_fwd(x); \
    } \
}

FUSED_BIAS_SILU(float, fused_bias_silu_f32)

#if __CUDA_ARCH__ >= 530
FUSED_BIAS_SILU(__half, fused_bias_silu_f16)
#endif

#define FUSED_BIAS_RESIDUAL(TYPENAME, FN_NAME) \
extern "C" __global__ void FN_NAME( \
    const TYPENAME *inp, \
    const TYPENAME *bias, \
    const TYPENAME *residual, \
    TYPENAME *out, \
    const int cols, \
    const int total \
) { \
    for (unsigned int i = blockIdx.x * blockDim.x + threadIdx.x; i < total; i += blockDim.x * gridDim.x) { \
        out[i] = inp[i] + bias[i % cols] + residual[i]; \
    } \
}

FUSED_BIAS_RESIDUAL(float, fused_bias_residual_f32)

#if __CUDA_ARCH__ >= 530
FUSED_BIAS_RESIDUAL(__half, fused_bias_residual_f16)
#endif

#define FUSED_ATTENTION_SCORES_SHIFTED(TYPENAME, FN_NAME) \
extern "C" __global__ void FN_NAME( \
    const TYPENAME *matrix_ac, \
    const TYPENAME *matrix_bd, \
    TYPENAME *out, \
    const int q_len, \
    const int k_len, \
    const int pos_len, \
    const float scale, \
    const int total \
) { \
    for (unsigned int i = blockIdx.x * blockDim.x + threadIdx.x; i < total; i += blockDim.x * gridDim.x) { \
        int h = i / (q_len * k_len); \
        int rem = i - h * q_len * k_len; \
        int q = rem / k_len; \
        int k = rem - q * k_len; \
        int bd_pos = (k_len - 1 - q) + k; \
        int bd_i = h * q_len * pos_len + q * pos_len + bd_pos; \
        out[i] = (matrix_ac[i] + matrix_bd[bd_i]) * static_cast<TYPENAME>(scale); \
    } \
}

FUSED_ATTENTION_SCORES_SHIFTED(float, fused_attention_scores_shifted_f32)

#if __CUDA_ARCH__ >= 530
FUSED_ATTENTION_SCORES_SHIFTED(__half, fused_attention_scores_shifted_f16)
#endif

#define QKV_SPLIT_TRANSPOSE_BIAS(TYPENAME, FN_NAME) \
extern "C" __global__ void FN_NAME( \
    const TYPENAME *qkv, \
    const TYPENAME *bias, \
    TYPENAME *out, \
    const int tokens, \
    const int heads, \
    const int head_dim, \
    const int total \
) { \
    int part_stride = heads * tokens * head_dim; \
    int model_dim = heads * head_dim; \
    int qkv_dim = 3 * model_dim; \
    for (unsigned int i = blockIdx.x * blockDim.x + threadIdx.x; i < total; i += blockDim.x * gridDim.x) { \
        int part = i / part_stride; \
        int rem = i - part * part_stride; \
        int h = rem / (tokens * head_dim); \
        rem -= h * tokens * head_dim; \
        int t = rem / head_dim; \
        int d = rem - t * head_dim; \
        int offset = part * model_dim + h * head_dim + d; \
        int src_i = t * qkv_dim + offset; \
        out[i] = qkv[src_i] + bias[offset]; \
    } \
}

QKV_SPLIT_TRANSPOSE_BIAS(float, qkv_split_transpose_bias_f32)

#if __CUDA_ARCH__ >= 530
QKV_SPLIT_TRANSPOSE_BIAS(__half, qkv_split_transpose_bias_f16)
#endif

#define POS_SPLIT_TRANSPOSE(TYPENAME, FN_NAME) \
extern "C" __global__ void FN_NAME( \
    const TYPENAME *pos, \
    TYPENAME *out, \
    const int pos_len, \
    const int heads, \
    const int head_dim, \
    const int total \
) { \
    int model_dim = heads * head_dim; \
    for (unsigned int i = blockIdx.x * blockDim.x + threadIdx.x; i < total; i += blockDim.x * gridDim.x) { \
        int h = i / (pos_len * head_dim); \
        int rem = i - h * pos_len * head_dim; \
        int p = rem / head_dim; \
        int d = rem - p * head_dim; \
        int src_i = p * model_dim + h * head_dim + d; \
        out[i] = pos[src_i]; \
    } \
}

POS_SPLIT_TRANSPOSE(float, pos_split_transpose_f32)

#if __CUDA_ARCH__ >= 530
POS_SPLIT_TRANSPOSE(__half, pos_split_transpose_f16)
#endif

#define Q_BIASES(TYPENAME, FN_NAME) \
extern "C" __global__ void FN_NAME( \
    const TYPENAME *q, \
    const TYPENAME *bias_u, \
    const TYPENAME *bias_v, \
    TYPENAME *out, \
    const int tokens, \
    const int head_dim, \
    const int per_part, \
    const int total \
) { \
    for (unsigned int i = blockIdx.x * blockDim.x + threadIdx.x; i < total; i += blockDim.x * gridDim.x) { \
        int part = i / per_part; \
        int rem = i - part * per_part; \
        int h = rem / (tokens * head_dim); \
        rem -= h * tokens * head_dim; \
        int t = rem / head_dim; \
        int d = rem - t * head_dim; \
        int q_i = h * tokens * head_dim + t * head_dim + d; \
        int b_i = h * head_dim + d; \
        out[i] = q[q_i] + (part == 0 ? bias_u[b_i] : bias_v[b_i]); \
    } \
}

Q_BIASES(float, q_biases_f32)

#if __CUDA_ARCH__ >= 530
Q_BIASES(__half, q_biases_f16)
#endif

extern "C" __global__ void softmax_last_dim_f32(
    const float *inp,
    float *out,
    const int cols
) {
    extern __shared__ float shared[];
    int row = blockIdx.x;
    int base = row * cols;
    float local_max = -INFINITY;
    for (int c = threadIdx.x; c < cols; c += blockDim.x) {
        local_max = fmaxf(local_max, inp[base + c]);
    }
    shared[threadIdx.x] = local_max;
    __syncthreads();
    for (int stride = blockDim.x / 2; stride > 0; stride >>= 1) {
        if (threadIdx.x < stride) {
            shared[threadIdx.x] = fmaxf(shared[threadIdx.x], shared[threadIdx.x + stride]);
        }
        __syncthreads();
    }
    float max_val = shared[0];
    float local_sum = 0.0f;
    for (int c = threadIdx.x; c < cols; c += blockDim.x) {
        float e = expf(inp[base + c] - max_val);
        out[base + c] = e;
        local_sum += e;
    }
    shared[threadIdx.x] = local_sum;
    __syncthreads();
    for (int stride = blockDim.x / 2; stride > 0; stride >>= 1) {
        if (threadIdx.x < stride) {
            shared[threadIdx.x] += shared[threadIdx.x + stride];
        }
        __syncthreads();
    }
    float inv_sum = 1.0f / shared[0];
    for (int c = threadIdx.x; c < cols; c += blockDim.x) {
        out[base + c] *= inv_sum;
    }
}

#if __CUDA_ARCH__ >= 530
extern "C" __global__ void softmax_last_dim_f16(
    const __half *inp,
    __half *out,
    const int cols
) {
    extern __shared__ float shared[];
    int row = blockIdx.x;
    int base = row * cols;
    float local_max = -INFINITY;
    for (int c = threadIdx.x; c < cols; c += blockDim.x) {
        local_max = fmaxf(local_max, __half2float(inp[base + c]));
    }
    shared[threadIdx.x] = local_max;
    __syncthreads();
    for (int stride = blockDim.x / 2; stride > 0; stride >>= 1) {
        if (threadIdx.x < stride) {
            shared[threadIdx.x] = fmaxf(shared[threadIdx.x], shared[threadIdx.x + stride]);
        }
        __syncthreads();
    }
    float max_val = shared[0];
    float local_sum = 0.0f;
    for (int c = threadIdx.x; c < cols; c += blockDim.x) {
        float e = expf(__half2float(inp[base + c]) - max_val);
        out[base + c] = __float2half(e);
        local_sum += e;
    }
    shared[threadIdx.x] = local_sum;
    __syncthreads();
    for (int stride = blockDim.x / 2; stride > 0; stride >>= 1) {
        if (threadIdx.x < stride) {
            shared[threadIdx.x] += shared[threadIdx.x + stride];
        }
        __syncthreads();
    }
    float inv_sum = 1.0f / shared[0];
    for (int c = threadIdx.x; c < cols; c += blockDim.x) {
        out[base + c] = __float2half(__half2float(out[base + c]) * inv_sum);
    }
}
#endif

extern "C" __global__ void scaled_softmax_last_dim_f32(
    const float *inp,
    float *out,
    const int rows,
    const int cols,
    const float scale,
    const int causal
) {
    extern __shared__ float shared[];
    int row = blockIdx.x;
    int q = row % rows;
    int base = row * cols;
    float local_max = -INFINITY;
    for (int c = threadIdx.x; c < cols; c += blockDim.x) {
        float v = (causal != 0 && c > q) ? -INFINITY : inp[base + c] * scale;
        local_max = fmaxf(local_max, v);
    }
    shared[threadIdx.x] = local_max;
    __syncthreads();
    for (int stride = blockDim.x / 2; stride > 0; stride >>= 1) {
        if (threadIdx.x < stride) {
            shared[threadIdx.x] = fmaxf(shared[threadIdx.x], shared[threadIdx.x + stride]);
        }
        __syncthreads();
    }
    float max_val = shared[0];
    float local_sum = 0.0f;
    for (int c = threadIdx.x; c < cols; c += blockDim.x) {
        float e = (causal != 0 && c > q) ? 0.0f : expf(inp[base + c] * scale - max_val);
        out[base + c] = e;
        local_sum += e;
    }
    shared[threadIdx.x] = local_sum;
    __syncthreads();
    for (int stride = blockDim.x / 2; stride > 0; stride >>= 1) {
        if (threadIdx.x < stride) {
            shared[threadIdx.x] += shared[threadIdx.x + stride];
        }
        __syncthreads();
    }
    float inv_sum = 1.0f / shared[0];
    for (int c = threadIdx.x; c < cols; c += blockDim.x) {
        out[base + c] *= inv_sum;
    }
}

#if __CUDA_ARCH__ >= 530
extern "C" __global__ void scaled_softmax_last_dim_f16(
    const __half *inp,
    __half *out,
    const int rows,
    const int cols,
    const float scale,
    const int causal
) {
    extern __shared__ float shared[];
    int row = blockIdx.x;
    int q = row % rows;
    int base = row * cols;
    float local_max = -INFINITY;
    for (int c = threadIdx.x; c < cols; c += blockDim.x) {
        float v = (causal != 0 && c > q) ? -INFINITY : __half2float(inp[base + c]) * scale;
        local_max = fmaxf(local_max, v);
    }
    shared[threadIdx.x] = local_max;
    __syncthreads();
    for (int stride = blockDim.x / 2; stride > 0; stride >>= 1) {
        if (threadIdx.x < stride) {
            shared[threadIdx.x] = fmaxf(shared[threadIdx.x], shared[threadIdx.x + stride]);
        }
        __syncthreads();
    }
    float max_val = shared[0];
    float local_sum = 0.0f;
    for (int c = threadIdx.x; c < cols; c += blockDim.x) {
        float e = (causal != 0 && c > q) ? 0.0f : expf(__half2float(inp[base + c]) * scale - max_val);
        out[base + c] = __float2half(e);
        local_sum += e;
    }
    shared[threadIdx.x] = local_sum;
    __syncthreads();
    for (int stride = blockDim.x / 2; stride > 0; stride >>= 1) {
        if (threadIdx.x < stride) {
            shared[threadIdx.x] += shared[threadIdx.x + stride];
        }
        __syncthreads();
    }
    float inv_sum = 1.0f / shared[0];
    for (int c = threadIdx.x; c < cols; c += blockDim.x) {
        out[base + c] = __float2half(__half2float(out[base + c]) * inv_sum);
    }
}
#endif

template<typename T>
__device__ __forceinline__ T sigmoid_fwd(T x) {
    return recipg(static_cast<T>(1) + expg(-x));
}

#define UNARY_OP1(TYPENAME, FN_NAME, FUNC) \
extern "C" __global__ void FN_NAME( \
    const size_t numel, \
    const size_t num_dims, \
    const size_t *info, \
    const TYPENAME param, \
    const TYPENAME *inp, \
    TYPENAME *out \
) { \
    const size_t *dims = info; \
    const size_t *strides = info + num_dims; \
    if (info == nullptr || is_contiguous(num_dims, dims, strides)) { \
        for (unsigned int i = blockIdx.x * blockDim.x + threadIdx.x; i < numel; i += blockDim.x * gridDim.x) { \
            TYPENAME x = inp ? inp[i] : out[i]; \
            out[i] = FUNC; \
        } \
    } \
    else { \
        for (unsigned int i = blockIdx.x * blockDim.x + threadIdx.x; i < numel; i += blockDim.x * gridDim.x) { \
            unsigned strided_i = get_strided_index(i, num_dims, dims, strides); \
            TYPENAME x = inp ? inp[strided_i] : out[i]; \
            out[i] = FUNC; \
        } \
    } \
} \

template<typename T>
__device__ T sign_(T t) {
  return static_cast<T>(t > static_cast<T>(0)) - static_cast<T>(t < static_cast<T>(0));
}


#if __CUDA_ARCH__ >= 800
UNARY_OP(__nv_bfloat16, ucopy_bf16, x)
UNARY_OP(__nv_bfloat16, uneg_bf16, -x)
UNARY_OP(__nv_bfloat16, urecip_bf16, recipg(x))
UNARY_OP(__nv_bfloat16, uexp_bf16, expg(x))
UNARY_OP(__nv_bfloat16, ulog_bf16, logg(x))
UNARY_OP(__nv_bfloat16, usin_bf16, sing(x))
UNARY_OP(__nv_bfloat16, ucos_bf16, cosg(x))
UNARY_OP(__nv_bfloat16, utanh_bf16, tanhg(x))
UNARY_OP(__nv_bfloat16, uerf_bf16, erfg(x))
UNARY_OP(__nv_bfloat16, uceil_bf16, ceilg(x))
UNARY_OP(__nv_bfloat16, ufloor_bf16, floorg(x))
UNARY_OP(__nv_bfloat16, uround_bf16, roundg(x))
UNARY_OP(__nv_bfloat16, unormcdf_bf16, normcdfg(x))
UNARY_OP(__nv_bfloat16, uabs_bf16, absg(x))
UNARY_OP(__nv_bfloat16, usqr_bf16, x*x)
UNARY_OP(__nv_bfloat16, usqrt_bf16, sqrtg(x))
UNARY_OP(__nv_bfloat16, ugelu_bf16, gelu_fwd(x))
UNARY_OP(__nv_bfloat16, ugelu_erf_bf16, gelu_erf_fwd(x))
UNARY_OP(__nv_bfloat16, urelu_bf16, relu_fwd(x))
UNARY_OP1(__nv_bfloat16, uelu_bf16, elu_fwd(x, param))
UNARY_OP(__nv_bfloat16, usilu_bf16, silu_fwd(x))
UNARY_OP1(__nv_bfloat16, upowf_bf16, powg(x, param))
UNARY_OP(__nv_bfloat16, usign_bf16, sign_(x))
UNARY_OP(__nv_bfloat16, usigmoid_bf16, sigmoid_fwd(x))
#endif

#if __CUDA_ARCH__ >= 890
#define F8E4M3_TO_FLOAT(x) __half2float(__nv_cvt_fp8_to_halfraw(x.__x, __NV_E4M3))

UNARY_OP(__nv_fp8_e4m3, ucopy_f8_e4m3, x)
UNARY_OP(__nv_fp8_e4m3, uneg_fp8_e4m3, __nv_fp8_e4m3(-F8E4M3_TO_FLOAT(x)))
UNARY_OP(__nv_fp8_e4m3, urecip_fp8_e4m3, recipg(x))
UNARY_OP(__nv_fp8_e4m3, uexp_fp8_e4m3, expg(x))
UNARY_OP(__nv_fp8_e4m3, ulog_fp8_e4m3, logg(x))
UNARY_OP(__nv_fp8_e4m3, usin_fp8_e4m3, sing(x))
UNARY_OP(__nv_fp8_e4m3, ucos_fp8_e4m3, cosg(x))
UNARY_OP(__nv_fp8_e4m3, utanh_fp8_e4m3, tanhg(x))
UNARY_OP(__nv_fp8_e4m3, uerf_fp8_e4m3, erfg(x))
UNARY_OP(__nv_fp8_e4m3, uceil_fp8_e4m3, ceilg(x))
UNARY_OP(__nv_fp8_e4m3, ufloor_fp8_e4m3, floorg(x))
UNARY_OP(__nv_fp8_e4m3, uround_fp8_e4m3, roundg(x))
UNARY_OP(__nv_fp8_e4m3, unormcdf_fp8_e4m3, normcdfg(x))
UNARY_OP(__nv_fp8_e4m3, uabs_fp8_e4m3, absg(x))
UNARY_OP(__nv_fp8_e4m3, usqr_fp8_e4m3, __nv_fp8_e4m3(F8E4M3_TO_FLOAT(x)*F8E4M3_TO_FLOAT(x)))
UNARY_OP(__nv_fp8_e4m3, usqrt_fp8_e4m3, sqrtg(x))
UNARY_OP(__nv_fp8_e4m3, ugelu_fp8_e4m3, __nv_fp8_e4m3(gelu_fwd(F8E4M3_TO_FLOAT(x))))
UNARY_OP(__nv_fp8_e4m3, ugelu_erf_fp8_e4m3, __nv_fp8_e4m3(gelu_erf_fwd(F8E4M3_TO_FLOAT(x))))
UNARY_OP(__nv_fp8_e4m3, urelu_fp8_e4m3, __nv_fp8_e4m3(relu_fwd(F8E4M3_TO_FLOAT(x))))
UNARY_OP1(__nv_fp8_e4m3, uelu_fp8_e4m3, __nv_fp8_e4m3(elu_fwd(F8E4M3_TO_FLOAT(x), F8E4M3_TO_FLOAT(param))))
UNARY_OP(__nv_fp8_e4m3, usilu_fp8_e4m3, __nv_fp8_e4m3(silu_fwd(F8E4M3_TO_FLOAT(x))))
UNARY_OP1(__nv_fp8_e4m3, upowf_fp8_e4m3, powg(x, param))
UNARY_OP(__nv_fp8_e4m3, usign_fp8_e4m3, __nv_fp8_e4m3(sign_(F8E4M3_TO_FLOAT(x))))
UNARY_OP(__nv_fp8_e4m3, usigmoid_fp8_e4m3, __nv_fp8_e4m3(sigmoid_fwd(F8E4M3_TO_FLOAT(x))))
#endif

#if __CUDA_ARCH__ >= 530
UNARY_OP(__half, ucopy_f16, x)
UNARY_OP(__half, uneg_f16, -x)
UNARY_OP(__half, urecip_f16, recipg(x))
UNARY_OP(__half, uexp_f16, expg(x))
UNARY_OP(__half, ulog_f16, logg(x))
UNARY_OP(__half, usin_f16, sing(x))
UNARY_OP(__half, ucos_f16, cosg(x))
UNARY_OP(__half, utanh_f16, tanhg(x))
UNARY_OP(__half, uerf_f16, erfg(x))
UNARY_OP(__half, uceil_f16, ceilg(x))
UNARY_OP(__half, ufloor_f16, floorg(x))
UNARY_OP(__half, uround_f16, roundg(x))
UNARY_OP(__half, unormcdf_f16, normcdfg(x))
UNARY_OP(__half, uabs_f16, absg(x))
UNARY_OP(__half, usqr_f16, x*x)
UNARY_OP(__half, usqrt_f16, sqrtg(x))
UNARY_OP(__half, ugelu_f16, gelu_fwd(x))
UNARY_OP(__half, ugelu_erf_f16, gelu_erf_fwd(x))
UNARY_OP(__half, urelu_f16, relu_fwd(x))
UNARY_OP1(__half, uelu_f16, elu_fwd(x, param))
UNARY_OP(__half, usilu_f16, silu_fwd(x))
UNARY_OP1(__half, upowf_f16, powg(x, param))
UNARY_OP(__half, usign_f16, sign_(x))
UNARY_OP(__half, usigmoid_f16, sigmoid_fwd(x))
#endif

UNARY_OP(uint8_t, ucopy_u8, x)
UNARY_OP(uint32_t, ucopy_u32, x)
UNARY_OP(int64_t, ucopy_i64, x)
UNARY_OP(float, ucopy_f32, x)
UNARY_OP(double, ucopy_f64, x)
UNARY_OP(float, uneg_f32, -x)
UNARY_OP(double, uneg_f64, -x)
UNARY_OP(float, urecip_f32, recipg(x))
UNARY_OP(double, urecip_f64, recipg(x))
UNARY_OP(float, uexp_f32, expg(x))
UNARY_OP(double, uexp_f64, expg(x))
UNARY_OP(float, ulog_f32, logg(x))
UNARY_OP(double, ulog_f64, logg(x))
UNARY_OP(float, usin_f32, sing(x))
UNARY_OP(double, usin_f64, sing(x))
UNARY_OP(float, ucos_f32, cosg(x))
UNARY_OP(double, ucos_f64, cosg(x))
UNARY_OP(float, utanh_f32, tanhg(x))
UNARY_OP(double, utanh_f64, tanhg(x))
UNARY_OP(float, uerf_f32, erfg(x))
UNARY_OP(double, uerf_f64, erfg(x))
UNARY_OP(float, uceil_f32, ceilg(x))
UNARY_OP(double, uceil_f64, ceilg(x))
UNARY_OP(float, ufloor_f32, floorg(x))
UNARY_OP(double, ufloor_f64, floorg(x))
UNARY_OP(float, uround_f32, roundg(x))
UNARY_OP(double, uround_f64, roundg(x))
UNARY_OP(float, unormcdf_f32, normcdfg(x))
UNARY_OP(double, unormcdf_f64, normcdfg(x))
UNARY_OP(float, uabs_f32, absg(x))
UNARY_OP(double, uabs_f64, absg(x))
UNARY_OP(float, usqr_f32, x*x)
UNARY_OP(double, usqr_f64, x*x)
UNARY_OP(float, usqrt_f32, sqrtg(x))
UNARY_OP(double, usqrt_f64, sqrtg(x))
UNARY_OP(float, ugelu_f32, gelu_fwd(x))
UNARY_OP(double, ugelu_f64, gelu_fwd(x))
UNARY_OP(float, ugelu_erf_f32, gelu_erf_fwd(x))
UNARY_OP(double, ugelu_erf_f64, gelu_erf_fwd(x))
UNARY_OP(float, urelu_f32, relu_fwd(x))
UNARY_OP(double, urelu_f64, relu_fwd(x))
UNARY_OP1(float, uelu_f32, elu_fwd(x, param))
UNARY_OP1(double, uelu_f64, elu_fwd(x, param))
UNARY_OP(float, usilu_f32, silu_fwd(x))
UNARY_OP(double, usilu_f64, silu_fwd(x))
UNARY_OP1(float, upowf_f32, powg(x, param))
UNARY_OP1(double, upowf_f64, powg(x, param))
UNARY_OP(float, usign_f32, sign_(x))
UNARY_OP(double, usign_f64, sign_(x))
UNARY_OP(float, usigmoid_f32, sigmoid_fwd(x))
UNARY_OP(double, usigmoid_f64, sigmoid_fwd(x))

extern "C" __global__ void fused_glu_depthwise_silu_tc_bias_f16(
    const half* input, const half* pw_bias, const half* dw_params, half* output,
    int channels, int tokens) {
  int idx = blockIdx.x * blockDim.x + threadIdx.x;
  int total = channels * tokens;
  if (idx >= total) return;

  int t = idx % tokens;
  int c = idx / tokens;
  int two_channels = channels * 2;
  float acc = 0.0f;
  for (int k = 0; k < 9; ++k) {
    int src_t = t + k - 4;
    if (src_t < 0 || src_t >= tokens) continue;
    int base = src_t * two_channels;
    float left = __half2float(input[base + c]) + __half2float(pw_bias[c]);
    float right = __half2float(input[base + channels + c]) + __half2float(pw_bias[channels + c]);
    float gate = 1.0f / (1.0f + expf(-right));
    acc += left * gate * __half2float(dw_params[c * 10 + k]);
  }
  acc += __half2float(dw_params[c * 10 + 9]);
  output[idx] = __float2half_rn(acc / (1.0f + expf(-acc)));
}

extern "C" __global__ void fused_glu_depthwise_silu_tc_bias_f32(
    const float* input, const float* pw_bias, const float* dw_params, float* output,
    int channels, int tokens) {
  int idx = blockIdx.x * blockDim.x + threadIdx.x;
  int total = channels * tokens;
  if (idx >= total) return;

  int t = idx % tokens;
  int c = idx / tokens;
  int two_channels = channels * 2;
  float acc = 0.0f;
  for (int k = 0; k < 9; ++k) {
    int src_t = t + k - 4;
    if (src_t < 0 || src_t >= tokens) continue;
    int base = src_t * two_channels;
    float left = input[base + c] + pw_bias[c];
    float right = input[base + channels + c] + pw_bias[channels + c];
    float gate = 1.0f / (1.0f + expf(-right));
    acc += left * gate * dw_params[c * 10 + k];
  }
  acc += dw_params[c * 10 + 9];
  output[idx] = acc / (1.0f + expf(-acc));
}
