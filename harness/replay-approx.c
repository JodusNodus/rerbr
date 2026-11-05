// rsqrtss/rcpss tables of the reference CPU (the test machine, i9-10980HK), for
// harness/replay.py: the emulator computes them exactly, the hardware approximates.
// Both results depend on 11 input bits only (verified over every input in [1, 4)):
// rsqrt on the exponent's parity and the top 10 mantissa bits, rcp on the top 11.
// Writes rsqrt.bin (2048 rsqrt results for 1 + i/1024, i < 1024, then 2 + (i - 1024)/512) and
// rcp.bin (2048 rcp results for 1 + i/2048), u32 float bits each; move them into
// crates/rbr-physics/data/ (what harness/replay.py reads).
// gcc -O1 -msse -o approx replay-approx.c && ./approx
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <xmmintrin.h>
static float f(uint32_t u) { float x; memcpy(&x, &u, 4); return x; }
static uint32_t u(float x) { uint32_t v; memcpy(&v, &x, 4); return v; }
int main(void)
{
    FILE* o = fopen("rsqrt.bin", "wb");
    for (uint32_t i = 0; i < 2048; i++)
    {
        uint32_t r = u(_mm_cvtss_f32(_mm_rsqrt_ss(_mm_set_ss(f((127u << 23) + (i << 13))))));
        fwrite(&r, 4, 1, o);
    }
    fclose(o);
    o = fopen("rcp.bin", "wb");
    for (uint32_t i = 0; i < 2048; i++)
    {
        uint32_t r = u(_mm_cvtss_f32(_mm_rcp_ss(_mm_set_ss(f((127u << 23) + (i << 12))))));
        fwrite(&r, 4, 1, o);
    }
    fclose(o);
    return 0;
}
